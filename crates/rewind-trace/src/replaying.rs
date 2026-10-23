//! How far a replay toward a step has got. The engine's commands that
//! bring a run back to a step, such as `rewind cat`, `rewind diff
//! --outputs` and `rewind where`, replay it from the nearest saved
//! keyframe, which can take minutes. With `--progress json` they print
//! where the replay is on standard error, a line of JSON each time, and
//! the desktop app reads the lines to say how long the wait is.

use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The option, and its environment variable, that choose how the engine
/// says how far a replay got.
pub const PROGRESS_OPTION: &str = "progress";
pub const PROGRESS_ENV: &str = "REWIND_PROGRESS";

/// The value of `--progress` that asks for a line of JSON each time.
pub const PROGRESS_JSON: &str = "json";

/// A replay of a run toward a step, as far as it has got.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replaying {
    /// The run's id.
    pub run: String,
    /// The step the replay started at: the keyframe it restored, or 0
    /// when it started at boot.
    pub from: u64,
    /// The step it has reached.
    pub at: u64,
    /// The step it goes to.
    pub to: u64,
    /// How long it has taken so far, in milliseconds.
    pub elapsed_ms: u64,
}

/// The line a replay's progress is printed as: `{"replaying": {...}}`.
#[derive(Serialize, Deserialize)]
struct Line {
    replaying: Replaying,
}

/// How far into a replay its speed is taken as known. The first seconds of
/// a replay restored from a keyframe go slower, while the guest's memory
/// is read back in: a replay of a sagittarius build restored at step
/// 152,410 first said 39 minutes were left, and twenty seconds later
/// about three.
const MEASURED_AFTER: Duration = Duration::from_secs(5);

/// Below this, the time left is said as a few seconds.
const FEW_SECONDS: Duration = Duration::from_secs(10);

/// A minute and an hour, for saying the time left in words.
const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(60 * 60);

/// Seconds between ten seconds and a minute are rounded to this many.
const SECONDS_ROUNDED_TO: u64 = 5;

impl Replaying {
    /// The line of JSON the engine prints.
    pub fn line(&self) -> String {
        let line = Line {
            replaying: self.clone(),
        };
        serde_json::to_string(&line).expect("a replay's progress serializes")
    }

    /// The replay a line the engine printed reports, if it reports one.
    pub fn parse(line: &str) -> Option<Replaying> {
        let line: Line = serde_json::from_str(line.trim()).ok()?;
        Some(line.replaying)
    }

    /// The steps the replay goes, in all.
    pub fn steps(&self) -> u64 {
        self.to.saturating_sub(self.from)
    }

    /// How much of the replay is done, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        let steps = self.steps();
        if steps == 0 {
            return 1.0;
        }
        let done = self.at.saturating_sub(self.from).min(steps);
        done as f32 / steps as f32
    }

    /// Whether the replay has reached its step.
    pub fn done(&self) -> bool {
        self.at >= self.to
    }

    /// The time the rest of the replay takes at the speed it has gone so
    /// far, once it has gone long enough to measure that speed.
    pub fn left(&self) -> Option<Duration> {
        let elapsed = Duration::from_millis(self.elapsed_ms);
        let done = self.at.saturating_sub(self.from);
        if elapsed < MEASURED_AFTER || done == 0 {
            return None;
        }
        let rest = self.to.saturating_sub(self.at);
        Some(elapsed.mul_f64(rest as f64 / done as f64))
    }
}

/// How much of a replay's latest stretch its recent speed is taken over.
const RECENT: Duration = Duration::from_secs(20);

/// A replay's reports over about its last RECENT, for the time left at
/// its recent speed. A replay's speed changes with what the guest does:
/// a sagittarius build replayed from boot to step 100,000 in 10 s and
/// then about 1,000 steps a second, so at step 150,000 its speed so far
/// would say about 56 s were left of a replay to step 290,000 that takes
/// 140 s more.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recent {
    /// The steps reached and the milliseconds taken, oldest first: the
    /// latest report at or before the window's start, then the rest.
    heard: VecDeque<(u64, u64)>,
}

impl Recent {
    /// Takes the replay's latest report.
    pub fn heard(&mut self, r: &Replaying) {
        self.heard.push_back((r.at, r.elapsed_ms));
        let start = r.elapsed_ms.saturating_sub(RECENT.as_millis() as u64);
        while self.heard.len() > 1 && self.heard[1].1 <= start {
            self.heard.pop_front();
        }
    }

    /// The time the rest of replay `r`, the latest heard, takes at its
    /// speed over its last RECENT, once it has gone long enough to measure
    /// it; at its speed so far when it reached no step in that time.
    pub fn left(&self, r: &Replaying) -> Option<Duration> {
        if Duration::from_millis(r.elapsed_ms) < MEASURED_AFTER {
            return None;
        }
        let &(at, elapsed_ms) = self.heard.front()?;
        let steps = r.at.saturating_sub(at);
        let ms = r.elapsed_ms.saturating_sub(elapsed_ms);
        if steps == 0 || Duration::from_millis(ms) < MEASURED_AFTER {
            return r.left();
        }
        let rest = r.to.saturating_sub(r.at);
        let left_ms = u128::from(rest) * u128::from(ms) / u128::from(steps);
        Some(Duration::from_millis(left_ms as u64))
    }
}

/// A time left in words: "a few seconds", "about 25 seconds", "about 1
/// minute", "about 12 minutes", "about 2 hours".
pub fn about(left: Duration) -> String {
    if left < FEW_SECONDS {
        return "a few seconds".to_string();
    }
    let seconds = left.as_secs().div_ceil(SECONDS_ROUNDED_TO) * SECONDS_ROUNDED_TO;
    if seconds < MINUTE.as_secs() {
        return format!("about {seconds} seconds");
    }
    if left < HOUR {
        let minutes = (left.as_secs_f64() / MINUTE.as_secs_f64()).round() as u64;
        return format!("about {minutes} {}", plural(minutes, "minute"));
    }
    let hours = (left.as_secs_f64() / HOUR.as_secs_f64()).round() as u64;
    format!("about {hours} {}", plural(hours, "hour"))
}

/// `unit`, with an s unless there is one of it.
fn plural(n: u64, unit: &str) -> String {
    if n == 1 {
        unit.to_string()
    } else {
        format!("{unit}s")
    }
}

#[cfg(test)]
mod tests {
    // The progress line and what is read from it: the line round-trips,
    // other lines are not progress, and the time left follows the speed
    // so far once there is enough of it.
    use super::*;

    fn replaying(at: u64, elapsed_ms: u64) -> Replaying {
        Replaying {
            run: "455fc36b67cc83be".into(),
            from: 50_000,
            at,
            to: 150_000,
            elapsed_ms,
        }
    }

    #[test]
    fn a_progress_line_reads_back() {
        // A replay's line parses to the same replay; the engine's other
        // lines, its messages and other JSON, are not progress.
        let r = replaying(83_000, 41_000);
        assert_eq!(Replaying::parse(&r.line()), Some(r.clone()));
        assert_eq!(Replaying::parse(&format!("  {}\n", r.line())), Some(r));
        assert_eq!(Replaying::parse("rewind: walking thread 3's stack"), None);
        assert_eq!(Replaying::parse(r#"{"run":"a","step":1}"#), None);
    }

    #[test]
    fn the_time_left_follows_the_speed_so_far() {
        // 25,000 of 100,000 steps in 50 s leaves 75,000 at 500 a second:
        // 150 s. Before five seconds, or before a step, there is no
        // estimate; at the end none is left.
        assert_eq!(
            replaying(75_000, 50_000).left(),
            Some(Duration::from_secs(150))
        );
        assert_eq!(replaying(50_100, 4_900).left(), None);
        assert_eq!(replaying(50_000, 9_000).left(), None);
        assert_eq!(replaying(150_000, 9_000).left(), Some(Duration::ZERO));
        assert_eq!(replaying(75_000, 1).fraction(), 0.25);
        assert!(replaying(150_000, 1).done());
        assert!(!replaying(149_999, 1).done());
    }

    #[test]
    fn the_time_left_follows_the_recent_speed() {
        // 100,000 steps in the first 10 s, then 500 a second for 30 s, of
        // 200,000: the speed so far says 30 s are left, the last RECENT
        // of it says 170 s, which the replay takes. Heard a report every
        // half second, as the engine says them.
        let mut recent = Recent::default();
        let mut last = replaying(0, 0);
        for tick in 0..=80u64 {
            let elapsed_ms = tick * 500;
            let at = if elapsed_ms <= 10_000 {
                elapsed_ms * 10
            } else {
                100_000 + (elapsed_ms - 10_000) / 2
            };
            last = Replaying {
                from: 0,
                to: 200_000,
                ..replaying(at, elapsed_ms)
            };
            recent.heard(&last);
        }
        assert_eq!(last.at, 115_000);
        assert_eq!(last.left().map(|l| l.as_secs()), Some(29));
        assert_eq!(recent.left(&last), Some(Duration::from_secs(170)));

        // Before five seconds there is no estimate, recent or not.
        let mut early = Recent::default();
        let first = replaying(60_000, 4_000);
        early.heard(&first);
        assert_eq!(early.left(&first), None);
    }

    #[test]
    fn the_time_left_is_said_in_round_words() {
        // Each unit at its edges, rounded as a person would say it.
        let s = Duration::from_secs;
        assert_eq!(about(s(3)), "a few seconds");
        assert_eq!(about(s(10)), "about 10 seconds");
        assert_eq!(about(s(22)), "about 25 seconds");
        assert_eq!(about(s(54)), "about 55 seconds");
        assert_eq!(about(s(59)), "about 1 minute");
        assert_eq!(about(s(60)), "about 1 minute");
        assert_eq!(about(s(100)), "about 2 minutes");
        assert_eq!(about(s(59 * 60)), "about 59 minutes");
        assert_eq!(about(s(60 * 60)), "about 1 hour");
        assert_eq!(about(s(150 * 60)), "about 3 hours");
    }
}
