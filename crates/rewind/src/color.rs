//! `--color`: whether rewind colors what it prints. A verdict reads green
//! when two things are the same and red when they differ, the left run of
//! two (or the recording) amber and the right one (or the replay) blue, as
//! in the desktop app's Compare view, and rewind's own `rewind:` lines dim
//! beside the job's output. Each of standard output and standard error is
//! decided on its own, so `rewind replay x | less` gets plain text while
//! the notes on the terminal stay colored.

use std::ffi::OsString;
use std::fmt::Display;
use std::io::IsTerminal;
use std::sync::OnceLock;

use clap::ValueEnum;

/// The environment variable that sets `--color`.
pub(crate) const ENV: &str = "REWIND_COLOR";

/// The option itself, which [`asked`] finds before clap parses.
const OPTION: &str = "--color";

/// The argument after which every other is the job's, not rewind's.
const END_OF_OPTIONS: &str = "--";

/// Set to anything but empty, no program colors its output
/// (no-color.org).
const NO_COLOR: &str = "NO_COLOR";

/// Set to anything but empty or 0, a program colors its output even into
/// a pipe, as clap and cargo do.
const CLICOLOR_FORCE: &str = "CLICOLOR_FORCE";

/// The environment variable that names the terminal's type.
const TERM: &str = "TERM";

/// The terminal type that draws no color.
const DUMB_TERMINAL: &str = "dumb";

/// The CLICOLOR_FORCE value that forces nothing.
const NOT_FORCED: &str = "0";

/// When to color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum Choice {
    /// On a terminal, unless NO_COLOR is set or TERM is dumb; also into a
    /// pipe when CLICOLOR_FORCE is set.
    #[default]
    Auto,
    /// Always, even into a pipe or a file.
    Always,
    /// Never.
    Never,
}

impl Choice {
    /// The same choice for clap's help and errors.
    pub(crate) fn clap(self) -> clap::ColorChoice {
        match self {
            Choice::Auto => clap::ColorChoice::Auto,
            Choice::Always => clap::ColorChoice::Always,
            Choice::Never => clap::ColorChoice::Never,
        }
    }
}

/// The `--color` in `args`, the program's arguments with its name first,
/// else the environment's [`ENV`]. Clap draws its help and errors while it
/// parses, so they need the choice before clap has it.
pub(crate) fn asked(args: impl IntoIterator<Item = OsString>) -> Choice {
    // The last --color before the job's own arguments wins, as in clap.
    let mut found = None;
    let mut args = args.into_iter().skip(1);
    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else {
            continue;
        };
        if arg == END_OF_OPTIONS {
            break;
        }
        let value = if arg == OPTION {
            args.next().and_then(|v| v.into_string().ok())
        } else if let Some(v) = arg.strip_prefix(OPTION).and_then(|v| v.strip_prefix('=')) {
            Some(v.to_string())
        } else {
            continue;
        };
        if let Some(choice) = value.and_then(|v| Choice::from_str(&v, true).ok()) {
            found = Some(choice);
        }
    }
    if let Some(choice) = found {
        return choice;
    }

    // Otherwise the environment's, and auto when it names none.
    std::env::var(ENV)
        .ok()
        .and_then(|v| Choice::from_str(&v, true).ok())
        .unwrap_or_default()
}

/// Whether a stream is a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Terminal,
    Other,
}

/// What the environment says of color, read once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Env {
    no_color: Option<String>,
    force: Option<String>,
    term: Option<String>,
}

impl Env {
    fn read() -> Env {
        Env {
            no_color: std::env::var(NO_COLOR).ok(),
            force: std::env::var(CLICOLOR_FORCE).ok(),
            term: std::env::var(TERM).ok(),
        }
    }
}

/// How a stream is painted under `choice`. Under auto, CLICOLOR_FORCE
/// comes before NO_COLOR, as in clap, so rewind's help and its output
/// agree.
fn decide(choice: Choice, stream: Stream, env: &Env) -> Paint {
    match choice {
        Choice::Never => return Paint::Plain,
        Choice::Always => return Paint::Ansi,
        Choice::Auto => {}
    }
    let set = |v: &Option<String>| v.as_deref().is_some_and(|v| !v.is_empty());
    if set(&env.force) && env.force.as_deref() != Some(NOT_FORCED) {
        return Paint::Ansi;
    }
    if set(&env.no_color) {
        return Paint::Plain;
    }
    if env.term.as_deref() == Some(DUMB_TERMINAL) {
        return Paint::Plain;
    }
    match stream {
        Stream::Terminal => Paint::Ansi,
        Stream::Other => Paint::Plain,
    }
}

/// How standard output and standard error are painted, once [`init`] has
/// decided; plain until then, as in tests.
static STDOUT: OnceLock<Paint> = OnceLock::new();
static STDERR: OnceLock<Paint> = OnceLock::new();

/// Decides how each stream is painted under `choice`.
pub(crate) fn init(choice: Choice) {
    let env = Env::read();
    let stream = |terminal: bool| {
        if terminal {
            Stream::Terminal
        } else {
            Stream::Other
        }
    };
    let _ = STDOUT.set(decide(
        choice,
        stream(std::io::stdout().is_terminal()),
        &env,
    ));
    let _ = STDERR.set(decide(
        choice,
        stream(std::io::stderr().is_terminal()),
        &env,
    ));
}

/// How text for standard output is painted.
pub(crate) fn stdout() -> Paint {
    STDOUT.get().copied().unwrap_or(Paint::Plain)
}

/// How text for standard error is painted.
pub(crate) fn stderr() -> Paint {
    STDERR.get().copied().unwrap_or(Paint::Plain)
}

/// What a piece of text means, which decides its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// The same as what it was compared with: identical, held, matches,
    /// exited 0.
    Same,
    /// Different from what it was compared with, or failed.
    Differs,
    /// The left run of two, or the recording a replay is compared with.
    Left,
    /// The right run of two, or the replay.
    Right,
    /// Something that works less well, though it works.
    Warn,
    /// Rewind's own `rewind:` and asides that are not the answer.
    Dim,
    /// The `rewind:` of an error.
    Error,
}

impl Style {
    /// The style's Select Graphic Rendition parameters: the terminal's
    /// own 16 colors, so its theme decides the shades.
    fn sgr(self) -> &'static str {
        match self {
            Style::Same => "32",
            Style::Differs => "31",
            Style::Left => "33",
            Style::Right => "34",
            Style::Warn => "33",
            Style::Dim => "2",
            Style::Error => "1;31",
        }
    }
}

/// The prefix of every line rewind prints about itself.
const PREFIX: &str = "rewind:";

/// How text for one stream is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Paint {
    /// As it is.
    Plain,
    /// In ANSI escape sequences.
    Ansi,
}

impl Paint {
    /// `text` in `style`.
    pub(crate) fn paint(self, style: Style, text: impl Display) -> String {
        match self {
            Paint::Plain => text.to_string(),
            Paint::Ansi => format!("\x1b[{}m{text}\x1b[0m", style.sgr()),
        }
    }

    /// `text` padded to `width` columns, then painted, so a column lines
    /// up whatever bytes the escapes add.
    pub(crate) fn padded(self, style: Style, text: impl Display, width: usize) -> String {
        let text = text.to_string();
        let pad = width.saturating_sub(text.chars().count());
        format!("{}{}", self.paint(style, text), " ".repeat(pad))
    }

    /// `rewind:`, dim, to start a line rewind prints about itself.
    pub(crate) fn prefix(self) -> String {
        self.paint(Style::Dim, PREFIX)
    }

    /// `rewind:` for an error.
    pub(crate) fn error_prefix(self) -> String {
        self.paint(Style::Error, PREFIX)
    }

    /// `text` in `style` when `same` says it matched what it was
    /// compared with, else in [`Style::Differs`].
    pub(crate) fn verdict(self, same: Same, text: impl Display) -> String {
        match same {
            Same::Yes => self.paint(Style::Same, text),
            Same::No => self.paint(Style::Differs, text),
        }
    }
}

/// Whether something matched what it was compared with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Same {
    Yes,
    No,
}

impl From<bool> for Same {
    fn from(same: bool) -> Same {
        if same { Same::Yes } else { Same::No }
    }
}

#[cfg(test)]
mod tests {
    // How rewind decides to color each stream, finds --color before clap
    // parses, and writes the escapes.
    use super::*;

    fn args(line: &str) -> Vec<OsString> {
        line.split_whitespace().map(OsString::from).collect()
    }

    #[test]
    fn auto_colors_a_terminal_unless_the_environment_says_otherwise() {
        // A terminal is colored and a pipe is not; NO_COLOR and a dumb
        // terminal turn color off, CLICOLOR_FORCE on, and always and never
        // ignore all three.
        let none = Env::default();
        assert_eq!(decide(Choice::Auto, Stream::Terminal, &none), Paint::Ansi);
        assert_eq!(decide(Choice::Auto, Stream::Other, &none), Paint::Plain);

        let no_color = Env {
            no_color: Some("1".into()),
            ..Env::default()
        };
        assert_eq!(
            decide(Choice::Auto, Stream::Terminal, &no_color),
            Paint::Plain
        );
        assert_eq!(
            decide(Choice::Always, Stream::Other, &no_color),
            Paint::Ansi
        );

        let empty_no_color = Env {
            no_color: Some(String::new()),
            ..Env::default()
        };
        assert_eq!(
            decide(Choice::Auto, Stream::Terminal, &empty_no_color),
            Paint::Ansi
        );

        let dumb = Env {
            term: Some(DUMB_TERMINAL.into()),
            ..Env::default()
        };
        assert_eq!(decide(Choice::Auto, Stream::Terminal, &dumb), Paint::Plain);

        let forced = Env {
            force: Some("1".into()),
            no_color: Some("1".into()),
            ..Env::default()
        };
        assert_eq!(decide(Choice::Auto, Stream::Other, &forced), Paint::Ansi);
        assert_eq!(
            decide(Choice::Never, Stream::Terminal, &forced),
            Paint::Plain
        );

        let not_forced = Env {
            force: Some(NOT_FORCED.into()),
            ..Env::default()
        };
        assert_eq!(
            decide(Choice::Auto, Stream::Other, &not_forced),
            Paint::Plain
        );
    }

    #[test]
    fn the_last_color_option_before_the_job_wins() {
        // Both spellings are found, the last one counts, and a --color
        // after `--` is the job's.
        assert_eq!(asked(args("rewind --color never ls")), Choice::Never);
        assert_eq!(asked(args("rewind ls --color=always")), Choice::Always);
        assert_eq!(
            asked(args("rewind --color always ls --color never")),
            Choice::Never
        );
        assert_eq!(
            asked(args(
                "rewind --color never run --root r -- ls --color=always"
            )),
            Choice::Never
        );
    }

    #[test]
    fn painted_text_keeps_its_column() {
        // Plain text is untouched; painted text is wrapped in its style
        // and a reset, and padding goes after the reset so columns line
        // up.
        assert_eq!(Paint::Plain.paint(Style::Same, "identical"), "identical");
        assert_eq!(
            Paint::Ansi.paint(Style::Differs, "DIFFERENT"),
            "\x1b[31mDIFFERENT\x1b[0m"
        );
        assert_eq!(Paint::Plain.padded(Style::Same, "ok", 6), "ok    ");
        assert_eq!(
            Paint::Ansi.padded(Style::Same, "ok", 6),
            "\x1b[32mok\x1b[0m    "
        );
        assert_eq!(Paint::Ansi.prefix(), "\x1b[2mrewind:\x1b[0m");
    }
}
