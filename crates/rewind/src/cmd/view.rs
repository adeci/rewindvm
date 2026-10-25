//! The `rewind` subcommands that read recorded runs: listing them, the
//! commands that make them, their output, processes and events, comparing
//! and replaying them, and opening them in the desktop app.

use std::process::ExitCode;

use anyhow::{Result, bail};
use rewind_core::{Guest, Home, Run, image};
use rewind_trace::Part;

use crate::color::{self, Same};
use crate::{RUN_HELP, RUN_LONG_HELP, json, list, reproduce, show};

/// The arguments of `rewind open`.
#[derive(clap::Args)]
pub(crate) struct OpenArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// The step the playhead starts at; by default the app's choice.
    pub(crate) step: Option<u64>,
    /// Show this run beside it, lined up with it, as the passing run
    /// beside a failing one.
    #[arg(long, value_name = "RUN")]
    pub(crate) compare: Option<String>,
}

pub(crate) fn open(home: &Home, args: OpenArgs) -> Result<ExitCode> {
    let OpenArgs { run, step, compare } = args;
    use std::os::unix::process::CommandExt;
    let run = Run::find(home, &run)?;
    let step = step.map(|s| run.check_step(s)).transpose()?;
    let compare = compare.map(|c| Run::find(home, &c)).transpose()?;
    let program = std::env::var_os(APP_ENV).unwrap_or_else(|| APP_PROGRAM.into());
    let args = app_args(&run.dir, step, compare.as_ref().map(|c| c.dir.as_path()));

    // exec only returns when the app did not start.
    let e = std::process::Command::new(&program).args(args).exec();
    if e.kind() == std::io::ErrorKind::NotFound {
        bail!(
            "the desktop app, {}, is not on PATH; install it from \
                 https://rewindvm.dev, or set {APP_ENV} to it",
            program.to_string_lossy()
        );
    }
    Err(anyhow::Error::new(e).context(format!("starting {}", program.to_string_lossy())))
}

/// The arguments of `rewind ls`.
#[derive(clap::Args)]
pub(crate) struct LsArgs {
    /// List only the newest N of the runs the other options pick.
    #[arg(short = 'n', long = "limit", value_name = "N")]
    pub(crate) limit: Option<usize>,
    /// Only runs whose name contains TEXT.
    #[arg(long, value_name = "TEXT")]
    pub(crate) name: Option<String>,
    /// Only runs that stand so.
    #[arg(long, value_enum)]
    pub(crate) status: Option<list::Status>,
    /// Only the runs forked from RUN, and forks of those.
    #[arg(long, value_name = "RUN")]
    pub(crate) forks_of: Option<String>,
    /// Only runs made since WHEN: an amount ago, such as 30m, 2h, 7d or
    /// 2w, or a day, such as 2026-10-01, from its start in UTC.
    #[arg(long, value_name = "WHEN")]
    pub(crate) since: Option<String>,
    /// Print one JSON object a line per run, for programs: its id,
    /// name, directory, when it was made, its parent, how it stands,
    /// its wait status, ending and steps.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn ls(home: &Home, args: LsArgs) -> Result<ExitCode> {
    let LsArgs {
        limit,
        name,
        status,
        forks_of,
        since,
        json,
    } = args;
    // Every run and every unreadable one, in one list, newest first.
    let listing = Run::list_all(home)?;
    let mut rows: Vec<(list::Entry, list::Row)> = listing
        .runs
        .iter()
        .map(|r| (list::Entry::of(r), list::Row::Run(r)))
        .chain(
            listing
                .unreadable
                .iter()
                .map(|u| (list::Entry::unreadable(u), list::Row::Unreadable(u))),
        )
        .collect();
    rows.sort_by_key(|(entry, _)| std::cmp::Reverse(entry.created));

    // The runs the options pick, the newest `limit` of them.
    let entries: Vec<list::Entry> = rows.iter().map(|(e, _)| e.clone()).collect();
    let among = match &forks_of {
        Some(run) => Some(list::forks_of(&entries, &Run::find(home, run)?.manifest.id)),
        None => None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filter = list::Filter {
        name,
        status,
        among,
        since: since.map(|s| list::since(&s, now)).transpose()?,
    };
    let picked = rows
        .iter()
        .filter(|(entry, _)| filter.keeps(entry))
        .take(limit.unwrap_or(usize::MAX));
    for (entry, row) in picked {
        if json {
            println!("{}", list::json(entry, row));
        } else {
            println!("{}", list::line(row, color::stdout()));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The arguments of `rewind show`.
#[derive(clap::Args)]
pub(crate) struct ShowArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// Print one JSON object on standard output, for programs: the
    /// run's id, whether this rewind boots the guest the run booted, and
    /// oldest first each command with the id of the run it makes, as
    /// arguments after `rewind` and as a line for a shell.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn show(home: &Home, args: ShowArgs) -> Result<ExitCode> {
    let ShowArgs { run, json } = args;
    let run = Run::find(home, &run)?;

    // The run and the runs it was forked from, oldest first, as far
    // back as this home has them. Imported manifests can name each
    // other as parents in a loop, which ends at the first repeat.
    let mut chain = vec![run.manifest.clone()];
    let mut gone = None;
    while let Some(parent) = chain.last().and_then(|m| m.parent.clone()).map(|p| p.run) {
        if chain.iter().any(|m| m.id == parent) {
            break;
        }
        match Run::find(home, &parent) {
            Ok(p) => chain.push(p.manifest),
            Err(_) => {
                gone = Some(parent);
                break;
            }
        }
    }
    chain.reverse();
    let commands = chain
        .iter()
        .map(|m| {
            let args = reproduce::command(m).map_err(|unset| {
                anyhow::anyhow!(
                    "no command line makes run {} again: it has {}",
                    m.id,
                    unset.0
                )
            })?;
            let line = std::iter::once("rewind".to_string())
                .chain(args.iter().map(|a| show::quote(a)))
                .collect::<Vec<_>>()
                .join(" ");
            Ok((m.id.clone(), args, line))
        })
        .collect::<Result<Vec<_>>>()?;

    // The guest is an input too, by its contents.
    let same_guest = Guest::from_env().ok().map(|guest| {
        let same = |recorded: &std::path::Path, now: &std::path::Path| {
            recorded == now || image::hash_file(recorded).ok() == image::hash_file(now).ok()
        };
        let spec = &run.manifest.spec;
        same(&spec.kernel, &guest.kernel) && same(&spec.initrd, &guest.initrd)
    });

    if json {
        let commands: Vec<serde_json::Value> = commands
            .iter()
            .map(|(id, args, line)| serde_json::json!({ "id": id, "args": args, "line": line }))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "id": run.manifest.id,
                "recorded_by": run.manifest.recorded_by,
                "same_guest": same_guest,
                "parent_gone": gone,
                "commands": commands,
            })
        );
        return Ok(ExitCode::SUCCESS);
    }
    println!("{}", show::summary(&run, color::stdout()));
    println!("recorded by rewind {}", run.manifest.recorded_by);
    let paint = color::stderr();
    if let Some(parent) = gone {
        eprintln!(
            "{} run {parent}, which the first of these forks, is not in this home",
            paint.prefix()
        );
    }
    if same_guest == Some(false) {
        eprintln!(
            "{} this rewind boots another kernel or initramfs than the run did, so \
                 these commands make other runs",
            paint.prefix()
        );
    }
    for (id, _, line) in &commands {
        println!("{line}  # {id}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The arguments of `rewind log`.
#[derive(clap::Args)]
pub(crate) struct LogArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// The step to print up to; by default the run's end.
    pub(crate) step: Option<u64>,
    /// Prefix each line with its step and pid.
    #[arg(long, short)]
    pub(crate) steps: bool,
}

pub(crate) fn log(home: &Home, args: LogArgs) -> Result<ExitCode> {
    let LogArgs { run, step, steps } = args;
    let run = Run::find(home, &run)?;
    let until = step.map(|s| run.check_step(s)).transpose()?;
    let trace = run.trace()?;
    for line in trace.lines_until(until.unwrap_or(u64::MAX)) {
        if steps {
            println!("{:>10} {:>5}  {}", line.step, line.pid, line.text);
        } else {
            println!("{}", line.text);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The arguments of `rewind ps`.
#[derive(clap::Args)]
pub(crate) struct PsArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// The step; by default the run's end.
    pub(crate) step: Option<u64>,
    /// Show the kernel's own threads too.
    #[arg(long)]
    pub(crate) all: bool,
    /// Print one JSON object on standard output, for programs: the step
    /// and each process alive then, with its parent, command line and
    /// threads.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn ps(home: &Home, args: PsArgs) -> Result<ExitCode> {
    let PsArgs {
        run,
        step,
        all,
        json,
    } = args;
    let run = Run::find(home, &run)?;
    let at = step.map(|s| run.check_step(s)).transpose()?;
    let trace = run.trace()?;
    let at = at.unwrap_or(trace.last_step());
    let threads = if all {
        show::KernelThreads::Show
    } else {
        show::KernelThreads::Hide
    };
    if json {
        println!("{}", json::processes(trace, at, threads));
        return Ok(ExitCode::SUCCESS);
    }
    print!("{}", show::process_tree(trace, at, threads));
    Ok(ExitCode::SUCCESS)
}

/// The arguments of `rewind events`.
#[derive(clap::Args)]
pub(crate) struct EventsArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// The first step whose events to print; by default 0.
    #[arg(long, value_name = "STEP")]
    pub(crate) from: Option<u64>,
    /// The last step whose events to print; by default the run's end.
    #[arg(long, value_name = "STEP")]
    pub(crate) to: Option<u64>,
    /// One JSON object per line.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn events(home: &Home, args: EventsArgs) -> Result<ExitCode> {
    let EventsArgs {
        run,
        from,
        to,
        json,
    } = args;
    let run = Run::find(home, &run)?;
    let from = from.map(|s| run.check_step(s)).transpose()?;
    let to = to.map(|s| run.check_step(s)).transpose()?;
    let trace = run.trace()?;
    let (from, to) = (from.unwrap_or(0), to.unwrap_or(u64::MAX));
    for e in trace
        .events
        .iter()
        .filter(|e| (from..=to).contains(&e.step))
    {
        if json {
            println!("{}", serde_json::to_string(e)?);
        } else {
            println!("{}", show::event(e));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The arguments of `rewind replay`.
#[derive(clap::Args)]
pub(crate) struct ReplayArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// Start from the keyframe at or before this step instead of boot.
    #[arg(long, value_name = "STEP")]
    pub(crate) from: Option<u64>,
    /// Print one JSON object on standard output, for programs: whether
    /// the trace came out identical, the keyframe it started from, where
    /// it first differed, and what comparing the machine at the run's
    /// keyframes found.
    #[arg(long)]
    pub(crate) json: bool,
    /// Where to compare the whole machine with the recording: at every
    /// keyframe after the start, only at the last, or nowhere, comparing
    /// only events and records.
    ///
    /// Each comparison hashes the pages the guest wrote since the last
    /// one, as recording does when it takes the keyframe, without storing
    /// them. `last` takes no capture before the last keyframe, so the
    /// replay runs as one without keyframes would.
    #[arg(long, value_enum, default_value_t = MachineArg::Every, value_name = "WHERE")]
    pub(crate) machine: MachineArg,
}

/// Where `rewind replay` compares the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MachineArg {
    Every,
    Last,
    None,
}

impl From<MachineArg> for rewind_core::run::CompareMachine {
    fn from(arg: MachineArg) -> Self {
        match arg {
            MachineArg::Every => rewind_core::run::CompareMachine::Every,
            MachineArg::Last => rewind_core::run::CompareMachine::Last,
            MachineArg::None => rewind_core::run::CompareMachine::Never,
        }
    }
}

/// The words `rewind replay` starts its verdict with.
const IDENTICAL: &str = "identical";
const DIFFERENT: &str = "DIFFERENT";

/// How many places of differing pages `rewind replay` names by symbol,
/// each of which asks gdb.
const NAMED_PLACES: usize = 4;

pub(crate) fn replay(home: &Home, args: ReplayArgs) -> Result<ExitCode> {
    let ReplayArgs {
        run,
        from,
        json,
        machine,
    } = args;
    let run = Run::find(home, &run)?;
    let from = from.map(|s| run.check_step(s)).transpose()?;
    let started = std::time::Instant::now();
    let replayed = run.replay_machine(home, from, machine.into())?;
    let machine = &replayed.machine;
    if let Some(first) = &machine.first {
        run.note_machine_difference(first)?;
    }
    let identical = replayed.divergence.is_none() && machine.first.is_none();
    let exit = if identical {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };

    if json {
        let (original, again) = (&replayed.original, &replayed.again);
        let divergence = match &replayed.divergence {
            None => serde_json::Value::Null,
            Some(d) if d.part == Part::IoRecords => json::io_divergence(d),
            Some(_) => json::divergence(original, again),
        };
        println!(
            "{}",
            serde_json::json!({
                "identical": identical,
                "keyframe": replayed.keyframe,
                "divergence": divergence,
                "machine": machine,
            })
        );
        return Ok(exit);
    }

    // The events' verdict, as replay has always given it.
    let paint = color::stdout();
    let identical_word = paint.verdict(Same::Yes, IDENTICAL);
    let different_word = paint.verdict(Same::No, DIFFERENT);
    let at_keyframes = |n: usize| match n {
        0 => String::new(),
        1 => ", and the whole machine at its keyframe".to_string(),
        n => format!(", and the whole machine at its {n} keyframes"),
    };
    let compared = match &machine.first {
        Some(_) => String::new(),
        None => at_keyframes(machine.compared.len()),
    };
    match (&replayed.divergence, replayed.keyframe) {
        (None, Some(kf)) => println!(
            "{identical_word} to the recording from the snapshot at step {kf} to the end{compared} ({:.2}s)",
            started.elapsed().as_secs_f64()
        ),
        (None, None) => println!(
            "{identical_word} to the recording: {} events and {} read and write records over {} steps{compared}",
            run.trace()?.events.len(),
            run.io()?.records.len(),
            run.manifest.outcome.as_ref().map_or(0, |o| o.step)
        ),
        (Some(d), Some(kf)) if d.part == Part::IoRecords => {
            println!("{different_word} from the recording after the snapshot at step {kf}:");
            println!("{}", show::io_divergence(d));
        }
        (Some(_), Some(kf)) => {
            println!("{different_word} from the recording after the snapshot at step {kf}:");
            print!(
                "{}",
                show::divergence(&replayed.original, &replayed.again, paint)
            );
        }
        (Some(d), None) if d.part == Part::IoRecords => println!(
            "{different_word} from the recording: {}: the run was not deterministic",
            show::io_divergence(d)
        ),
        (Some(d), None) => println!(
            "{different_word} from the recording at event {} (step {} recorded, step {} replayed): the run was not deterministic",
            d.index, d.left_step, d.right_step
        ),
    }

    // The machine's, when it differed at a keyframe.
    if let Some(first) = &machine.first {
        print!("{}", machine_words(home, &run, first, &replayed, paint));
    }
    if !identical {
        println!(
            "\nto find the instruction that made the difference: rewind replay {} --bisect",
            run.manifest.id
        );
    }
    Ok(exit)
}

/// What a replay found of the machine at a keyframe, for a person: where
/// it first differed, what differs there, and what that means for a look
/// inside the run.
fn machine_words(
    home: &Home,
    run: &Run,
    first: &rewind_core::state::MachineDifference,
    replayed: &rewind_core::run::Replayed,
    paint: color::Paint,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let different_word = paint.verdict(Same::No, DIFFERENT);
    let matched = match first.matched.or(replayed.keyframe) {
        Some(step) => format!(" It matched at step {step}."),
        None => String::new(),
    };
    let _ = writeln!(
        out,
        "the machine is {different_word} from the recording at the keyframe at step {}.{matched}",
        first.step
    );

    // The pages, each where the replayed machine has it, the first few
    // named by their symbol.
    let mut named = 0;
    if !first.pages.is_empty() {
        let _ = writeln!(
            out,
            "\n{} {}{}:",
            first.pages_differing,
            if first.pages_differing == 1 {
                "page of memory differs"
            } else {
                "pages of memory differ"
            },
            if first.pages_differing > first.pages.len() {
                format!(", the first {} shown", first.pages.len())
            } else {
                String::new()
            }
        );
    }
    for page in &first.pages {
        let physical = page.page as u64 * 4096;
        for place in &page.places {
            let words = match place {
                rewind_core::state::Place::Process { pid, name, address } => {
                    // The address of the first differing byte, by symbol.
                    let offset = page.bytes.first().map_or(0, |b| b.offset as u64);
                    let at = address + offset;
                    let symbol = if named < NAMED_PLACES {
                        named += 1;
                        crate::locate::address_words(home, run, first.step, *pid, at)
                    } else {
                        None
                    };
                    match symbol {
                        Some(symbol) => format!("process {pid} ({name}) at {at:#x}: {symbol}"),
                        None => format!("process {pid} ({name}) at {at:#x}"),
                    }
                }
                rewind_core::state::Place::KernelImage { address, symbol } => match symbol {
                    Some(symbol) => format!("the kernel at {address:#x}: {symbol}"),
                    None => format!("the kernel at {address:#x}"),
                },
                rewind_core::state::Place::Kernel => {
                    format!("kernel memory at physical {physical:#x}")
                }
            };
            let _ = writeln!(out, "  {words}");
        }
        for bytes in &page.bytes {
            let hex = |b: &[u8]| {
                b.iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let recorded = bytes
                .recorded
                .as_deref()
                .map_or_else(|| "not in the page store".to_string(), &hex);
            let _ = writeln!(
                out,
                "    {:#06x}  {}  {}\n            {}  {}",
                bytes.offset,
                paint.padded(color::Style::Left, "recorded", 8),
                recorded,
                paint.padded(color::Style::Right, "replayed", 8),
                hex(&bytes.replayed)
            );
        }
    }

    // Registers and the rest of the machine's state.
    if !first.state.is_empty() {
        let _ = writeln!(out, "\nstate that differs:");
    }
    for d in &first.state {
        let _ = writeln!(
            out,
            "  {}: {} {}, {} {}",
            d.name,
            paint.paint(color::Style::Left, "recorded"),
            d.left,
            paint.paint(color::Style::Right, "replayed"),
            d.right
        );
    }

    // What it means for a look inside the run.
    let since = first.matched.or(replayed.keyframe);
    let stretch = match since {
        Some(matched) => format!(
            "gdb, shell and cat at steps {} to {} start from the keyframe at {matched}",
            matched + 1,
            first.step - 1
        ),
        None => format!(
            "gdb, shell and cat at steps before {} start from boot or a keyframe before it",
            first.step
        ),
    };
    if replayed.divergence.is_none() {
        let _ = writeln!(out, "\nnothing the run printed or wrote differs.");
    }
    let _ = writeln!(
        out,
        "{stretch}, so they can show values the recording never had there."
    );
    out
}

/// The arguments of `rewind diff`.
#[derive(clap::Args)]
pub(crate) struct DiffArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) left: String,
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) right: String,
    /// Print one JSON object on standard output, for programs: the two
    /// runs' ids and where they first differ, null when identical.
    #[arg(long)]
    pub(crate) json: bool,
    /// Compare one file as each run had it when its job exited: how it
    /// differs, then the first lines or bytes that differ.
    ///
    /// Rewind brings each run back to that step to read the file, which
    /// takes a few seconds. Only the first 1,024 KiB of each copy are
    /// compared. Exits 0 when the file is the same in both runs and 1
    /// when it differs.
    #[arg(long, value_name = "PATH", conflicts_with = "outputs")]
    pub(crate) file: Option<String>,
    /// List the files either run's job wrote and left behind whose bytes
    /// differ, as each run had them when its job exited.
    ///
    /// For a Nix build these are its outputs and the files it left in its
    /// build directory. Rewind brings each run back to that step once to
    /// read them all. Exits 0 when none differ and 1 when some do.
    #[arg(long)]
    pub(crate) outputs: bool,
}

impl DiffArgs {
    /// Whether the command reads files inside the runs' VMs, which holds
    /// the home in use.
    pub(crate) fn reads_files(&self) -> bool {
        self.file.is_some() || self.outputs
    }
}

pub(crate) fn diff(home: &Home, args: DiffArgs) -> Result<ExitCode> {
    let DiffArgs {
        left,
        right,
        json,
        file,
        outputs,
    } = args;
    let left = Run::find(home, &left)?;
    let right = Run::find(home, &right)?;

    // The files the runs left behind, rather than their events.
    let format = if json { Format::Json } else { Format::Text };
    if let Some(path) = file {
        return crate::diff_files::diff_file(home, &left, &right, &path, format);
    }
    if outputs {
        return crate::diff_files::diff_outputs(home, &left, &right, format);
    }
    let (lt, rt) = (left.trace()?, right.trace()?);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "left": left.manifest.id,
                "right": right.manifest.id,
                "divergence": json::divergence(lt, rt),
            })
        );
    } else {
        print!("{}", show::divergence(lt, rt, color::stdout()));
    }
    match lt.divergence(rt) {
        None => Ok(ExitCode::SUCCESS),
        Some(_) => Ok(ExitCode::FAILURE),
    }
}

/// How `rewind diff` prints what it found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// Lines for a person.
    Text,
    /// One JSON object, for programs.
    Json,
}

/// rewind-app's arguments for a run's directory, the step to start at and
/// the directory of the run to show beside it.
fn app_args(
    dir: &std::path::Path,
    step: Option<u64>,
    compare: Option<&std::path::Path>,
) -> Vec<std::ffi::OsString> {
    let mut args = vec![dir.as_os_str().to_owned()];
    if let Some(step) = step {
        args.extend(["--step".into(), step.to_string().into()]);
    }
    if let Some(compare) = compare {
        args.extend(["--compare".into(), compare.as_os_str().to_owned()]);
    }
    args
}

/// The desktop app, looked up on PATH unless REWIND_APP names it.
const APP_PROGRAM: &str = "rewind-app";

const APP_ENV: &str = "REWIND_APP";

#[cfg(test)]
mod tests {
    // The arguments `rewind open` starts the app with.
    use super::*;
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn the_app_is_given_the_run_its_step_and_the_run_beside_it() {
        // rewind-app takes a run's directory, then --step and --compare
        // when asked for; a run alone opens alone.
        let args = |step, compare: Option<&str>| -> Vec<String> {
            app_args(
                std::path::Path::new("/runs/a"),
                step,
                compare.map(std::path::Path::new),
            )
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
        };
        assert_eq!(args(None, None), vec!["/runs/a"]);
        assert_eq!(
            args(Some(5), Some("/runs/b")),
            vec!["/runs/a", "--step", "5", "--compare", "/runs/b"]
        );
        assert!(Cli::try_parse_from("rewind open abc 5 --compare def".split_whitespace()).is_ok());
    }
}
