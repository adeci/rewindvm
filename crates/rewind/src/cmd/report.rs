//! `rewind report`: what a bug report about a run needs, gathered into a
//! directory, and a link that opens a GitHub issue filled in with it.
//!
//! The report names this build of rewind, the host's CPU, kernel and KVM,
//! what `rewind doctor` finds, the run's inputs, and what replaying the
//! run finds. Beside it goes the run's replayable .rwd, which holds the
//! run's programs, inputs and memory: a link cannot attach a file, and
//! rewind uploads nothing, so the person attaches it to the issue, or
//! leaves it out with --no-run.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use rewind_core::{Home, Run, export};

use crate::{RUN_HELP, RUN_LONG_HELP, color, show};

/// Where issues about rewind go.
pub(crate) const NEW_ISSUE: &str = "https://github.com/fzakaria/rewindvm/issues/new";

/// What a replay's verdict says when it found a difference.
const DIFFERENT: &str = "DIFFERENT";

/// The largest file GitHub takes as an issue's attachment, in MB.
const ATTACHMENT_MB: u64 = 25;

/// The report's own file in the directory.
const REPORT: &str = "report.md";

/// How much of the report the link carries in the issue's body. Browsers
/// and GitHub refuse links much past 8 KiB once encoded.
const BODY_IN_LINK: usize = 5000;

/// The lines of /proc/cpuinfo that say which CPU the host is.
const CPU_FIELDS: [&str; 6] = [
    "vendor_id",
    "model name",
    "cpu family",
    "model",
    "stepping",
    "microcode",
];

/// The KVM module parameters that change what a VM sees.
const KVM_PARAMETERS: [&str; 2] = [
    "/sys/module/kvm/parameters/enable_pmu",
    "/sys/module/kvm/parameters/ignore_msrs",
];

/// The arguments of `rewind report`.
#[derive(clap::Args)]
pub(crate) struct ReportArgs {
    #[arg(help = RUN_HELP, long_help = RUN_LONG_HELP)]
    pub(crate) run: String,
    /// The directory to write; rewind-report-<id> by default.
    #[arg(long, short)]
    pub(crate) output: Option<PathBuf>,
    /// Also find the instruction the replay went another way at, with
    /// `rewind replay --bisect`, and include what it says.
    #[arg(long)]
    pub(crate) bisect: bool,
    /// Leave out the run's .rwd, which holds its programs, inputs and
    /// memory: the report then says only what rewind found.
    #[arg(long)]
    pub(crate) no_run: bool,
    /// Open the issue's page in the browser, filled in with the report.
    #[arg(long)]
    pub(crate) open: bool,
}

pub(crate) fn report(home: &Home, args: ReportArgs) -> Result<ExitCode> {
    let ReportArgs {
        run,
        output,
        bisect,
        no_run,
        open,
    } = args;
    let run = Run::find(home, &run)?;
    let id = run.manifest.id.to_string();
    let dir = output.unwrap_or_else(|| PathBuf::from(format!("rewind-report-{id}")));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let paint = color::stderr();

    // What rewind finds, each from a command of its own, without color.
    eprintln!("{} replaying run {id}", paint.prefix());
    let mut replay = vec!["replay".to_string(), id.clone()];
    if bisect {
        replay.push("--bisect".to_string());
    }
    let replayed = rewind_output(&replay)?;
    let doctor = rewind_output(&["doctor".to_string()])?;

    let text = report_text(&run, &replay.join(" "), &replayed, &doctor);
    let report_path = dir.join(REPORT);
    std::fs::write(&report_path, &text)
        .with_context(|| format!("writing {}", report_path.display()))?;

    // The run itself, which another machine can replay.
    let rwd = dir.join(format!("{id}.rwd"));
    if !no_run {
        eprintln!("{} exporting run {id}", paint.prefix());
        export::export(home, &run, export::Contents::Replayable, &rwd)?;
    }

    eprintln!("{} wrote {}", paint.prefix(), report_path.display());
    if !no_run {
        let size = std::fs::metadata(&rwd)?.len();
        eprintln!(
            "{} wrote {} ({}), which holds the run's programs, inputs and memory: attach it \
             only if you can share those, or leave it out",
            paint.prefix(),
            rwd.display(),
            show::size(size)
        );
    }
    let link = issue_link(&run, &text);
    println!("open an issue with the report filled in: {link}");
    if !no_run {
        println!(
            "then attach {} to it: GitHub takes attachments of up to {} MB and not .rwd by \
             name, so zip it first, or share it from elsewhere and link it",
            rwd.display(),
            ATTACHMENT_MB
        );
    }
    if open {
        open_link(&link);
    }
    Ok(ExitCode::SUCCESS)
}

/// What this rewind prints for `args`, its standard output then its
/// standard error, without color, whatever it exits with.
fn rewind_output(args: &[String]) -> Result<String> {
    let exe = std::env::current_exe().context("finding rewind's own program")?;
    let out = Command::new(exe)
        .args(["--color", "never", "--progress", "off"])
        .args(args)
        .output()
        .with_context(|| format!("running rewind {}", args.join(" ")))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text)
}

/// The report, in Markdown, as it goes in report.md and the issue.
fn report_text(run: &Run, command: &str, replayed: &str, doctor: &str) -> String {
    let m = &run.manifest;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## What happened\n\n<!-- What you ran, and what you expected. -->\n"
    );
    let _ = writeln!(
        out,
        "## Replay\n\n```console\n$ rewind {command}\n{replayed}```\n"
    );
    let _ = writeln!(out, "## The run\n");
    let _ = writeln!(out, "- id: {}", m.id);
    let _ = writeln!(out, "- recorded by: rewind {}", m.recorded_by);
    let _ = writeln!(out, "- reported by: rewind {}", rewind_core::VERSION);
    let _ = writeln!(out, "- CPU model: {:?}", m.spec.cpu);
    let _ = writeln!(out, "- clock: {:?}", m.spec.clock);
    let _ = writeln!(out, "- kernel: {}", m.spec.kernel.display());
    if let Some(o) = &m.outcome {
        let _ = writeln!(out, "- ended: {:?} at step {}", o.stop, o.step);
    }
    let _ = writeln!(out, "\n## The host\n");
    for line in host_lines() {
        let _ = writeln!(out, "- {line}");
    }
    let _ = writeln!(out, "\n## rewind doctor\n\n```console\n{doctor}```");
    out
}

/// The host's CPU, kernel and KVM, a line each.
fn host_lines() -> Vec<String> {
    let mut lines = Vec::new();
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    for field in CPU_FIELDS {
        let value = cpuinfo.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == field).then(|| value.trim().to_string())
        });
        if let Some(value) = value {
            lines.push(format!("{field}: {value}"));
        }
    }
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        lines.push(format!("Linux {}", release.trim()));
    }
    for path in KVM_PARAMETERS {
        if let Ok(value) = std::fs::read_to_string(path) {
            let name = Path::new(path).file_name().unwrap_or_default();
            lines.push(format!("kvm {}: {}", name.to_string_lossy(), value.trim()));
        }
    }
    lines
}

/// The link to a new issue about `run` with `text` as its body, cut short
/// to fit in a link when it is long.
fn issue_link(run: &Run, text: &str) -> String {
    let title = if text.contains(DIFFERENT) {
        format!("Run {} does not replay exactly", run.manifest.id)
    } else {
        format!("A report on run {}", run.manifest.id)
    };
    let body = if text.len() > BODY_IN_LINK {
        let mut cut = BODY_IN_LINK;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!(
            "{}\n\n<!-- The rest is in {REPORT}; please attach it. -->",
            &text[..cut]
        )
    } else {
        text.to_string()
    };
    format!(
        "{NEW_ISSUE}?title={}&body={}",
        percent_encode(&title),
        percent_encode(&body)
    )
}

/// `text` with every byte but the unreserved ones percent-encoded, as a
/// link's query takes it.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Opens `link` in the desktop's browser, saying so when it cannot.
fn open_link(link: &str) {
    if Command::new("xdg-open").arg(link).status().is_err() {
        eprintln!(
            "{} could not start xdg-open; open the link above yourself",
            color::stderr().prefix()
        );
    }
}

#[cfg(test)]
mod tests {
    // The link a report opens, built without a run or a browser.
    use super::*;

    #[test]
    fn a_link_query_encodes_everything_but_the_unreserved_bytes() {
        // Spaces, newlines, the query's own & and = and non-ASCII text are
        // encoded; letters, digits and -_.~ are not.
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("x=1&y\n"), "x%3D1%26y%0A");
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
    }
}
