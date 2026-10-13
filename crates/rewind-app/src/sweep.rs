//! "Check from here": schedules tried from the playhead as forks of the
//! run, the way `rewind check --run RUN --schedule-from STEP --all
//! --no-narrow --json` tries them, and how each ended against the run.
//! With several seeds, each seed after the run's own reseeds the run's
//! randomness at the step and is tried unperturbed and under every
//! schedule.

use std::path::PathBuf;

use serde::Deserialize;

/// How many schedules a check from here can try with the run's own seed,
/// and how many it tries until another count is chosen.
pub const SIZES: [u64; 4] = [8, 16, 32, 64];
pub const DEFAULT_SCHEDULES: u64 = 16;

/// The checks from here that try several seeds: how many seeds, and how
/// many schedules each.
pub const SEEDED: [(u64, u64); 1] = [(8, 8)];

/// What the amber button at the end of the controls does at the
/// playhead: fork the run once, or check it with this many seeds and
/// schedules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HereAction {
    #[default]
    Fork,
    Check {
        schedules: u64,
        seeds: u64,
    },
}

/// What `rewind check --json` prints once it has tried every run.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Checked {
    /// The run itself, standing for schedule 0 of its own seed, then each
    /// run tried.
    pub schedules: Vec<Tried>,
    pub tried: u64,
    pub differing: u64,
    /// The `sometimes` assertions no run made true: the check never
    /// reached what they describe.
    pub sometimes_never: Vec<String>,
}

/// One run `rewind check` tried.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Tried {
    pub id: String,
    pub dir: PathBuf,
    /// The seed its randomness came from: the run's own, or the one a
    /// fork reseeded it with.
    pub seed: u64,
    pub schedule: u64,
    /// How it ended, in the app's words: exited:2, killed:SIGSEGV,
    /// timed-out.
    pub ending: Option<String>,
    /// Whether its verdict differs from the run's.
    pub differs: bool,
    /// What it violated, when the check judged properties.
    pub violations: Vec<Violation>,
}

/// A property a run violated, the step to look at when there is one, and
/// why.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Violation {
    pub property: String,
    pub step: Option<u64>,
    pub why: String,
}

/// How a run ended against the run it was forked from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// As the run did.
    Same,
    /// Some other way.
    Differs,
    /// It ran out of time, which says nothing of how it would have ended.
    TimedOut,
}

/// A place in a check's grid: the seed, counted from the run's own, and
/// the schedule.
pub type Cell = (u64, u64);

impl Tried {
    pub fn outcome(&self) -> Outcome {
        if self.ending.as_deref() == Some(rewind_trace::stop::TIMED_OUT_ENDING) {
            return Outcome::TimedOut;
        }
        if self.differs {
            return Outcome::Differs;
        }
        Outcome::Same
    }
}

impl Checked {
    /// The runs tried, without the run itself.
    pub fn forks(&self) -> &[Tried] {
        self.schedules.get(1..).unwrap_or_default()
    }

    /// The run tried at `cell`, if the check tried one there: a seed
    /// whose unperturbed run already differed is not perturbed further.
    pub fn at(&self, cell: Cell) -> Option<&Tried> {
        let first = self.schedules.first()?.seed;
        self.forks()
            .iter()
            .find(|t| t.seed.wrapping_sub(first) == cell.0 && t.schedule == cell.1)
    }

    /// How the runs ended, in words: "11 ended as this run did
    /// (killed:SIGSEGV), 4 differently, 1 timed out".
    pub fn summary(&self) -> String {
        let count = |outcome: Outcome| {
            self.forks()
                .iter()
                .filter(|t| t.outcome() == outcome)
                .count()
        };
        let ending = self
            .schedules
            .first()
            .and_then(|run| run.ending.as_deref())
            .unwrap_or_default();
        let mut parts = vec![format!(
            "{} ended as this run did ({ending})",
            count(Outcome::Same)
        )];
        let differs = count(Outcome::Differs);
        if differs > 0 {
            parts.push(format!("{differs} differently"));
        }
        let timed_out = count(Outcome::TimedOut);
        if timed_out > 0 {
            parts.push(format!("{timed_out} timed out"));
        }
        parts.join(", ")
    }
}

/// The cells of a check of `seeds` seeds and `schedules` schedules, in
/// order: the run's own seed's schedules, then each other seed's
/// unperturbed run and schedules.
pub fn cells(seeds: u64, schedules: u64) -> Vec<Cell> {
    (0..seeds)
        .flat_map(|seed| {
            let first = if seed == 0 { 1 } else { 0 };
            (first..=schedules).map(move |schedule| (seed, schedule))
        })
        .collect()
}

/// The seed and schedule a line `rewind check` says while it works is
/// about, as in "schedule   3: exited:1 ..." or, with several seeds,
/// "seed  2 schedule   3: ...": the progress of a check from here.
pub fn schedule_said(line: &str) -> Option<(Option<u64>, u64)> {
    let (seed, rest) = match line.strip_prefix(SEED_LINE) {
        Some(rest) => {
            let rest = rest.trim_start();
            let (number, rest) = rest.split_once(' ')?;
            (Some(number.parse().ok()?), rest.trim_start())
        }
        None => (None, line),
    };
    let (number, _) = rest.strip_prefix(SCHEDULE_LINE)?.split_once(':')?;
    Some((seed, number.trim().parse().ok()?))
}

/// How `rewind check` starts the line it says for each run.
const SEED_LINE: &str = "seed ";
const SCHEDULE_LINE: &str = "schedule ";

#[cfg(test)]
mod tests {
    // Checks from made-up JSON in the shape `rewind check --json` prints.
    use super::*;

    fn tried(seed: u64, schedule: u64, ending: &str, differs: bool) -> Tried {
        Tried {
            id: format!("run{seed}-{schedule}"),
            dir: PathBuf::from(format!("/runs/run{seed}-{schedule}")),
            seed,
            schedule,
            ending: Some(ending.to_string()),
            differs,
            violations: Vec::new(),
        }
    }

    /// A schedule that timed out counts as timed out whether or not its
    /// ending differs; the rest are the same or differ as check says.
    #[test]
    fn a_schedule_ended_the_same_differently_or_out_of_time() {
        assert_eq!(
            tried(0, 1, "killed:SIGSEGV", false).outcome(),
            Outcome::Same
        );
        assert_eq!(tried(0, 2, "exited:0", true).outcome(), Outcome::Differs);
        assert_eq!(tried(0, 3, "timed-out", true).outcome(), Outcome::TimedOut);
    }

    /// The JSON reads with the fields the app uses and ignores the rest;
    /// the summary counts each outcome and names the run's ending, and a
    /// violation reads with its property, step and reason.
    #[test]
    fn the_engines_json_reads_and_sums_up() {
        let json = r#"{"schedules":[
            {"id":"base","dir":"/runs/base","seed":0,"schedule":0,"ending":"killed:SIGSEGV","differs":false,"violations":[],"steps":4412},
            {"id":"a","dir":"/runs/a","seed":0,"schedule":1,"ending":"killed:SIGSEGV","differs":false,"violations":[]},
            {"id":"b","dir":"/runs/b","seed":0,"schedule":2,"ending":"exited:0","differs":true,"violations":[{"property":"sdk","step":321,"why":"always \"even\" failed"}]},
            {"id":"c","dir":"/runs/c","seed":0,"schedule":3,"ending":"timed-out","differs":true,"violations":[]},
            {"id":"d","dir":"/runs/d","seed":0,"schedule":4,"ending":"exited:0","differs":true,"violations":[]}
        ],"tried":4,"differing":3,"sometimes_never":["huge-byte"],"schedule_0_failed":true,"narrowed":null}"#;
        let checked: Checked = serde_json::from_str(json).unwrap();
        assert_eq!(checked.forks().len(), 4);
        assert_eq!(checked.forks()[1].id, "b");
        assert_eq!(checked.forks()[1].violations[0].step, Some(321));
        assert_eq!(checked.sometimes_never, vec!["huge-byte".to_string()]);
        assert_eq!(
            checked.summary(),
            "1 ended as this run did (killed:SIGSEGV), 2 differently, 1 timed out"
        );

        // Nothing timed out, nothing differs.
        let same = Checked {
            schedules: vec![
                tried(0, 0, "exited:0", false),
                tried(0, 1, "exited:0", false),
            ],
            tried: 1,
            differing: 0,
            sometimes_never: Vec::new(),
        };
        assert_eq!(same.summary(), "1 ended as this run did (exited:0)");
    }

    /// A check of several seeds has a row per seed, the run's own without
    /// its schedule 0; a run is found at its cell by its seed counted
    /// from the run's own, and a cell the check skipped has none.
    #[test]
    fn a_seeded_check_has_a_cell_per_seed_and_schedule() {
        assert_eq!(cells(1, 3), vec![(0, 1), (0, 2), (0, 3)]);
        assert_eq!(cells(2, 2), vec![(0, 1), (0, 2), (1, 0), (1, 1), (1, 2)]);
        let checked = Checked {
            schedules: vec![
                tried(5, 0, "exited:0", false),
                tried(5, 1, "exited:0", false),
                tried(6, 0, "exited:1", true),
            ],
            tried: 2,
            differing: 1,
            sometimes_never: Vec::new(),
        };
        assert_eq!(checked.at((1, 0)).map(|t| t.seed), Some(6));
        assert_eq!(checked.at((1, 1)), None);
    }

    /// Check's per-run lines name their schedule, and with several seeds
    /// their seed; other lines do not.
    #[test]
    fn progress_lines_name_their_seed_and_schedule() {
        assert_eq!(
            schedule_said("schedule   3: exited:1        355 steps    run 28fb816f"),
            Some((None, 3))
        );
        assert_eq!(schedule_said("schedule  16: exited:0"), Some((None, 16)));
        assert_eq!(
            schedule_said("seed  2 schedule   3: exited:0     323 steps    run 7bca"),
            Some((Some(2), 3))
        );
        assert_eq!(
            schedule_said("4 of 16 perturbed schedules ended differently"),
            None
        );
        assert_eq!(schedule_said("rewind: schedules 1..8 executing"), None);
        assert_eq!(schedule_said("seed 2 decides it: unperturbed"), None);
    }
}
