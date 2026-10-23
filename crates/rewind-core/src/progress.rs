//! Who hears how far a replay toward a step has got. A command that
//! brings a run back to a step, such as `rewind cat`, names a listener,
//! and [`crate::run::Run::machine_at`] tells it where the replay is as it
//! goes: once as it starts, then at most every SAY_EVERY, then once it
//! is there. Without a listener nothing is said.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub use rewind_trace::replaying::Replaying;

/// What hears a replay's progress.
pub type Listener = Box<dyn Fn(&Replaying) + Send + Sync>;

/// The process's listener, once a command has named one.
static LISTENER: OnceLock<Listener> = OnceLock::new();

/// How often a replay says how far it got, at most, between its start
/// and its end.
const SAY_EVERY: Duration = Duration::from_millis(500);

/// Names the listener every replay in this process tells. A command
/// names one at most once; a second is ignored.
pub fn listen(listener: Listener) {
    let _ = LISTENER.set(listener);
}

/// The process's listener, if a command named one.
pub(crate) fn listener() -> Option<&'static Listener> {
    LISTENER.get()
}

/// One replay's reports to a listener.
pub(crate) struct Reports<'a> {
    listener: Option<&'a (dyn Fn(&Replaying) + Send + Sync)>,
    replaying: Replaying,
    started: Instant,
    said: Instant,
}

impl<'a> Reports<'a> {
    /// The reports of a replay of `run` from step `from` to `to`, begun
    /// at `now`, to `listener`, which hears the start at once.
    pub(crate) fn start(
        listener: Option<&'a (dyn Fn(&Replaying) + Send + Sync)>,
        run: &str,
        (from, to): (u64, u64),
        now: Instant,
    ) -> Reports<'a> {
        let reports = Reports {
            listener,
            replaying: Replaying {
                run: run.to_string(),
                from,
                at: from,
                to,
                elapsed_ms: 0,
            },
            started: now,
            said: now,
        };
        reports.say();
        reports
    }

    /// The replay has reached step `at` at `now`: said when SAY_EVERY has
    /// passed since the last report.
    pub(crate) fn reached(&mut self, at: u64, now: Instant) {
        self.replaying.at = at;
        self.replaying.elapsed_ms = now.duration_since(self.started).as_millis() as u64;
        if now.duration_since(self.said) < SAY_EVERY {
            return;
        }
        self.said = now;
        self.say();
    }

    /// The replay has ended at step `at`, at `now`, its step or where the
    /// machine stopped short of it: always said.
    pub(crate) fn ended(&mut self, at: u64, now: Instant) {
        self.replaying.at = at;
        self.replaying.elapsed_ms = now.duration_since(self.started).as_millis() as u64;
        self.said = now;
        self.say();
    }

    fn say(&self) {
        if let Some(listener) = self.listener {
            listener(&self.replaying);
        }
    }
}

#[cfg(test)]
mod tests {
    // A replay's reports to a listener that keeps them: the start at once,
    // stops on the way only every SAY_EVERY, and the end always.
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn a_replay_says_its_start_its_way_and_its_end() {
        // Stops every 100 ms from step 1000 to 2000: the start, then one
        // report a SAY_EVERY, then the end, with the steps and times so
        // far.
        let heard = Mutex::new(Vec::new());
        let listener = |r: &Replaying| heard.lock().unwrap().push((r.at, r.elapsed_ms));
        let started = Instant::now();
        let mut reports = Reports::start(Some(&listener), "r", (1000, 2000), started);
        let tick = Duration::from_millis(100);
        for i in 1..=12u32 {
            reports.reached(1000 + 50 * u64::from(i), started + tick * i);
        }
        reports.ended(2000, started + tick * 13);
        assert_eq!(
            heard.into_inner().unwrap(),
            vec![(1000, 0), (1250, 500), (1500, 1000), (2000, 1300)]
        );
    }
}
