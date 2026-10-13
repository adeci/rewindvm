//! The properties `rewind check` judges each run it tries by.
//!
//! A property says whether a finished run is correct and, when it is not,
//! why and at which step. The search looks for a schedule whose verdict
//! differs from the unperturbed run's, then narrows it to the step that
//! decides it. With no property given, check asks for the same outcome as
//! the unperturbed run, which is what it asked before properties existed.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use rewind_core::{Home, Run};
use rewind_trace::ending::Ending;
use rewind_trace::properties::{self as trace_properties, Finding, Glob};
use rewind_trace::sdk::{SdkMark, TestResult};

use crate::show::{self, OutcomeKey};

/// The word a property is named by on the command line.
const SAME_OUTCOME: &str = "same-outcome";
const EXIT_ZERO: &str = "exit-zero";
const NO_CRASH: &str = "no-crash";
const NO_ORPHANS: &str = "no-orphans";
const SINGLE_WRITER: &str = "single-writer";
const ATOMIC_REPLACE: &str = "atomic-replace";
const WRITES_WITHIN: &str = "writes-within";
const SDK: &str = "sdk";
const SCRIPT: &str = "script";

/// Every property as the command line writes it, for the message that
/// refuses an unknown one.
const ALL: &str = "same-outcome, exit-zero, no-crash, no-orphans, single-writer=GLOB, \
     atomic-replace=GLOB, writes-within=DIR:DIR, sdk and script=PATH";

/// What separates the directories of writes-within.
const DIR_SEPARATOR: char = ':';

/// What starts a comment line in a properties file.
const COMMENT: char = '#';

/// What a property script's exit status means.
const SCRIPT_HOLDS: i32 = 0;
const SCRIPT_VIOLATED: i32 = 1;

/// The environment a property script reads its run from.
const ENV_RUN: &str = "REWIND_RUN";
const ENV_BASE: &str = "REWIND_BASE";
const ENV_END: &str = "REWIND_END";

/// A property of a finished run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Property {
    /// The run ends as the unperturbed run did: the same wait status and
    /// every output with the same hash.
    SameOutcome,
    /// The job exits 0 having made every output, before the time limit.
    ExitZero,
    /// No process of the job crashed (see [`trace_properties::no_crash`]).
    NoCrash,
    /// Every process the job started had exited when the job did.
    NoOrphans,
    /// No two of the job's processes wrote a file matching the glob at
    /// once.
    SingleWriter(Glob),
    /// No file matching the glob that the run had written was truncated
    /// in place.
    AtomicReplace(Glob),
    /// The job wrote files only under these directories.
    WritesWithin(Vec<String>),
    /// What the job asserted with the Rewind SDK held: no `always` failed,
    /// no `unreachable` was reached, and every test that passed in the
    /// unperturbed run passed (see [`rewind_trace::sdk`]).
    Sdk,
    /// A program on this machine judges the run: exit 0 means the
    /// property holds, 1 that it is violated, and any other ending is an
    /// error that stops the search.
    Script(PathBuf),
}

impl FromStr for Property {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Property> {
        let (name, arg) = match s.split_once('=') {
            Some((name, arg)) => (name, Some(arg)),
            None => (s, None),
        };
        let arg = arg.filter(|a| !a.is_empty());
        match (name, arg) {
            (SAME_OUTCOME, None) => Ok(Property::SameOutcome),
            (EXIT_ZERO, None) => Ok(Property::ExitZero),
            (NO_CRASH, None) => Ok(Property::NoCrash),
            (NO_ORPHANS, None) => Ok(Property::NoOrphans),
            (SDK, None) => Ok(Property::Sdk),
            (SINGLE_WRITER, Some(glob)) => Ok(Property::SingleWriter(Glob::new(glob))),
            (ATOMIC_REPLACE, Some(glob)) => Ok(Property::AtomicReplace(Glob::new(glob))),
            (WRITES_WITHIN, Some(dirs)) => {
                let dirs: Vec<String> = dirs.split(DIR_SEPARATOR).map(str::to_string).collect();
                if let Some(relative) = dirs.iter().find(|d| !d.starts_with('/')) {
                    bail!("property {WRITES_WITHIN} takes absolute directories, not {relative}");
                }
                Ok(Property::WritesWithin(dirs))
            }
            (SCRIPT, Some(path)) => Ok(Property::Script(PathBuf::from(path))),
            (SAME_OUTCOME | EXIT_ZERO | NO_CRASH | NO_ORPHANS | SDK, Some(_)) => {
                bail!("property {name} takes no argument")
            }
            (SINGLE_WRITER | ATOMIC_REPLACE, None) => {
                bail!("property {name} needs the files it is about: {name}=GLOB")
            }
            (WRITES_WITHIN, None) => bail!("property {name} needs directories: {name}=DIR:DIR"),
            (SCRIPT, None) => bail!("property script needs a program: script=PATH"),
            _ => bail!("no property {name}; the properties are {ALL}"),
        }
    }
}

impl fmt::Display for Property {
    /// The property as the command line names it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Property::SameOutcome => write!(f, "{SAME_OUTCOME}"),
            Property::ExitZero => write!(f, "{EXIT_ZERO}"),
            Property::NoCrash => write!(f, "{NO_CRASH}"),
            Property::NoOrphans => write!(f, "{NO_ORPHANS}"),
            Property::Sdk => write!(f, "{SDK}"),
            Property::SingleWriter(glob) => write!(f, "{SINGLE_WRITER}={}", glob.as_str()),
            Property::AtomicReplace(glob) => write!(f, "{ATOMIC_REPLACE}={}", glob.as_str()),
            Property::WritesWithin(dirs) => {
                let dirs = dirs.join(&DIR_SEPARATOR.to_string());
                write!(f, "{WRITES_WITHIN}={dirs}")
            }
            Property::Script(path) => write!(f, "{SCRIPT}={}", path.display()),
        }
    }
}

impl Property {
    /// The short name a report calls the property by: a script by its
    /// file name without its extension, and a property that takes an
    /// argument without it.
    pub fn name(&self) -> String {
        match self {
            Property::Script(path) => path.file_stem().map_or_else(
                || path.display().to_string(),
                |s| s.to_string_lossy().into_owned(),
            ),
            other => {
                let spec = other.to_string();
                spec.split_once('=')
                    .map_or(spec.clone(), |(name, _)| name.to_string())
            }
        }
    }

    /// The properties a file lists, one a line, without blank lines and
    /// lines starting with `#`. A script's path is taken from the file's
    /// directory, so a repository's file can name the scripts beside it.
    pub fn read_file(path: &Path) -> Result<Vec<Property>> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading properties from {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut properties = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(COMMENT) {
                continue;
            }
            let property: Property = line
                .parse()
                .with_context(|| format!("{}:{}", path.display(), i + 1))?;
            properties.push(match property {
                Property::Script(script) if script.is_relative() => {
                    Property::Script(dir.join(script))
                }
                other => other,
            });
        }
        Ok(properties)
    }
}

/// Why a run violates a property: the property, the step to look at when
/// there is one, and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub property: String,
    pub step: Option<u64>,
    pub why: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.step {
            Some(step) => write!(f, "step {step}: {}", self.why),
            None => write!(f, "{}", self.why),
        }
    }
}

/// The properties of one search, and the unperturbed run they compare
/// with.
pub struct Judge<'a> {
    home: &'a Home,
    base_id: String,
    base_key: OutcomeKey,
    base_ending: Ending,
    /// How each test the unperturbed run marked with the SDK ended.
    base_tests: BTreeMap<String, TestResult>,
    properties: Vec<Property>,
}

impl<'a> Judge<'a> {
    /// A judge of `properties`, or of the same outcome as `base` when
    /// there are none.
    pub fn new(home: &'a Home, base: &Run, properties: Vec<Property>) -> Result<Judge<'a>> {
        let properties = if properties.is_empty() {
            vec![Property::SameOutcome]
        } else {
            properties
        };
        let base_key = show::outcome_key(base)?;
        let base_tests = base
            .trace()?
            .sdk_marks()
            .filter_map(|(_, mark)| match mark {
                SdkMark::TestEnd { name, result } => Some((name, result)),
                _ => None,
            })
            .collect();
        Ok(Judge {
            home,
            base_id: base.manifest.id.to_string(),
            base_ending: ending(base, &base_key),
            base_key,
            base_tests,
            properties,
        })
    }

    /// Whether the search asks only for the same outcome as the
    /// unperturbed run, as check did before properties.
    pub fn same_outcome_only(&self) -> bool {
        self.properties == [Property::SameOutcome]
    }

    /// The properties, as the command line names them.
    pub fn properties(&self) -> &[Property] {
        &self.properties
    }

    /// The unperturbed run's outcome, which `same-outcome` compares with.
    pub fn base_key(&self) -> &OutcomeKey {
        &self.base_key
    }

    /// Every property `run` violates, in the order given; none when all
    /// hold.
    pub fn violations(&self, run: &Run) -> Result<Vec<Violation>> {
        let mut found = Vec::new();
        for property in &self.properties {
            if let Some(v) = self.judge(property, run)? {
                found.push(v);
            }
        }
        Ok(found)
    }

    /// Whether `run` violates `property`, and how.
    fn judge(&self, property: &Property, run: &Run) -> Result<Option<Violation>> {
        let violation = |step: Option<u64>, why: String| Violation {
            property: property.name(),
            step,
            why,
        };
        let found = |finding: Option<Finding>| finding.map(|f| violation(Some(f.step), f.why));
        match property {
            // The outcome against the unperturbed run's.
            Property::SameOutcome => {
                let key = show::outcome_key(run)?;
                if key == self.base_key {
                    return Ok(None);
                }
                Ok(Some(violation(
                    end_step(run).ok(),
                    format!(
                        "ended {}, where {} ended {}",
                        ending(run, &key),
                        self.base_id,
                        self.base_ending
                    ),
                )))
            }

            // The job's own ending.
            Property::ExitZero => {
                let key = show::outcome_key(run)?;
                let ended = ending(run, &key);
                if ended.passed() {
                    return Ok(None);
                }
                Ok(Some(violation(
                    end_step(run).ok(),
                    format!("ended {ended}"),
                )))
            }

            // What the trace says the job's processes did.
            Property::NoCrash => Ok(found(trace_properties::no_crash(run.trace()?))),
            Property::NoOrphans => Ok(found(trace_properties::no_orphans(run.trace()?))),
            Property::SingleWriter(glob) => {
                Ok(found(trace_properties::single_writer(run.trace()?, glob)))
            }
            Property::AtomicReplace(glob) => {
                Ok(found(trace_properties::atomic_replace(run.trace()?, glob)))
            }
            Property::WritesWithin(dirs) => {
                Ok(found(trace_properties::writes_within(run.trace()?, dirs)))
            }

            // What the job asserted, and its tests against the unperturbed
            // run's.
            Property::Sdk => Ok(self
                .sdk_violation(run)?
                .map(|(step, why)| violation(step, why))),

            // A program's verdict on the run.
            Property::Script(path) => {
                let end = end_step(run)?;
                let output = script_command(path, self.home, run, &self.base_id, end)?
                    .output()
                    .with_context(|| format!("running property script {}", path.display()))?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                match output.status.code() {
                    Some(SCRIPT_HOLDS) => Ok(None),
                    Some(SCRIPT_VIOLATED) => {
                        let (step, why) = script_witness(&stdout);
                        Ok(Some(violation(step, why)))
                    }
                    _ => bail!(
                        "property script {} failed on run {} ({}): {}",
                        path.display(),
                        run.manifest.id,
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                }
            }
        }
    }
}

impl Judge<'_> {
    /// The first thing `run` broke that it asserted with the SDK, by step:
    /// an `always` that failed, an `unreachable` it reached, or a test
    /// that passed in the unperturbed run and failed or never finished in
    /// this one.
    fn sdk_violation(&self, run: &Run) -> Result<Option<(Option<u64>, String)>> {
        let mut broken: Vec<(Option<u64>, String)> = Vec::new();
        let mut begun: HashMap<String, u64> = HashMap::new();
        let mut ended: HashMap<String, (u64, TestResult)> = HashMap::new();
        for (event, mark) in run.trace()?.sdk_marks() {
            match mark {
                SdkMark::Always {
                    id,
                    held: false,
                    message,
                } => broken.push((
                    Some(event.step),
                    said(&format!("always \"{id}\" failed"), &message),
                )),
                SdkMark::Unreachable { id, message } => {
                    broken.push((
                        Some(event.step),
                        said(&format!("unreachable \"{id}\" reached"), &message),
                    ));
                }
                SdkMark::TestBegin { name } => {
                    begun.entry(name).or_insert(event.step);
                }
                SdkMark::TestEnd { name, result } => {
                    ended.insert(name, (event.step, result));
                }
                SdkMark::Always { .. } | SdkMark::Sometimes { .. } => {}
            }
        }

        // The tests the unperturbed run passed, as this run ended them.
        for (name, base_result) in &self.base_tests {
            if *base_result != TestResult::Pass {
                continue;
            }
            match ended.get(name) {
                Some((_, TestResult::Pass | TestResult::Skip)) => {}
                Some((step, TestResult::Fail)) => broken.push((
                    Some(*step),
                    format!("test {name} passed in the unperturbed run and failed here"),
                )),
                None => broken.push((
                    begun.get(name).copied(),
                    format!("test {name} passed in the unperturbed run and did not finish here"),
                )),
            }
        }
        Ok(broken
            .into_iter()
            .min_by_key(|(step, _)| step.unwrap_or(u64::MAX)))
    }
}

/// `what`, followed by what the program said about it when it said
/// anything.
fn said(what: &str, message: &str) -> String {
    if message.is_empty() {
        what.to_string()
    } else {
        format!("{what}: {message}")
    }
}

/// The `sometimes` conditions of a search: each id any run checked, and
/// whether any run made it true.
#[derive(Default)]
pub struct Coverage {
    sometimes: BTreeMap<String, bool>,
}

impl Coverage {
    /// Adds the `sometimes` checks `run` made.
    pub fn add(&mut self, run: &Run) -> Result<()> {
        for (_, mark) in run.trace()?.sdk_marks() {
            if let SdkMark::Sometimes { id, hit } = mark {
                *self.sometimes.entry(id).or_default() |= hit;
            }
        }
        Ok(())
    }

    /// The ids no run made true, in order.
    pub fn never(&self) -> Vec<&str> {
        self.sometimes
            .iter()
            .filter(|(_, hit)| !**hit)
            .map(|(id, _)| id.as_str())
            .collect()
    }
}

/// Whether `run` wrote any SDK mark, so the search judges them.
pub fn writes_sdk_marks(run: &Run) -> Result<bool> {
    Ok(run.trace()?.sdk_marks().next().is_some())
}

/// How `run` ended, as a run's line says it.
fn ending(run: &Run, key: &OutcomeKey) -> Ending {
    let missing = show::missing_outputs(&run.manifest.spec.job.outputs, &key.outputs);
    match &run.manifest.outcome {
        Some(o) => Ending::of(&o.stop, key.status, &missing),
        None => Ending::NoStatus,
    }
}

/// The step a property script reads a run's files at: the job's exit,
/// where the job's files are as it left them, else the run's last.
fn end_step(run: &Run) -> Result<u64> {
    match run.trace()?.job_exit() {
        Some(exit) => Ok(exit.step),
        None => run.last_step(),
    }
}

/// The command that runs property script `path` on `run`: the run, the
/// unperturbed run and the step to read files at in its environment, the
/// same Rewind home, and this rewind first on PATH, so the `rewind` it
/// calls reads the same runs.
fn script_command(path: &Path, home: &Home, run: &Run, base_id: &str, end: u64) -> Result<Command> {
    let mut command = Command::new(path);
    command
        .env(ENV_RUN, run.manifest.id.to_string())
        .env(ENV_BASE, base_id)
        .env(ENV_END, end.to_string())
        .env(rewind_core::home::ENV_HOME, home.root())
        .stdin(Stdio::null());

    // This rewind's directory first on PATH.
    let exe = std::env::current_exe().context("finding this rewind")?;
    if let Some(dir) = exe.parent() {
        let rest = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&rest)),
        )?;
        command.env("PATH", path);
    }
    Ok(command)
}

/// What a script that found its property violated said: its first line,
/// and the step that line names when it reads `step N: why`.
fn script_witness(stdout: &str) -> (Option<u64>, String) {
    const STEP_PREFIX: &str = "step ";
    const NO_REASON: &str = "violated, without saying why";

    let Some(line) = stdout.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return (None, NO_REASON.to_string());
    };
    let named = line
        .strip_prefix(STEP_PREFIX)
        .and_then(|rest| rest.split_once(':'))
        .and_then(|(n, why)| Some((n.trim().parse::<u64>().ok()?, why.trim())));
    match named {
        Some((step, why)) if !why.is_empty() => (Some(step), why.to_string()),
        Some((step, _)) => (Some(step), NO_REASON.to_string()),
        None => (None, line.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each property parses from the form the command line takes, and
    // prints back to it.
    #[test]
    fn properties_parse_from_the_command_line() {
        for (given, property) in [
            ("same-outcome", Property::SameOutcome),
            ("exit-zero", Property::ExitZero),
            ("no-crash", Property::NoCrash),
            ("no-orphans", Property::NoOrphans),
            ("sdk", Property::Sdk),
            (
                "single-writer=*.log",
                Property::SingleWriter(Glob::new("*.log")),
            ),
            (
                "atomic-replace=*.conf",
                Property::AtomicReplace(Glob::new("*.conf")),
            ),
            (
                "writes-within=/build:/tmp",
                Property::WritesWithin(vec!["/build".into(), "/tmp".into()]),
            ),
            (
                "script=./adds-up.sh",
                Property::Script(PathBuf::from("./adds-up.sh")),
            ),
        ] {
            assert_eq!(given.parse::<Property>().unwrap(), property);
            assert_eq!(property.to_string(), given);
        }
    }

    // A misspelled property, an argument where none is taken, and a
    // script without a program are refused with the reason.
    #[test]
    fn a_bad_property_is_refused() {
        let error = |s: &str| format!("{:#}", s.parse::<Property>().unwrap_err());
        assert!(error("exit-0").contains("no property exit-0"));
        assert!(error("exit-zero=1").contains("takes no argument"));
        assert!(error("script").contains("script=PATH"));
        assert!(error("script=").contains("script=PATH"));
        assert!(error("atomic-replace").contains("atomic-replace=GLOB"));
        assert!(error("writes-within=build").contains("absolute directories, not build"));
    }

    // A properties file skips blanks and comments, and takes a script's
    // relative path from the file's directory.
    #[test]
    fn a_properties_file_lists_one_a_line() {
        let dir = std::env::temp_dir().join(format!("rewind-properties-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rewind.properties");
        std::fs::write(
            &file,
            "# what correct means\nno-crash\n\n  script=checks/adds-up.sh\n",
        )
        .unwrap();
        assert_eq!(
            Property::read_file(&file).unwrap(),
            vec![
                Property::NoCrash,
                Property::Script(dir.join("checks/adds-up.sh")),
            ]
        );

        std::fs::write(&file, "no-crash\nexit-0\n").unwrap();
        let error = format!("{:#}", Property::read_file(&file).unwrap_err());
        assert!(error.contains("rewind.properties:2"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A script is named by its file name, without directory or
    // extension.
    #[test]
    fn a_script_is_named_by_its_file() {
        let p: Property = "script=properties/report-adds-up.sh".parse().unwrap();
        assert_eq!(p.name(), "report-adds-up");
        assert_eq!(Property::ExitZero.name(), "exit-zero");
        assert_eq!(
            Property::AtomicReplace(Glob::new("*.conf")).name(),
            "atomic-replace"
        );
    }

    // A script's first line is its reason, and a line that starts with
    // `step N:` also names the step.
    #[test]
    fn a_script_says_why_and_where() {
        assert_eq!(
            script_witness("step 4021: the report is short\nmore\n"),
            (Some(4021), "the report is short".to_string())
        );
        assert_eq!(
            script_witness("\n  total 19, expected 20\n"),
            (None, "total 19, expected 20".to_string())
        );
        assert_eq!(
            script_witness(""),
            (None, "violated, without saying why".to_string())
        );
        assert_eq!(
            script_witness("step later: no number"),
            (None, "step later: no number".to_string())
        );
    }
}
