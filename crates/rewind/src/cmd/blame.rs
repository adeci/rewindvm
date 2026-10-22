//! `rewind blame`: why a file came out different in two runs of the same
//! build. It works from what runs record today, the opens for writing,
//! renames, execs and forks in the trace, and reads files at steps by
//! forking the run, as `rewind cat` does.
//!
//! The file's bytes are compared at each run's end. The writer of the
//! file is the last process to open it for writing, following renames
//! back to the file renamed into place. A writer that copied another file
//! of the run, such as `install` copying a build product into an output,
//! is followed back to that file. At the process that made the bytes, the
//! step a differing byte took its final value is found by bisection over
//! the steps the file was open, and the two writers' command lines are
//! compared. Arguments the two runs give in another order are traced to
//! the step each run created them on, since on the VM's tmpfs a directory
//! lists its newest entries first.
//!
//! No run records its writes yet, so every writer is found by forking,
//! and `blame` warns before it starts what that may cost.

use std::process::ExitCode;

use anyhow::{Result, bail};
use rewind_core::inspect::Inspection;
use rewind_core::{Home, Run};
use rewind_trace::{Event, EventKind, Process, Trace};

use crate::{RUN_HELP, RUN_LONG_HELP};

/// How many bytes from the first differing offset are shown and bisected
/// on.
const WINDOW: usize = 16;

/// How many copies `blame` follows back before it gives up, so a cycle of
/// copies cannot loop forever.
const MAX_HOPS: usize = 8;

/// How many earlier files of the same name are compared with a file a
/// program made, looking for the one it copied.
const MAX_SAME_NAME: usize = 8;

/// Programs whose writes are copies of a file named in their arguments.
const COPIERS: &[&str] = &["install", "cp", "mv", "ln", "rsync", "cat"];

/// Whether a run recorded its writes to files and its opens for reading.
/// Runs with these records name each file's writers directly; runs
/// without them leave `blame` to find writers by forking and bisecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IoRecords {
    Absent,
    // Nothing makes a run with records yet; the tests mark runs this way.
    #[allow(dead_code)]
    Present,
}

/// What forking a run to find its writers costs: how far one probe may
/// replay, and how fast the run went when it was recorded.
#[derive(Clone, Debug)]
struct ProbeCost {
    id: String,
    steps: u64,
    /// The steps the run has keyframes at; a probe replays from the
    /// nearest one at or before its step, or from boot.
    keyframes: Vec<u64>,
    /// How long the run took to record, when it finished.
    wall_ms: Option<u64>,
    records: IoRecords,
}

impl ProbeCost {
    fn of(run: &Run) -> Result<ProbeCost> {
        Ok(ProbeCost {
            id: run.manifest.id.to_string(),
            steps: run.last_step()?,
            keyframes: run.keyframes().map(|k| k.steps()).unwrap_or_default(),
            wall_ms: run.manifest.outcome.as_ref().map(|o| o.wall_ms),
            records: io_records(run),
        })
    }

    /// The most steps one probe may replay: from boot or a keyframe to
    /// the next keyframe, or to the run's end.
    fn longest_replay(&self) -> u64 {
        let mut marks: Vec<u64> = self
            .keyframes
            .iter()
            .copied()
            .filter(|&k| k <= self.steps)
            .collect();
        marks.sort_unstable();
        let mut longest = 0;
        let mut from = 0;
        for k in marks {
            longest = longest.max(k - from);
            from = k;
        }
        longest.max(self.steps - from)
    }

    /// How long replaying `steps` of this run takes, at the speed it was
    /// recorded, when that is known.
    fn seconds(&self, steps: u64) -> Option<f64> {
        let ms = self.wall_ms?;
        (self.steps > 0).then(|| steps as f64 * ms as f64 / self.steps as f64 / 1000.0)
    }
}

/// Whether a run recorded its reads and writes. No run does yet; once
/// records are a run input, as the seed is, this reads them from the
/// run's spec.
fn io_records(_run: &Run) -> IoRecords {
    IoRecords::Absent
}

/// The fewest and the most probes, both runs together, that blame made on
/// the cases it was measured on: neatvnc, setuptools, sagittarius and a
/// six-object archive.
const PROBES_MEASURED: (u64, u64) = (20, 43);

/// What a probe costs beyond its replay: the fork, starting from a
/// keyframe, and reading the file. Blame took 23 to 37 s for 20 to 38
/// probes on neatvnc, setuptools and the archive, whose keyframes are
/// close together.
const PROBE_OVERHEAD_SECS: f64 = 1.0;

/// How many times blame replays a run's longest stretch without
/// keyframes before the keyframes it keeps where it looks make later
/// probes short. On sagittarius, where one run had no keyframes and a
/// replay from boot took about 4.5 min, blame took 12 to 22 min.
const LONG_REPLAYS: (f64, f64) = (3.0, 5.0);

/// How to avoid bisecting, for the last line of the warning.
const RECORD_AGAIN: &str = "To skip this, record the runs again with --record-io, or with \
     check --property same-output; the new runs may not show the same difference.";

/// The warning `blame` prints before it forks runs that have no write
/// records, or None when every run has them.
fn bisection_warning(costs: &[ProbeCost]) -> Option<String> {
    let without: Vec<&ProbeCost> = costs
        .iter()
        .filter(|c| c.records == IoRecords::Absent)
        .collect();
    let worst = without.iter().max_by_key(|c| c.longest_replay())?;

    // Which runs blame forks.
    let names: Vec<&str> = without.iter().map(|c| short(&c.id)).collect();
    let what = match names.as_slice() {
        [one] => format!("rewind: run {one} has no write records, so blame forks it"),
        _ => format!(
            "rewind: runs {} have no write records, so blame forks them",
            names.join(" and ")
        ),
    };

    // The longest replay a probe may make, and the whole blame: each
    // probe's overhead, and a few long replays in each run.
    let steps = worst.longest_replay();
    let replays: Option<Vec<f64>> = without
        .iter()
        .map(|c| c.seconds(c.longest_replay()))
        .collect();
    let (fewest, most) = PROBES_MEASURED;
    let (few, many) = LONG_REPLAYS;
    let cost = match (worst.seconds(steps), replays) {
        (Some(secs), Some(replays)) => {
            let long: f64 = replays.iter().sum();
            format!(
                ", about {} at the speed run {} was recorded. A blame takes {fewest} to {most} \
                 probes, about {} in all",
                duration(secs),
                short(&worst.id),
                span(
                    fewest as f64 * PROBE_OVERHEAD_SECS + few * long,
                    most as f64 * PROBE_OVERHEAD_SECS + many * long
                )
            )
        }
        _ => format!(". A blame takes {fewest} to {most} probes"),
    };
    Some(format!(
        "{what} and bisects to find each writer.\nEach probe replays up to {} steps from the \
         nearest keyframe{cost}.\n{RECORD_AGAIN}",
        grouped(steps)
    ))
}

/// A range of durations, in the unit of its upper end: "2 to 3 min".
fn span(low: f64, high: f64) -> String {
    let (low, high) = if high < 90.0 {
        (low.round() as u64, format!("{} s", high.round() as u64))
    } else {
        (
            (low / 60.0).round() as u64,
            format!("{} min", (high / 60.0).round() as u64),
        )
    };
    format!("{low} to {high}")
}

/// `n` with its thousands grouped by commas.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A duration in seconds, or minutes once it passes a minute and a half.
fn duration(secs: f64) -> String {
    if secs < 90.0 {
        format!("{} s", secs.round() as u64)
    } else {
        format!("{} min", (secs / 60.0).round() as u64)
    }
}

/// The arguments of `rewind blame`.
#[derive(clap::Args)]
pub struct BlameArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) left: String,
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) right: String,
    /// The file, by its absolute path in the VM.
    pub(crate) path: String,
    /// The byte to explain; by default the first that differs.
    #[arg(long)]
    pub(crate) offset: Option<usize>,
}

/// One process that wrote a file, as the trace has it.
#[derive(Clone, Debug)]
struct Writer {
    /// The path the bytes were written under, before any rename.
    path: String,
    process: Process,
    /// The step of the open for writing, and of the close that ended it.
    open: u64,
    close: u64,
}

/// What made a file's bytes in one run: the copies back to the process
/// that wrote them.
struct Chain {
    hops: Vec<Writer>,
}

impl Chain {
    /// The `k`th writer, counting from the first.
    fn at(&self, k: usize) -> &Writer {
        &self.hops[self.hops.len() - 1 - k]
    }
}

/// Whether a writer's file was the same in both runs once it was done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Same,
    Differs,
}

pub fn blame(home: &Home, args: BlameArgs) -> Result<ExitCode> {
    let BlameArgs {
        left,
        right,
        path,
        offset,
    } = args;
    let runs = [Run::find(home, &left)?, Run::find(home, &right)?];

    // Without write records, even reading the file is done by forking the
    // runs, and finding each writer can take minutes; say so first.
    let costs = [ProbeCost::of(&runs[0])?, ProbeCost::of(&runs[1])?];
    if let Some(warning) = bisection_warning(&costs) {
        eprintln!("{warning}");
    }

    // The file as each run left it at the job's exit.
    let ends = [end_step(&runs[0])?, end_step(&runs[1])?];
    let finals = [
        read(home, &runs[0], ends[0], &path)?,
        read(home, &runs[1], ends[1], &path)?,
    ];
    let (Some(a), Some(b)) = (&finals[0], &finals[1]) else {
        bail!("{path} is missing at the end of one of the runs");
    };
    let Some(first) = offset.or_else(|| first_difference(a, b)) else {
        println!("{path} is identical in both runs ({} bytes)", a.len());
        return Ok(ExitCode::SUCCESS);
    };
    let differing =
        a.iter().zip(b.iter()).filter(|(x, y)| x != y).count() + a.len().abs_diff(b.len());
    println!(
        "{path}: {differing} of {} bytes differ, the first at offset {first} ({first:#x})",
        a.len().max(b.len())
    );
    for (run, bytes) in runs.iter().zip([a, b]) {
        println!("  {}  {}", short(&run.manifest.id), window(bytes, first));
    }
    println!();

    // Each run's chain of writers, back to the first.
    let mut chains = Vec::new();
    for (run, end) in runs.iter().zip(ends) {
        chains.push(chain(home, run, end, &path)?);
    }

    // The origin is the earliest writer whose file already differed
    // between the runs once it was done; the chains are lined up from
    // their first writers.
    let depth = chains[0].hops.len().min(chains[1].hops.len());
    let mut verdicts = vec![Verdict::Same; depth];
    for (k, verdict) in verdicts.iter_mut().enumerate() {
        let [x, y] = [
            &chains[0].hops[chains[0].hops.len() - 1 - k],
            &chains[1].hops[chains[1].hops.len() - 1 - k],
        ];
        let left = read(home, &runs[0], x.close, &x.path)?;
        let right = read(home, &runs[1], y.close, &y.path)?;
        if left != right {
            *verdict = Verdict::Differs;
        }
    }
    let origin_k = verdicts
        .iter()
        .position(|v| *v == Verdict::Differs)
        .unwrap_or(0);
    for (run, chain) in runs.iter().zip(&chains) {
        println!("run {}, newest writer first:", short(&run.manifest.id));
        let n = chain.hops.len();
        for (i, hop) in chain.hops.iter().enumerate() {
            let k = n - 1 - i;
            let how = match (verdicts.get(k), k == origin_k) {
                (Some(Verdict::Differs), true) => "differs first here",
                (Some(Verdict::Differs), false) => "differs",
                (Some(Verdict::Same), _) => "same in both runs",
                (None, _) => "",
            };
            let copy = if i + 1 < n && chain.hops[i + 1].path != hop.path {
                format!(", a copy of {}", chain.hops[i + 1].path)
            } else {
                String::new()
            };
            println!(
                "  {:>7}..{:<7} pid {:<5} {}{copy}  [{how}]",
                hop.open,
                hop.close,
                hop.process.pid,
                argv_line(&hop.process.argv),
            );
        }
    }
    println!();

    // When the byte took its final value, at the process that made it.
    let origins = [
        chains[0].at(origin_k).clone(),
        chains[1].at(origin_k).clone(),
    ];
    for (run, writer) in runs.iter().zip(&origins) {
        let Some(bytes) = read(home, run, writer.close, &writer.path)? else {
            continue;
        };
        let at = settle(home, run, writer, &bytes, first)?;
        println!(
            "run {}: offset {first} of {} took its final value between steps {} and {} (pid {}, {})",
            short(&run.manifest.id),
            writer.path,
            at.saturating_sub(1),
            at,
            writer.process.pid,
            writer.process.name()
        );
    }
    println!();

    // Whether the writers were asked for different things.
    let traces = [runs[0].trace()?, runs[1].trace()?];
    explain_argv(&origins, &traces, &runs);

    // Where the two runs first behave differently.
    match traces[0].divergence(traces[1]) {
        None => println!("the two runs are identical"),
        Some(d) => println!(
            "the runs' events first differ at step {} in {} and {} in {}",
            d.left_step,
            short(&runs[0].manifest.id),
            d.right_step,
            short(&runs[1].manifest.id)
        ),
    }
    Ok(ExitCode::FAILURE)
}

/// The step a run's job exited at, else its last step.
fn end_step(run: &Run) -> Result<u64> {
    match run.trace()?.job_exit() {
        Some(exit) => Ok(exit.step),
        None => run.last_step(),
    }
}

/// A file at a step, or None when it did not exist then.
fn read(home: &Home, run: &Run, step: u64, path: &str) -> Result<Option<Vec<u8>>> {
    match rewind_core::inspect::cat(home, run, step, None, path)? {
        Inspection::Contents(bytes) => Ok(Some(bytes)),
        Inspection::NotFound(_) => Ok(None),
        Inspection::Failed(message) => bail!("reading {path} at step {step}: {message}"),
    }
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    let shared = a.len().min(b.len());
    (0..shared)
        .find(|&i| a[i] != b[i])
        .or((a.len() != b.len()).then_some(shared))
}

/// The bytes from `offset`, as hex and as text.
fn window(bytes: &[u8], offset: usize) -> String {
    let end = (offset + WINDOW).min(bytes.len());
    let slice = &bytes[offset.min(end)..end];
    let hex: Vec<String> = slice.iter().map(|b| format!("{b:02x}")).collect();
    let text: String = slice
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("{}  |{text}|", hex.join(" "))
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn argv_line(argv: &[String]) -> String {
    let words: Vec<String> = argv
        .iter()
        .map(|a| match a.rsplit_once('/') {
            Some((dir, base)) if dir.starts_with("/nix/store/") && argv[0] == *a => base.into(),
            _ => a.clone(),
        })
        .collect();
    words.join(" ")
}

/// The writers of `path` in `run`, from the last one back: through
/// copies to the file copied, and through rewrites in place, as ranlib
/// and strip make, to the file's earlier writers.
fn chain(home: &Home, run: &Run, end: u64, path: &str) -> Result<Chain> {
    let trace = run.trace()?;
    let mut hops = Vec::new();
    let mut path = path.to_string();
    let mut before = end;
    for _ in 0..MAX_HOPS {
        let Some(writer) = last_writer(trace, &path, before) else {
            break;
        };

        // A copier's source is the argument naming a file the run wrote
        // earlier with the same bytes. Any other writer may have read the
        // file it rewrites, so its earlier writer comes next.
        let source = copied_from(home, run, trace, &writer)?;
        before = writer.open.saturating_sub(1);
        path = source.unwrap_or_else(|| writer.path.clone());
        hops.push(writer);
    }
    if hops.is_empty() {
        bail!(
            "no process of run {} opened {path} for writing",
            run.manifest.id
        );
    }
    Ok(Chain { hops })
}

/// The last process to write `path` before `before`, following a rename
/// into place back to the file that was renamed.
fn last_writer(trace: &Trace, path: &str, before: u64) -> Option<Writer> {
    let mut path = path.to_string();
    let processes = trace.processes();
    for (i, e) in trace.events.iter().enumerate().rev() {
        if e.step > before {
            continue;
        }
        match &e.kind {
            EventKind::Rename { from, to } if *to == path => {
                path = from.clone();
            }
            EventKind::Open { path: opened, .. } if *opened == path => {
                let process = processes.iter().find(|p| p.pid == e.pid)?.as_of(e.step);
                let close = close_after(&trace.events[i + 1..], e, &path, &process);
                return Some(Writer {
                    path,
                    process,
                    open: e.step,
                    close,
                });
            }
            _ => {}
        }
    }
    None
}

/// The step the write that `open` started ended: the close of the file's
/// last reference, else the process's exit.
fn close_after(rest: &[Event], open: &Event, path: &str, process: &Process) -> u64 {
    rest.iter()
        .find(|e| matches!(&e.kind, EventKind::Close { path: p } if p == path))
        .map(|e| e.step)
        .or(process.end)
        .unwrap_or(open.step)
}

/// The file a writer copied into `writer.path`. For a copier, such as
/// install or cp, an argument naming a file the run wrote before with the
/// copy's bytes. For any other program, an earlier file of the same name
/// with the same bytes, as meson install copies what meson setup
/// generated.
fn copied_from(home: &Home, run: &Run, trace: &Trace, writer: &Writer) -> Result<Option<String>> {
    let program = writer.process.name();
    let base = program.rsplit('/').next().unwrap_or(&program);
    let copy = read(home, run, writer.close, &writer.path)?;

    // A copier names its source.
    if COPIERS.contains(&base) {
        for arg in writer.process.argv.iter().skip(1) {
            if arg.starts_with('-') {
                continue;
            }
            let Some(candidate) = written_path(trace, arg, writer.open) else {
                continue;
            };
            if candidate == writer.path {
                continue;
            }
            if read(home, run, writer.open, &candidate)? == copy {
                return Ok(Some(candidate));
            }
        }
    }

    // Any other program: the newest earlier files of the same name with
    // the same bytes, as cmake's install step copies a library it built.
    // A program that rewrote the file in place, as ranlib and strip do,
    // matches none of them, and its earlier writer comes next instead.
    let name = writer.path.rsplit('/').next().unwrap_or(&writer.path);
    let mut tried = Vec::new();
    for e in trace.events.iter().rev().filter(|e| e.step < writer.open) {
        let EventKind::Open { path, .. } = &e.kind else {
            continue;
        };
        if *path == writer.path || tried.contains(path) {
            continue;
        }
        if path.rsplit('/').next() != Some(name) {
            continue;
        }
        if tried.len() == MAX_SAME_NAME {
            break;
        }
        tried.push(path.clone());

        // Read it as its own writer left it: a build often removes a
        // file once it is packed, as bdist_wheel does.
        let Some(earlier) = last_writer(trace, path, writer.open) else {
            continue;
        };
        if read(home, run, earlier.close, path)? == copy {
            return Ok(Some(path.clone()));
        }
    }
    Ok(None)
}

/// The absolute path of a file the run wrote before `before` that `arg`
/// names, relative or not.
fn written_path(trace: &Trace, arg: &str, before: u64) -> Option<String> {
    let arg = arg.trim_start_matches("./");
    trace
        .events
        .iter()
        .rev()
        .filter(|e| e.step < before)
        .find_map(|e| match &e.kind {
            EventKind::Open { path, .. } if path == arg || path.ends_with(&format!("/{arg}")) => {
                Some(path.clone())
            }
            _ => None,
        })
}

/// The first step at which the bytes at `offset` of the writer's file
/// equal their final value, by bisection over the steps the file was
/// open.
fn settle(home: &Home, run: &Run, writer: &Writer, last: &[u8], offset: usize) -> Result<u64> {
    let end = (offset + WINDOW).min(last.len());
    let want = &last[offset.min(end)..end];
    let settled = |step: u64| -> Result<bool> {
        let Some(bytes) = read(home, run, step, &writer.path)? else {
            return Ok(false);
        };
        Ok(bytes.get(offset..end) == Some(want))
    };

    // Invariant: unsettled at lo, settled at hi.
    let (mut lo, mut hi) = (writer.open, writer.close);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if settled(mid)? {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Ok(hi)
}

/// Compares the two writers' command lines, and traces arguments given in
/// another order to the steps the runs created them on.
fn explain_argv(origins: &[Writer; 2], traces: &[&Trace; 2], runs: &[Run; 2]) {
    let (a, b) = (&origins[0].process.argv, &origins[1].process.argv);
    if a == b {
        println!("both writers ran the same command line: {}", argv_line(a));
        println!("the difference is in what they read, or in their own scheduling");
        println!();
        return;
    }
    println!("the writers' command lines differ:");
    for (run, argv) in runs.iter().zip([a, b]) {
        println!("  {}  {}", short(&run.manifest.id), argv_line(argv));
    }

    // The same arguments in another order: when each run created each.
    let mut sorted_a = a.clone();
    let mut sorted_b = b.clone();
    sorted_a.sort();
    sorted_b.sort();
    if sorted_a != sorted_b {
        println!("  the arguments themselves differ, beyond their order");
        println!();
        return;
    }
    let Some(i) = (0..a.len()).find(|&i| a[i] != b[i]) else {
        return;
    };
    println!(
        "  the same {} arguments in another order, from argument {i} on",
        a.len()
    );
    for arg in [&a[i], &b[i]] {
        let steps: Vec<String> = traces
            .iter()
            .zip(runs)
            .map(|(t, r)| {
                let created = t.events.iter().find_map(|e| match &e.kind {
                    EventKind::Open { path, .. }
                        if path.ends_with(&format!("/{}", arg.trim_start_matches("./"))) =>
                    {
                        Some(e.step)
                    }
                    _ => None,
                });
                match created {
                    Some(step) => format!("step {step} in {}", short(&r.manifest.id)),
                    None => format!("not created in {}", short(&r.manifest.id)),
                }
            })
            .collect();
        println!("  {arg} was created at {}", steps.join(", "));
    }

    // Who built the command line: what the writer's process ran before,
    // and the programs it started meanwhile.
    for (trace, writer) in traces.iter().zip(origins) {
        let p = &writer.process;
        let earlier: Vec<&(u64, Vec<String>)> =
            p.execs.iter().filter(|(at, _)| *at < writer.open).collect();
        if earlier.len() < 2 {
            continue;
        }
        let (from, shell) = &earlier[earlier.len() - 2];
        let (to, _) = earlier[earlier.len() - 1];
        let children: Vec<String> = trace
            .processes()
            .into_iter()
            .filter(|c| c.parent == p.pid && c.start > *from && c.start < *to)
            .map(|c| argv_line(&c.argv))
            .collect();
        println!(
            "  pid {} ran {} first, and started: {}",
            p.pid,
            argv_line(shell),
            children.join("; ")
        );
        break;
    }
    println!();
}

#[cfg(test)]
mod tests {
    // The warning blame prints before it forks runs to find their writers:
    // built from run costs made by hand, with and without write records.
    use super::*;

    /// A neatvnc-sized run: 30,056 steps recorded in 15.1 s, with one
    /// keyframe where its job exited, as `rewind check` leaves one.
    fn neatvnc(id: &str, records: IoRecords) -> ProbeCost {
        ProbeCost {
            id: id.into(),
            steps: 30_056,
            keyframes: vec![30_052],
            wall_ms: Some(15_127),
            records,
        }
    }

    #[test]
    fn runs_without_records_warn_before_bisecting() {
        // Neither run has records: the warning names both runs, says a
        // probe may replay from boot to the exit keyframe, and how to
        // avoid it, in at most three lines.
        let runs = [
            neatvnc("a84b0112f11563fc", IoRecords::Absent),
            neatvnc("863054ef27a743e7", IoRecords::Absent),
        ];
        let warning = bisection_warning(&runs).expect("a warning");
        assert!(
            warning.contains("a84b0112 and 863054ef have no write records"),
            "{warning}"
        );
        assert!(warning.contains("30,052 steps"), "{warning}");
        // 20 to 43 probes of a second each, plus 3 to 5 replays of 15 s
        // from boot in each of the two runs.
        assert!(warning.contains("about 2 to 3 min"), "{warning}");
        assert!(warning.contains("--record-io"), "{warning}");
        assert!(warning.lines().count() <= 3, "{warning}");
    }

    #[test]
    fn runs_with_records_do_not_warn() {
        // Both runs have records, so blame does not bisect, and says
        // nothing.
        let runs = [
            neatvnc("a84b0112f11563fc", IoRecords::Present),
            neatvnc("863054ef27a743e7", IoRecords::Present),
        ];
        assert_eq!(bisection_warning(&runs), None);
    }

    #[test]
    fn only_runs_without_records_are_named() {
        // One run has records: the warning is about the other alone.
        let runs = [
            neatvnc("a84b0112f11563fc", IoRecords::Present),
            neatvnc("863054ef27a743e7", IoRecords::Absent),
        ];
        let warning = bisection_warning(&runs).expect("a warning");
        assert!(
            warning.contains("run 863054ef has no write records"),
            "{warning}"
        );
        assert!(!warning.contains("a84b0112"), "{warning}");
    }
}
