//! `--progress`: how the commands that bring a run back to a step, such as
//! `rewind cat`, say how far the replay to it got. On a terminal a status
//! line is drawn over itself and taken away once the replay is there; a
//! program such as the desktop app asks for a line of JSON each time
//! instead (see [`rewind_trace::replaying`]).

use std::sync::Mutex;
use std::time::Duration;

use rewind_core::progress::Replaying;
use rewind_trace::replaying::{Recent, about};

use crate::Terminal;

/// How replays say how far they got, on standard error.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Format {
    /// A status line, when standard error is a terminal.
    #[default]
    Auto,
    /// A line of JSON each time, for a program to read.
    Json,
    /// Nothing.
    Off,
}

/// Whether a command says how far its replays got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Says {
    /// It brings a run back to a step to look inside it, and says.
    Replays,
    /// It executes runs, or replays none, and says nothing of replays.
    Nothing,
}

/// Has every replay this process makes say how far it got, in `format`.
pub(crate) fn listen(format: Format) {
    match format {
        Format::Off => {}
        Format::Json => {
            rewind_core::progress::listen(Box::new(|r: &Replaying| eprintln!("{}", r.line())))
        }
        Format::Auto if Terminal::stderr() == Terminal::Yes => {
            // The time left goes by the replay's recent speed, heard anew
            // from each replay's start.
            let recent = Mutex::new(Recent::default());
            rewind_core::progress::listen(Box::new(move |r: &Replaying| {
                let mut recent = recent.lock().unwrap_or_else(|e| e.into_inner());
                if r.at == r.from {
                    *recent = Recent::default();
                }
                recent.heard(r);
                if r.done() {
                    crate::clear_status();
                } else {
                    crate::status(&line(r, recent.left(r)));
                }
            }))
        }
        Format::Auto => {}
    }
}

/// The status line for a replay: where it goes and, once its speed is
/// known, where it is and the time `left`.
pub(crate) fn line(r: &Replaying, left: Option<Duration>) -> String {
    let going = format!(
        "rewind: replaying run {} from step {} to step {}",
        r.run, r.from, r.to
    );
    match left {
        Some(left) => format!("{going}: step {}, {} left", r.at, about(left)),
        None => going,
    }
}

#[cfg(test)]
mod tests {
    // The status line from made-up progress: where the replay goes, then,
    // once there is a time left to go by, where it is and that time.
    use super::*;

    #[test]
    fn the_status_line_says_where_the_replay_is() {
        // 33,000 of 79,747 steps in 41 s leaves about a minute.
        let mut r = Replaying {
            run: "455fc36b67cc83be".into(),
            from: 50_000,
            at: 50_000,
            to: 129_747,
            elapsed_ms: 0,
        };
        assert_eq!(
            line(&r, None),
            "rewind: replaying run 455fc36b67cc83be from step 50000 to step 129747"
        );
        r.at = 83_000;
        r.elapsed_ms = 41_000;
        assert_eq!(
            line(&r, r.left()),
            "rewind: replaying run 455fc36b67cc83be from step 50000 to step 129747: step 83000, about 1 minute left"
        );
    }
}
