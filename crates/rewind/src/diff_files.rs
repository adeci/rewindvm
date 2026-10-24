//! `rewind diff --file` and `rewind diff --outputs`: how the files two
//! runs' jobs left behind differ, as each run had them when its job
//! exited.

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use rewind_core::inspect::{self, Inspection};
use rewind_core::{Home, Run};
use rewind_trace::file_diff::{self, ByteDiff, FileDiff, Line, LineDiff, Sides};
use rewind_trace::properties::{Glob, written_files};

use crate::cmd::view::Format;
use crate::property::end_step;

/// What the summaries call the two runs, as the rest of `rewind diff`
/// does.
pub(crate) const SIDES: Sides<'static> = Sides {
    a: "the left run",
    b: "the right run",
};

/// The lines an excerpt shows before the first line that differs.
const CONTEXT_LINES: usize = 3;

/// The most lines a text excerpt shows, context included.
const EXCERPT_LINES: usize = 16;

/// What an excerpt's lines start with: a line both runs have, one only
/// the left run has, and one only the right run has.
const MARK_BOTH: &str = "  ";
const MARK_LEFT: &str = "- ";
const MARK_RIGHT: &str = "+ ";

/// The line before a text excerpt that says what its marks mean.
pub(crate) const TEXT_LEGEND: &str =
    "Lines marked - are only in the left run. Lines marked + are only in the right run.";

/// Bytes per line of a byte excerpt, and per group within a line.
const BYTES_PER_LINE: usize = 16;
const BYTES_PER_GROUP: usize = 8;

/// The lines of bytes a byte excerpt shows, from the one with the first
/// difference.
const EXCERPT_BYTE_LINES: usize = 4;

/// The line before a byte excerpt that says what its marks mean.
pub(crate) const BYTES_LEGEND: &str = "Bytes marked ^^ differ.";

/// The first byte that `characters` shows as itself, and the last.
const PRINTABLE: std::ops::RangeInclusive<u8> = 0x20..=0x7e;

/// The lines around the first difference of two text copies, `a` the
/// left run's lines and `b` the right run's, each marked by which runs
/// have it.
pub(crate) fn text_excerpt(a: &[String], b: &[String], diff: &LineDiff) -> Vec<String> {
    let first = diff.lines.iter().position(changed).unwrap_or(0);
    diff.lines
        .iter()
        .skip(first.saturating_sub(CONTEXT_LINES))
        .take(EXCERPT_LINES)
        .map(|line| match *line {
            Line::Both { a: i, .. } => format!("{MARK_BOTH}{}", a[i]),
            Line::OnlyA(i) => format!("{MARK_LEFT}{}", a[i]),
            Line::OnlyB(j) => format!("{MARK_RIGHT}{}", b[j]),
        })
        .collect()
}

/// The lines of bytes from the first difference of two binary copies:
/// each run's bytes, then a line marking the bytes that differ.
pub(crate) fn byte_excerpt(a: &[u8], b: &[u8], diff: &ByteDiff) -> Vec<String> {
    /// What each run's line starts with, padded to one width.
    const LEFT: &str = "left   ";
    const RIGHT: &str = "right  ";
    const BOTH: &str = "both   ";

    // Where a byte's pair starts in a line of `hex_line`: past the run's
    // name, the offset and the gap after it.
    const OFFSET_WIDTH: usize = 8 + 2;
    const PAIR_WIDTH: usize = 3;
    let pair_column = |i: usize| LEFT.len() + OFFSET_WIDTH + i * PAIR_WIDTH + i / BYTES_PER_GROUP;

    let Some(first) = diff.first() else {
        return Vec::new();
    };
    let start = first - first % BYTES_PER_LINE;
    let mut excerpt = Vec::new();
    for offset in (start..diff.compared)
        .step_by(BYTES_PER_LINE)
        .take(EXCERPT_BYTE_LINES)
    {
        let end = offset + BYTES_PER_LINE;
        let chunk = |bytes: &[u8]| bytes[offset.min(bytes.len())..end.min(bytes.len())].to_vec();
        let (here, there) = (chunk(a), chunk(b));

        // A line the runs share is shown once.
        if here == there {
            excerpt.push(format!("{BOTH}{}", hex_line(offset, &here)));
            continue;
        }
        excerpt.push(format!("{LEFT}{}", hex_line(offset, &here)));
        excerpt.push(format!("{RIGHT}{}", hex_line(offset, &there)));

        // A ^^ under each pair that differs.
        let mut marks = String::new();
        for i in (0..BYTES_PER_LINE).filter(|i| diff.differs_at(offset + i)) {
            let column = pair_column(i);
            marks.push_str(&" ".repeat(column - marks.len()));
            marks.push_str("^^");
        }
        excerpt.push(marks);
    }
    excerpt
}

/// One line of bytes as `hexdump -C` lays it out: the offset, the bytes
/// in two groups, and the printable ones as characters.
fn hex_line(offset: usize, chunk: &[u8]) -> String {
    let mut hex = String::new();
    for i in 0..BYTES_PER_LINE {
        if i == BYTES_PER_GROUP {
            hex.push(' ');
        }
        match chunk.get(i) {
            Some(b) => hex.push_str(&format!("{b:02x} ")),
            None => hex.push_str("   "),
        }
    }
    let characters: String = chunk
        .iter()
        .map(|b| {
            if PRINTABLE.contains(b) {
                *b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("{offset:08x}  {hex} {characters}")
}

/// How one file the jobs wrote differs between the runs.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct OutputDiff {
    pub path: String,
    /// The file's size in each run, or None where it was not there.
    pub left_size: Option<usize>,
    pub right_size: Option<usize>,
    /// How many byte offsets differ, and the first that does.
    pub differing_bytes: usize,
    pub first_byte: Option<usize>,
}

/// The files of `paths` whose bytes differ between `left` and `right`,
/// each run's files by path, in the order of `paths`; and how many of
/// `paths` were the same in both. A path neither run has is left out.
pub(crate) fn differing_outputs(
    paths: &[String],
    left: &[(String, &[u8])],
    right: &[(String, &[u8])],
) -> (Vec<OutputDiff>, usize) {
    let find = |files: &[(String, &[u8])], path: &str| -> Option<Vec<u8>> {
        files
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, bytes)| bytes.to_vec())
    };
    let mut differ = Vec::new();
    let mut same = 0;
    for path in paths {
        let (here, there) = (find(left, path), find(right, path));
        if here.is_none() && there.is_none() {
            continue;
        }
        if here == there {
            same += 1;
            continue;
        }

        // A file only one run has differs in every byte it has.
        let (a, b) = (
            here.as_deref().unwrap_or(&[]),
            there.as_deref().unwrap_or(&[]),
        );
        let bytes = file_diff::byte_diff(a, b);
        differ.push(OutputDiff {
            path: path.clone(),
            left_size: here.as_ref().map(Vec::len),
            right_size: there.as_ref().map(Vec::len),
            differing_bytes: bytes.differing,
            first_byte: bytes.first(),
        });
    }
    (differ, same)
}

/// How a file differs, after its path in a list.
pub(crate) fn output_note(output: &OutputDiff) -> String {
    match (output.left_size, output.right_size) {
        (Some(_), None) => format!("only in {}", SIDES.a),
        (None, Some(_)) => format!("only in {}", SIDES.b),
        (left, right) => format!(
            "{} of {} bytes differ, first at byte {}",
            output.differing_bytes,
            left.max(right).unwrap_or(0),
            output.first_byte.unwrap_or(0)
        ),
    }
}

/// The summary and excerpt `rewind diff --file` prints for `name`, the
/// left run's copy `a` and the right run's `b`.
pub(crate) fn file_report(name: &str, a: &[u8], b: &[u8]) -> Vec<String> {
    let diff = file_diff::compare(a, b);
    let mut report = file_diff::summary(name, a, b, &diff, SIDES);
    match &diff {
        FileDiff::Same => {}
        FileDiff::Text(lines) => {
            report.push(String::new());
            report.push(TEXT_LEGEND.to_string());
            report.extend(text_excerpt(
                &file_diff::lines_of(a),
                &file_diff::lines_of(b),
                lines,
            ));
        }
        FileDiff::Binary(bytes) => {
            report.push(String::new());
            report.push(BYTES_LEGEND.to_string());
            report.extend(byte_excerpt(a, b, bytes));
        }
    }
    report
}

/// `rewind diff --file`: file `path` as each run had it when its job
/// exited, compared.
pub(crate) fn diff_file(
    home: &Home,
    left: &Run,
    right: &Run,
    path: &str,
    format: Format,
) -> Result<ExitCode> {
    let (left_step, right_step) = (end_step(left)?, end_step(right)?);
    let here = read_file(home, left, left_step, path)?;
    let there = read_file(home, right, right_step, path)?;
    let name = path.rsplit('/').next().unwrap_or(path);

    // The report: how the copies differ, or which run has the file, and
    // a note when they were too long to compare whole.
    let (report, same) = match (&here, &there) {
        (None, None) => bail!(
            "neither run has {path} when its job exits; `rewind events` lists the files each wrote"
        ),
        (Some(_), None) => (vec![file_diff::only_in(name, SIDES.a)], false),
        (None, Some(_)) => (vec![file_diff::only_in(name, SIDES.b)], false),
        (Some(a), Some(b)) => {
            let cut = a.len() > file_diff::MAX_COMPARED || b.len() > file_diff::MAX_COMPARED;
            let (a, b) = (compared(a), compared(b));
            let mut report = file_report(name, a, b);
            if cut {
                report.insert(1, file_diff::cut_note());
            }
            (report, a == b)
        }
    };
    match format {
        Format::Text => {
            for line in &report {
                println!("{line}");
            }
        }
        Format::Json => println!(
            "{}",
            serde_json::json!({
                "left": left.manifest.id,
                "right": right.manifest.id,
                "path": path,
                "left_step": left_step,
                "right_step": right_step,
                "left_size": here.as_ref().map(Vec::len),
                "right_size": there.as_ref().map(Vec::len),
                "same": same,
                "summary": report,
            })
        ),
    }
    Ok(if same {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The part of a copy that is compared.
fn compared(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.len().min(file_diff::MAX_COMPARED)]
}

/// File `path` at `step` of `run`, or None when it was not there.
fn read_file(home: &Home, run: &Run, step: u64, path: &str) -> Result<Option<Vec<u8>>> {
    match inspect::cat(home, run, step, None, path)? {
        Inspection::Contents(bytes) => Ok(Some(bytes)),
        Inspection::NotFound(_) => Ok(None),
        Inspection::Failed(message) => bail!(
            "reading {path} at step {step} of run {}: {message}",
            run.manifest.id
        ),
    }
}

/// `rewind diff --outputs`: the files either run's job wrote and left
/// behind, as each run had them when its job exited, and those that
/// differ.
pub(crate) fn diff_outputs(
    home: &Home,
    left: &Run,
    right: &Run,
    format: Format,
) -> Result<ExitCode> {
    // Every file either job left behind, the left run's in the order it
    // wrote them, then the ones only the right run wrote.
    let every = Glob::new("**");
    let mut paths = written_files(left.trace()?, &every);
    for path in written_files(right.trace()?, &every) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    // Each run's files, read at once at its job's exit.
    let (left_step, right_step) = (end_step(left)?, end_step(right)?);
    let here = read_files(home, left, left_step, &paths)?;
    let there = read_files(home, right, right_step, &paths)?;
    let (here, there) = (sections(&here, left)?, sections(&there, right)?);
    let (differ, same) = differing_outputs(&paths, &here, &there);

    match format {
        Format::Text => {
            for line in outputs_report(&differ, same) {
                println!("{line}");
            }
        }
        Format::Json => println!(
            "{}",
            serde_json::json!({
                "left": left.manifest.id,
                "right": right.manifest.id,
                "left_step": left_step,
                "right_step": right_step,
                "same": same,
                "differ": differ,
            })
        ),
    }
    Ok(if differ.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Files `paths` at `step` of `run`, in sections named by their paths.
fn read_files(home: &Home, run: &Run, step: u64, paths: &[String]) -> Result<Vec<u8>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    match inspect::files(home, run, step, None, paths)? {
        Inspection::Contents(bytes) => Ok(bytes),
        Inspection::NotFound(message) | Inspection::Failed(message) => bail!(
            "reading the files of run {} at step {step}: {message}",
            run.manifest.id
        ),
    }
}

/// The files in `answer`, the answer to reading files of `run`, by path.
fn sections<'a>(answer: &'a [u8], run: &Run) -> Result<Vec<(String, &'a [u8])>> {
    rewind_init::sections(answer)
        .with_context(|| format!("reading the files of run {}", run.manifest.id))
}

/// What `rewind diff --outputs` prints: how many files differ, then one
/// line for each.
pub(crate) fn outputs_report(differ: &[OutputDiff], same: usize) -> Vec<String> {
    let total = differ.len() + same;
    if differ.is_empty() {
        return vec![format!(
            "None of the {total} files the runs left behind differ."
        )];
    }
    let mut report = vec![format!(
        "{} of the {total} files the runs left behind differ:",
        differ.len()
    )];
    report.extend(
        differ
            .iter()
            .map(|d| format!("  {}: {}", d.path, output_note(d))),
    );
    report
}

/// Whether a line of a diff is one only one copy has.
fn changed(line: &Line) -> bool {
    !matches!(line, Line::Both { .. })
}

#[cfg(test)]
mod tests {
    // The excerpts and lists `rewind diff` prints, from small copies
    // written out in each test, checked line by line.
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    /// A changed line in a long file: three lines of context, the line
    /// taken out and the one put in, then the lines after, up to the
    /// excerpt's length.
    #[test]
    fn a_text_excerpt_starts_just_before_the_first_difference() {
        let a: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        let mut b = a.clone();
        b[9] = "line ten".to_string();
        let diff = file_diff::line_diff(&a, &b);
        let excerpt = text_excerpt(&a, &b, &diff);
        assert_eq!(excerpt.len(), EXCERPT_LINES);
        assert_eq!(
            excerpt[..6],
            [
                "  line 7",
                "  line 8",
                "  line 9",
                "- line 10",
                "+ line ten",
                "  line 11"
            ]
        );
        assert_eq!(excerpt.last().map(String::as_str), Some("  line 21"));

        // A difference on the first line has no context before it.
        let (a, b) = (lines("x\ny"), lines("z\ny"));
        let diff = file_diff::line_diff(&a, &b);
        assert_eq!(text_excerpt(&a, &b, &diff), ["- x", "+ z", "  y"]);
    }

    /// Two copies 40 bytes long that differ at bytes 18, 19 and 33: the
    /// excerpt starts at the line holding byte 18, shows each run's
    /// bytes, and marks the three under the right run's.
    #[test]
    fn a_byte_excerpt_marks_the_bytes_that_differ() {
        let a: Vec<u8> = (0u8..40).collect();
        let mut b = a.clone();
        b[18] = 0xff;
        b[19] = 0xfe;
        b[33] = b'A';
        let diff = file_diff::byte_diff(&a, &b);
        let excerpt = byte_excerpt(&a, &b, &diff);
        assert_eq!(
            excerpt,
            [
                "left   00000010  10 11 12 13 14 15 16 17  18 19 1a 1b 1c 1d 1e 1f  ................",
                "right  00000010  10 11 ff fe 14 15 16 17  18 19 1a 1b 1c 1d 1e 1f  ................",
                "                       ^^ ^^",
                "left   00000020  20 21 22 23 24 25 26 27                            !\"#$%&'",
                "right  00000020  20 41 22 23 24 25 26 27                            A\"#$%&'",
                "                    ^^",
            ]
        );
    }

    /// A file the same in both runs, one whose bytes differ, one only the
    /// left run left behind, and one neither has any more.
    #[test]
    fn outputs_that_differ_are_listed_in_order() {
        let paths = ["/out/same", "/out/pc", "/out/only-left", "/out/gone"].map(String::from);
        let left: Vec<(String, &[u8])> = vec![
            ("/out/same".into(), b"same"),
            ("/out/pc".into(), b"prefix=/nix/store/1a"),
            ("/out/only-left".into(), b"x"),
        ];
        let right: Vec<(String, &[u8])> = vec![
            ("/out/pc".into(), b"prefix=/nix/store/9z"),
            ("/out/same".into(), b"same"),
        ];
        let (differ, same) = differing_outputs(&paths, &left, &right);
        assert_eq!(same, 1);
        assert_eq!(
            differ,
            vec![
                OutputDiff {
                    path: "/out/pc".into(),
                    left_size: Some(20),
                    right_size: Some(20),
                    differing_bytes: 2,
                    first_byte: Some(18),
                },
                OutputDiff {
                    path: "/out/only-left".into(),
                    left_size: Some(1),
                    right_size: None,
                    differing_bytes: 1,
                    first_byte: Some(0),
                },
            ]
        );
        assert_eq!(
            output_note(&differ[0]),
            "2 of 20 bytes differ, first at byte 18"
        );
        assert_eq!(output_note(&differ[1]), "only in the left run");
    }

    /// The list's first line leads with how many files differ.
    #[test]
    fn the_outputs_report_counts_first() {
        let pc = OutputDiff {
            path: "/out/lib/pkgconfig/neatvnc.pc".into(),
            left_size: Some(407),
            right_size: Some(407),
            differing_bytes: 16,
            first_byte: Some(216),
        };
        assert_eq!(
            outputs_report(&[pc], 11),
            [
                "1 of the 12 files the runs left behind differ:",
                "  /out/lib/pkgconfig/neatvnc.pc: 16 of 407 bytes differ, first at byte 216",
            ]
        );
        assert_eq!(
            outputs_report(&[], 3),
            ["None of the 3 files the runs left behind differ."]
        );
    }

    /// The whole report for a text file: the summary, a blank line, the
    /// legend, then the excerpt.
    #[test]
    fn a_file_report_is_the_summary_then_the_excerpt() {
        let report = file_report("test.log", b"PASS a\nPASS b\n", b"PASS a\nFAIL b\n");
        assert_eq!(
            report,
            [
                "test.log differs, first at line 2.",
                "Only in the left run: 1 line.",
                "Only in the right run: 1 line.",
                "",
                TEXT_LEGEND,
                "  PASS a",
                "- PASS b",
                "+ FAIL b",
            ]
        );
    }
}
