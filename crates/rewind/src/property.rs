//! The properties `rewind check` judges each run it tries by.
//!
//! A property says whether a finished run is correct and, when it is not,
//! why and at which step. The search looks for a schedule whose verdict
//! differs from the unperturbed run's, then narrows it to the step that
//! decides it. With no property given, check asks for the same outcome as
//! the unperturbed run, which is what it asked before properties existed.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use rewind_core::{Home, Run};
use rewind_trace::ending::Ending;

use crate::show::{self, OutcomeKey};

/// The word a property is named by on the command line.
const SAME_OUTCOME: &str = "same-outcome";
const EXIT_ZERO: &str = "exit-zero";
const SCRIPT: &str = "script";

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
        match (name, arg) {
            (SAME_OUTCOME, None) => Ok(Property::SameOutcome),
            (EXIT_ZERO, None) => Ok(Property::ExitZero),
            (SCRIPT, Some(path)) if !path.is_empty() => Ok(Property::Script(PathBuf::from(path))),
            (SAME_OUTCOME | EXIT_ZERO, Some(_)) => bail!("property {name} takes no argument"),
            (SCRIPT, _) => bail!("property script needs a program: script=PATH"),
            _ => bail!(
                "no property {name}; the properties are {SAME_OUTCOME}, {EXIT_ZERO} and {SCRIPT}=PATH"
            ),
        }
    }
}

impl fmt::Display for Property {
    /// The property as the command line names it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Property::SameOutcome => write!(f, "{SAME_OUTCOME}"),
            Property::ExitZero => write!(f, "{EXIT_ZERO}"),
            Property::Script(path) => write!(f, "{SCRIPT}={}", path.display()),
        }
    }
}

impl Property {
    /// The short name a report calls the property by: a script by its
    /// file name without its extension.
    pub fn name(&self) -> String {
        match self {
            Property::Script(path) => path.file_stem().map_or_else(
                || path.display().to_string(),
                |s| s.to_string_lossy().into_owned(),
            ),
            other => other.to_string(),
        }
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
        Ok(Judge {
            home,
            base_id: base.manifest.id.to_string(),
            base_ending: ending(base, &base_key),
            base_key,
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
    }

    // A script is named by its file name, without directory or
    // extension.
    #[test]
    fn a_script_is_named_by_its_file() {
        let p: Property = "script=properties/report-adds-up.sh".parse().unwrap();
        assert_eq!(p.name(), "report-adds-up");
        assert_eq!(Property::ExitZero.name(), "exit-zero");
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
