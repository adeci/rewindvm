//! The rows of the two-run file view: one file as this run and the
//! compared run had it, side by side or one under the other, with what
//! differs marked. Text is matched line by line and the characters that
//! differ within a changed line are marked; anything else is a hex dump
//! of each copy, the bytes that differ marked. The matching itself is
//! `rewind_trace::file_diff`.
//!
//! Each byte that differs, and each changed line, stands for a byte of
//! its copy: a run with read and write records names the step whose
//! write set that byte, which `rewind_trace::writes` finds, and the view
//! offers to go there.

use std::ops::Range;

use rewind_trace::file_diff::{self, FileDiff, Kind, Row as Paired, Sides};
use rewind_trace::writes::ByteWrite;

use crate::compare::differing;
use crate::describe::thousands;
use crate::sideways::widest_line;
use crate::viewer::{FetchedAll, HEX_WIDTH, hex_line, hex_spans};

/// What the view's summary calls the two runs.
pub const SIDES: Sides<'static> = Sides {
    a: "this run",
    b: "the other run",
};

/// The steps two runs' jobs exited at, or their last steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ends {
    pub this: u64,
    pub other: u64,
}

/// Where two runs part: this run's step and the other run's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parted {
    pub this: u64,
    pub other: u64,
}

/// The step of the other run whose files the view compares with this
/// run's at `step`. Before the runs part, the step `matched` pairs with
/// it by the events they share; from where they part, as many steps past
/// the other run's parting step as `step` is past this run's; and from
/// this run's end, the other run's end. Never past the other run's end.
pub fn other_step(step: u64, ends: Ends, parted: Option<Parted>, matched: Option<u64>) -> u64 {
    if step >= ends.this {
        return ends.other;
    }
    let other = match parted {
        Some(p) if step >= p.this => p.other + (step - p.this),
        _ => matched.unwrap_or(step),
    };
    other.min(ends.other)
}

/// What the summary says when neither run has file `{name}`.
pub const NEITHER: &str = "Neither run has {name} at these steps.";

/// One run's copy of the file, as the engine read it.
#[derive(Clone, Copy, Debug)]
pub struct Copy<'a> {
    pub bytes: &'a [u8],
    /// Whether the engine read all of it, or stopped at the most the
    /// viewer shows.
    pub complete: FetchedAll,
}

/// The two copies compared: what the summary says, and the rows, unless
/// neither run has the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compared {
    pub summary: Vec<String>,
    pub laid: Option<Laid>,
}

/// File `name` as this run had it, `this`, and as the other run had it,
/// `other`, compared and laid out as `layout` says; None for a run that
/// did not have the file. A copy only one run has is laid out against
/// nothing, all of it that run's.
pub fn compare_copies(
    name: &str,
    this: Option<Copy>,
    other: Option<Copy>,
    layout: Layout,
) -> Compared {
    let (here, there) = match (this, other) {
        (None, None) => {
            return Compared {
                summary: vec![NEITHER.replace("{name}", name)],
                laid: None,
            };
        }
        (Some(here), Some(there)) => (here, there),
        (Some(here), None) => {
            let laid = lay_out(
                here.bytes,
                &[],
                &file_diff::compare(here.bytes, &[]),
                layout,
            );
            return Compared {
                summary: vec![file_diff::only_in(name, SIDES.a)],
                laid: Some(laid),
            };
        }
        (None, Some(there)) => {
            let laid = lay_out(
                &[],
                there.bytes,
                &file_diff::compare(&[], there.bytes),
                layout,
            );
            return Compared {
                summary: vec![file_diff::only_in(name, SIDES.b)],
                laid: Some(laid),
            };
        }
    };

    // Both runs have it: the summary of how it differs, and after its
    // first line, a note when either copy was cut short.
    let diff = file_diff::compare(here.bytes, there.bytes);
    let mut summary = file_diff::summary(name, here.bytes, there.bytes, &diff, SIDES);
    let cut = here.complete == FetchedAll::No || there.complete == FetchedAll::No;
    if cut {
        summary.insert(1, file_diff::cut_note());
    }
    Compared {
        summary,
        laid: Some(lay_out(here.bytes, there.bytes, &diff, layout)),
    }
}

/// How the two copies are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// This run's copy on the left, the other run's on the right, a line
    /// of each per row.
    SideBySide,
    /// One column: the lines both have once, and where the copies differ,
    /// this run's lines, then the other run's.
    Interleaved,
}

/// Which run's copy a cell shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// A line or bytes both copies have.
    Both,
    /// This run's, where it differs from the other's.
    This,
    /// The other run's, where it differs from this one's.
    Other,
}

/// Which of the two runs a copy is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    This,
    Other,
}

/// A part of a cell's text that stands for one byte of its copy: a
/// differing byte's hex pair or character in a hex dump, or a whole
/// changed line, which stands for the first byte of it that differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The part of the cell's text, by byte range.
    pub range: Range<usize>,
    /// The byte of the copy, by its offset from the start of the file.
    pub offset: usize,
}

/// One copy's line, or line of a hex dump, in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The line as the file has it, or the hex dump line.
    pub text: String,
    /// The line's number in its copy, from 1; None in a hex dump.
    pub number: Option<usize>,
    pub whose: Whose,
    /// The byte ranges of `text` that differ from the other copy's line in
    /// the same place: characters within a changed line, or a hex dump's
    /// bytes, in both their columns.
    pub marked: Vec<Range<usize>>,
    /// The bytes a hex dump line shows, for coloring them; empty for text.
    pub bytes: Vec<u8>,
    /// The parts of the text that stand for a byte that differs; none in
    /// a cell both copies share.
    pub targets: Vec<Target>,
}

/// A row of the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Side by side: this run's cell and the other run's; a copy with
    /// nothing in this place has none.
    Pair(Option<Cell>, Option<Cell>),
    /// One under the other: one cell.
    One(Cell),
}

impl Row {
    /// Whether the row shows something the copies do not share.
    pub fn differs(&self) -> bool {
        let differs = |cell: &Option<Cell>| cell.as_ref().is_some_and(|c| c.whose != Whose::Both);
        match self {
            Row::Pair(this, other) => differs(this) || differs(other),
            Row::One(cell) => cell.whose != Whose::Both,
        }
    }
}

/// The two copies laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Laid {
    pub kind: Kind,
    pub rows: Vec<Row>,
    /// The first row that differs, to scroll to.
    pub first: Option<usize>,
    /// The characters in the widest cell, tabs drawn as spaces, for
    /// scrolling sideways.
    pub widest: usize,
}

/// This run's copy `this` and the other run's `other`, compared as `diff`
/// says, laid out as `layout` says.
pub fn lay_out(this: &[u8], other: &[u8], diff: &FileDiff, layout: Layout) -> Laid {
    // Copies with the same bytes are laid out by what their bytes read
    // as, with nothing marked.
    let kind = match diff {
        FileDiff::Text(_) => Kind::Text,
        FileDiff::Binary(_) => Kind::Binary,
        FileDiff::Same => file_diff::kind_of(this),
    };
    let rows = match (kind, diff) {
        (Kind::Text, FileDiff::Text(lines)) => text_rows(this, other, &lines.lines, layout),
        (Kind::Text, _) => {
            let same =
                file_diff::line_diff(&file_diff::lines_of(this), &file_diff::lines_of(other));
            text_rows(this, other, &same.lines, layout)
        }
        (Kind::Binary, _) => byte_rows(this, other, layout),
    };

    // Where to scroll to first, and how far the rows scroll sideways.
    let first = rows.iter().position(Row::differs);
    let widest = rows
        .iter()
        .flat_map(|row| match row {
            Row::Pair(this, other) => [this.as_ref(), other.as_ref()],
            Row::One(cell) => [Some(cell), None],
        })
        .flatten()
        .map(|cell| widest_line(std::slice::from_ref(&cell.text)))
        .max()
        .unwrap_or(0);
    Laid {
        kind,
        rows,
        first,
        widest,
    }
}

/// The rows of two text copies whose lines `lines` matches.
fn text_rows(this: &[u8], other: &[u8], lines: &[file_diff::Line], layout: Layout) -> Vec<Row> {
    let (a, b) = (file_diff::lines_of(this), file_diff::lines_of(other));
    let (starts_a, starts_b) = (file_diff::line_starts(this), file_diff::line_starts(other));

    // A line's cell. A line that differs stands for the byte of its copy
    // at `column`, where it starts to differ.
    let cell = |text: &str, index: usize, whose: Whose, marked: Vec<Range<usize>>, column| {
        let targets = match whose {
            Whose::Both => Vec::new(),
            Whose::This => vec![Target {
                range: 0..text.len(),
                offset: file_diff::byte_in_line(this, &starts_a, index, column),
            }],
            Whose::Other => vec![Target {
                range: 0..text.len(),
                offset: file_diff::byte_in_line(other, &starts_b, index, column),
            }],
        };
        Cell {
            text: text.to_string(),
            number: Some(index + 1),
            whose,
            marked,
            bytes: Vec::new(),
            targets,
        }
    };

    // Each copy's cell of a side-by-side row: a changed line with the
    // characters that differ from the other's marked, and a line only one
    // copy has differing from its start.
    let cells = |row: Paired| -> (Option<Cell>, Option<Cell>) {
        const LINE_START: usize = 0;
        match row {
            Paired::Same { a: i, b: j } => (
                Some(cell(&a[i], i, Whose::Both, Vec::new(), LINE_START)),
                Some(cell(&b[j], j, Whose::Both, Vec::new(), LINE_START)),
            ),
            Paired::Changed { a: i, b: j } => {
                let (here, there) = differing(&a[i], &b[j]).unwrap_or_default();
                let (here_at, there_at) = (here.start, there.start);
                (
                    Some(cell(&a[i], i, Whose::This, non_empty(here), here_at)),
                    Some(cell(&b[j], j, Whose::Other, non_empty(there), there_at)),
                )
            }
            Paired::OnlyA(i) => (
                Some(cell(&a[i], i, Whose::This, Vec::new(), LINE_START)),
                None,
            ),
            Paired::OnlyB(j) => (
                None,
                Some(cell(&b[j], j, Whose::Other, Vec::new(), LINE_START)),
            ),
        }
    };
    let paired = file_diff::side_by_side(lines);
    if layout == Layout::SideBySide {
        return paired
            .into_iter()
            .map(|row| {
                let (here, there) = cells(row);
                Row::Pair(here, there)
            })
            .collect();
    }

    // One under the other: each stretch where the copies differ is this
    // run's cells, then the other run's.
    let mut rows = Vec::with_capacity(paired.len());
    let mut stretch: Vec<(Option<Cell>, Option<Cell>)> = Vec::new();
    let flush = |rows: &mut Vec<Row>, stretch: &mut Vec<(Option<Cell>, Option<Cell>)>| {
        let (here, there): (Vec<_>, Vec<_>) = stretch.drain(..).unzip();
        rows.extend(here.into_iter().flatten().map(Row::One));
        rows.extend(there.into_iter().flatten().map(Row::One));
    };
    for row in paired {
        let (here, there) = cells(row);
        if let Paired::Same { .. } = row {
            flush(&mut rows, &mut stretch);
            rows.extend(here.map(Row::One));
            continue;
        }
        stretch.push((here, there));
    }
    flush(&mut rows, &mut stretch);
    rows
}

/// A range in a list of marked ranges, unless it marks nothing: the
/// shorter of two lines where one only adds to the other.
fn non_empty(range: Range<usize>) -> Vec<Range<usize>> {
    if range.is_empty() {
        return Vec::new();
    }
    vec![range]
}

/// The rows of two copies as hex dumps, a line of `HEX_WIDTH` bytes of
/// each per place.
fn byte_rows(this: &[u8], other: &[u8], layout: Layout) -> Vec<Row> {
    let places = this.len().max(other.len()).div_ceil(HEX_WIDTH);
    let chunk = |bytes: &[u8], offset: usize| -> Vec<u8> {
        bytes[offset.min(bytes.len())..(offset + HEX_WIDTH).min(bytes.len())].to_vec()
    };
    let mut rows = Vec::with_capacity(places);
    for place in 0..places {
        let offset = place * HEX_WIDTH;
        let (here, there) = (chunk(this, offset), chunk(other, offset));
        let same = here == there;

        // Each copy's line, its bytes that differ from the other's marked
        // in the hex and in the characters; a copy that ended before this
        // place has none.
        let cell = |bytes: &[u8], against: &[u8], whose: Whose| -> Option<Cell> {
            if bytes.is_empty() {
                return None;
            }

            // Each differing byte's hex pair and character, each standing
            // for the byte.
            let spans = hex_spans(bytes);
            let targets: Vec<Target> = (0..bytes.len())
                .filter(|&i| bytes.get(i) != against.get(i))
                .flat_map(|i| {
                    [spans[1 + 2 * i].0.clone(), spans[2 + 2 * i].0.clone()].map(|range| Target {
                        range,
                        offset: offset + i,
                    })
                })
                .collect();
            let marked = targets.iter().map(|t| t.range.clone()).collect();
            Some(Cell {
                text: hex_line(offset, bytes),
                number: None,
                whose: if same { Whose::Both } else { whose },
                marked,
                bytes: bytes.to_vec(),
                targets: if same { Vec::new() } else { targets },
            })
        };
        let (here_cell, there_cell) = (
            cell(&here, &there, Whose::This),
            cell(&there, &here, Whose::Other),
        );
        match layout {
            Layout::SideBySide => rows.push(Row::Pair(here_cell, there_cell)),
            Layout::Interleaved if same => rows.extend(here_cell.map(Row::One)),
            Layout::Interleaved => {
                rows.extend(here_cell.map(Row::One));
                rows.extend(there_cell.map(Row::One));
            }
        }
    }
    rows
}

/// The offset of the first byte that differs in run `side`'s copy, as
/// the first row that differs shows it; None when that copy has no row
/// that differs.
pub fn first_offset(laid: &Laid, side: Side) -> Option<usize> {
    let whose = match side {
        Side::This => Whose::This,
        Side::Other => Whose::Other,
    };
    laid.rows
        .iter()
        .flat_map(|row| match row {
            Row::Pair(here, there) => [here.as_ref(), there.as_ref()],
            Row::One(cell) => [Some(cell), None],
        })
        .flatten()
        .filter(|cell| cell.whose == whose)
        .find_map(|cell| cell.targets.first())
        .map(|target| target.offset)
}

/// Where in a copy a note about a write is: a byte of a hex dump, or a
/// changed line by its number from 1, at the first byte of it that
/// differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Byte { offset: usize },
    Line { number: usize, offset: usize },
}

impl Place {
    /// The place a cell's target stands for.
    pub fn of(cell: &Cell, target: &Target) -> Place {
        match cell.number {
            Some(number) => Place::Line {
                number,
                offset: target.offset,
            },
            None => Place::Byte {
                offset: target.offset,
            },
        }
    }
}

/// What the view says about the write that set the byte at `place` in run
/// `side`'s copy, `write`, or that no recorded write did.
pub fn write_note(side: Side, place: Place, write: Option<&ByteWrite>) -> String {
    let (run, action) = match side {
        Side::This => ("this run", "Click to go there."),
        Side::Other => ("the other run", "Click to show that run there."),
    };

    // Where the byte is: a byte alone, or the line it starts to differ
    // at, then the byte by its number.
    let (lead, offset) = match place {
        Place::Byte { offset } => (None, offset),
        Place::Line { number, offset } => (
            Some(format!(
                "line {number} first differs at byte {}.",
                thousands(offset as u64)
            )),
            offset,
        ),
    };
    let byte = format!("byte {}", thousands(offset as u64));

    // The write, or that none set the byte, which names its run itself.
    let said = match write {
        Some(w) => format!(
            "{byte} was written at step {} by {} (pid {}). {action}",
            thousands(w.step),
            w.program,
            w.pid
        ),
        None => format!("No recorded write in {run} set {byte}."),
    };
    let names_run = write.is_none() && lead.is_none();
    let sentences = match lead {
        Some(lead) => format!("{lead} {}", capitalized(&said)),
        None => said,
    };

    // The other run's notes start by naming that run, unless the note
    // already does.
    match side {
        Side::Other if !names_run => format!("In the other run, {sentences}"),
        Side::This | Side::Other => capitalized(&sentences),
    }
}

/// `text` with its first letter a capital.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether a run recorded its writes to files, which the view needs to
/// say which step wrote a byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writes {
    Recorded,
    Unrecorded,
}

/// The line the view adds under its summary when either run did not
/// record its writes; None when both did.
pub fn unrecorded_note(this: Writes, other: Writes) -> Option<&'static str> {
    match (this, other) {
        (Writes::Recorded, Writes::Recorded) => None,
        (Writes::Unrecorded, Writes::Recorded) => Some(
            "This run did not record its file writes, so the step that wrote each byte is not known.",
        ),
        (Writes::Recorded, Writes::Unrecorded) => Some(
            "The other run did not record its file writes, so the step that wrote each of its bytes is not known.",
        ),
        (Writes::Unrecorded, Writes::Unrecorded) => Some(
            "Neither run recorded its file writes, so the step that wrote each byte is not known.",
        ),
    }
}

#[cfg(test)]
mod tests {
    // Small copies written out in each test, laid out both ways, and the
    // rows checked cell by cell: which run each cell is from, its number
    // and what in it is marked.
    use super::*;

    /// The text of a cell, its number, whose it is and the marked text.
    fn seen(cell: &Cell) -> (String, Option<usize>, Whose, Vec<String>) {
        let marked = cell
            .marked
            .iter()
            .map(|r| cell.text[r.clone()].to_string())
            .collect();
        (cell.text.clone(), cell.number, cell.whose, marked)
    }

    fn seen_pair(row: &Row) -> (Option<String>, Option<String>) {
        let Row::Pair(this, other) = row else {
            panic!("a side-by-side row: {row:?}");
        };
        (
            this.as_ref().map(|c| c.text.clone()),
            other.as_ref().map(|c| c.text.clone()),
        )
    }

    /// A line changed and one put in: side by side, the changed lines
    /// share a row with the digit that differs marked in each, and the
    /// new line has a row with nothing on this run's side.
    #[test]
    fn text_side_by_side_pairs_changed_lines_and_marks_what_differs() {
        let this = b"CC = cc\nCFLAGS = -O1\nall: pool\n";
        let other = b"CC = cc\nCFLAGS = -O2\nall: pool\nclean:\n";
        let diff = file_diff::compare(this, other);
        let laid = lay_out(this, other, &diff, Layout::SideBySide);
        assert_eq!(laid.kind, Kind::Text);
        assert_eq!(laid.rows.len(), 4);
        assert_eq!(laid.first, Some(1));
        let Row::Pair(Some(here), Some(there)) = &laid.rows[1] else {
            panic!("the changed lines share a row: {:?}", laid.rows[1]);
        };
        assert_eq!(
            seen(here),
            (
                "CFLAGS = -O1".into(),
                Some(2),
                Whose::This,
                vec!["1".into()]
            )
        );
        assert_eq!(
            seen(there),
            (
                "CFLAGS = -O2".into(),
                Some(2),
                Whose::Other,
                vec!["2".into()]
            )
        );
        assert_eq!(seen_pair(&laid.rows[3]), (None, Some("clean:".into())));
        assert!(!laid.rows[0].differs() && laid.rows[3].differs());
        assert_eq!(laid.widest, "CFLAGS = -O1".len());
    }

    /// The same copies one under the other: the shared lines once, then
    /// this run's line, then the other run's two.
    #[test]
    fn text_interleaved_puts_this_run_s_lines_first() {
        let this = b"CC = cc\nCFLAGS = -O1\nall: pool\n";
        let other = b"CC = cc\nCFLAGS = -O2\nclean:\nall: pool\n";
        let diff = file_diff::compare(this, other);
        let laid = lay_out(this, other, &diff, Layout::Interleaved);
        let cells: Vec<(String, Option<usize>, Whose, Vec<String>)> = laid
            .rows
            .iter()
            .map(|row| match row {
                Row::One(cell) => seen(cell),
                Row::Pair(..) => panic!("one cell a row: {row:?}"),
            })
            .collect();
        assert_eq!(
            cells,
            vec![
                ("CC = cc".into(), Some(1), Whose::Both, vec![]),
                (
                    "CFLAGS = -O1".into(),
                    Some(2),
                    Whose::This,
                    vec!["1".into()]
                ),
                (
                    "CFLAGS = -O2".into(),
                    Some(2),
                    Whose::Other,
                    vec!["2".into()]
                ),
                ("clean:".into(), Some(3), Whose::Other, vec![]),
                ("all: pool".into(), Some(3), Whose::Both, vec![]),
            ]
        );
        assert_eq!(laid.first, Some(1));
    }

    /// Two copies of 20 bytes that differ at bytes 9 and 10: side by side,
    /// the hex dump line holding them is this run's and the other's, the
    /// two hex pairs and characters marked; interleaved, the lines both
    /// have are one row, and that line is two.
    #[test]
    fn binary_copies_are_hex_dumps_with_the_differing_bytes_marked() {
        let this: Vec<u8> = (0u8..20).collect();
        let mut other = this.clone();
        other[9] = b'A';
        other[10] = b'B';
        let diff = file_diff::compare(&this, &other);

        let laid = lay_out(&this, &other, &diff, Layout::SideBySide);
        assert_eq!(laid.kind, Kind::Binary);
        assert_eq!(laid.rows.len(), 3);
        assert_eq!(laid.first, Some(1));
        let Row::Pair(Some(here), Some(there)) = &laid.rows[1] else {
            panic!("both copies have the second line: {:?}", laid.rows[1]);
        };
        assert_eq!(here.whose, Whose::This);
        assert_eq!(here.number, None);
        assert_eq!(here.bytes, this[8..16]);
        assert_eq!(
            seen(there).3,
            vec!["41".to_string(), "A".into(), "42".into(), "B".into()]
        );
        assert_eq!(seen(here).3, vec!["09", ".", "0a", "."]);
        assert!(!laid.rows[0].differs() && !laid.rows[2].differs());

        let laid = lay_out(&this, &other, &diff, Layout::Interleaved);
        let whose: Vec<Whose> = laid
            .rows
            .iter()
            .map(|row| match row {
                Row::One(cell) => cell.whose,
                Row::Pair(..) => panic!("one cell a row: {row:?}"),
            })
            .collect();
        assert_eq!(whose, [Whose::Both, Whose::This, Whose::Other, Whose::Both]);
    }

    /// A binary copy longer than the other: the lines past the shorter
    /// copy's end are only on the longer copy's side.
    #[test]
    fn bytes_past_the_shorter_copy_are_only_on_one_side() {
        let this = vec![0u8; 8];
        let other = vec![0u8; 12];
        let diff = file_diff::compare(&this, &other);
        let laid = lay_out(&this, &other, &diff, Layout::SideBySide);
        assert_eq!(laid.rows.len(), 2);
        let Row::Pair(None, Some(there)) = &laid.rows[1] else {
            panic!("only the other run has the second line: {:?}", laid.rows[1]);
        };
        assert_eq!(there.whose, Whose::Other);
        assert_eq!(there.marked.len(), 2 * 4);
    }

    /// What the summary says when a copy was cut short, when only one run
    /// has the file, and when neither has it; and the rows of a copy only
    /// one run has.
    #[test]
    fn copies_are_compared_with_a_plain_summary() {
        let whole = |bytes: &'static [u8]| Copy {
            bytes,
            complete: FetchedAll::Yes,
        };
        let cut = Copy {
            bytes: b"a\n",
            complete: FetchedAll::No,
        };
        let compared = compare_copies(
            "test.log",
            Some(cut),
            Some(whole(b"b\n")),
            Layout::SideBySide,
        );
        assert_eq!(compared.summary[0], "test.log differs, first at line 1.");
        assert_eq!(compared.summary[1], file_diff::cut_note());

        let compared = compare_copies("core", None, Some(whole(b"x\ny\n")), Layout::SideBySide);
        assert_eq!(compared.summary, ["core is only in the other run."]);
        let laid = compared.laid.unwrap();
        assert_eq!(laid.rows.len(), 2);
        assert!(
            laid.rows
                .iter()
                .all(|r| matches!(r, Row::Pair(None, Some(c)) if c.whose == Whose::Other))
        );

        let compared = compare_copies("core", None, None, Layout::SideBySide);
        assert_eq!(compared.summary, [NEITHER.replace("{name}", "core")]);
        assert_eq!(compared.laid, None);

        let same = compare_copies(
            "pool.o",
            Some(whole(b"\0\x01")),
            Some(whole(b"\0\x01")),
            Layout::SideBySide,
        );
        assert_eq!(same.summary, ["pool.o is the same in both runs: 2 bytes."]);
    }

    /// Steps before the runs part go to the step the events pair them
    /// with; past it, the same distance past the other's parting step;
    /// this run's end goes to the other's, and nothing goes past it.
    #[test]
    fn the_other_run_s_step_follows_the_playhead() {
        let ends = Ends {
            this: 1_938,
            other: 1_900,
        };
        let parted = Some(Parted {
            this: 1_164,
            other: 1_160,
        });
        assert_eq!(other_step(500, ends, parted, Some(498)), 498);
        assert_eq!(other_step(1_200, ends, parted, Some(1_160)), 1_196);
        assert_eq!(other_step(1_938, ends, parted, Some(1_160)), 1_900);
        assert_eq!(other_step(1_942, ends, parted, Some(1_160)), 1_900);
        assert_eq!(other_step(1_920, ends, parted, Some(1_160)), 1_900);

        // Runs that never part, and a step no event pairs.
        assert_eq!(other_step(700, ends, None, Some(702)), 702);
        assert_eq!(other_step(3, ends, None, None), 3);
    }

    /// A changed line stands for its first byte that differs, by its
    /// offset in its copy; a line only one copy has stands for its first
    /// byte; a line both share stands for none. The first difference of
    /// each copy is the changed line's byte.
    #[test]
    fn a_changed_line_stands_for_its_first_differing_byte() {
        let this = b"CC = cc\nCFLAGS = -O1\nall: pool\n";
        let other = b"CC = cc\nCFLAGS = -O2\nall: pool\nclean:\n";
        let diff = file_diff::compare(this, other);
        let laid = lay_out(this, other, &diff, Layout::SideBySide);
        let Row::Pair(Some(here), Some(there)) = &laid.rows[1] else {
            panic!("the changed lines share a row: {:?}", laid.rows[1]);
        };
        let whole = |cell: &Cell| Target {
            range: 0..cell.text.len(),
            offset: "CC = cc\nCFLAGS = -O".len(),
        };
        assert_eq!(here.targets, [whole(here)]);
        assert_eq!(there.targets, [whole(there)]);
        let Row::Pair(None, Some(clean)) = &laid.rows[3] else {
            panic!("only the other run has clean: {:?}", laid.rows[3]);
        };
        assert_eq!(
            clean.targets[0].offset,
            "CC = cc\nCFLAGS = -O2\nall: pool\n".len()
        );
        let Row::Pair(Some(shared), _) = &laid.rows[0] else {
            panic!("both have the first line: {:?}", laid.rows[0]);
        };
        assert!(shared.targets.is_empty());
        assert_eq!(first_offset(&laid, Side::This), Some(19));
        assert_eq!(first_offset(&laid, Side::Other), Some(19));
    }

    /// Two binary copies that differ at bytes 9 and 10: the line holding
    /// them stands for byte 9 by its hex pair and its character, and byte
    /// 10 the same, in both copies; one under the other too.
    #[test]
    fn a_differing_byte_stands_for_itself_twice() {
        let this: Vec<u8> = (0u8..20).collect();
        let mut other = this.clone();
        other[9] = b'A';
        other[10] = b'B';
        let diff = file_diff::compare(&this, &other);
        for layout in [Layout::SideBySide, Layout::Interleaved] {
            let laid = lay_out(&this, &other, &diff, layout);
            let cells: Vec<&Cell> = laid
                .rows
                .iter()
                .flat_map(|row| match row {
                    Row::Pair(here, there) => [here.as_ref(), there.as_ref()],
                    Row::One(cell) => [Some(cell), None],
                })
                .flatten()
                .filter(|cell| cell.whose != Whose::Both)
                .collect();
            assert_eq!(cells.len(), 2);
            for cell in cells {
                let offsets: Vec<usize> = cell.targets.iter().map(|t| t.offset).collect();
                assert_eq!(offsets, [9, 9, 10, 10]);
                let parts: Vec<&str> = cell
                    .targets
                    .iter()
                    .map(|t| &cell.text[t.range.clone()])
                    .collect();
                assert_eq!(
                    parts,
                    cell.marked
                        .iter()
                        .map(|r| &cell.text[r.clone()])
                        .collect::<Vec<_>>()
                );
            }
            assert_eq!(first_offset(&laid, Side::This), Some(9));
        }
    }

    /// The notes a byte, a line and the other run's byte get, with a write
    /// and without one, and the note for runs that did not record their
    /// writes.
    #[test]
    fn a_write_is_named_in_plain_words() {
        let write = ByteWrite {
            step: 13_232,
            pid: 56,
            program: "meson setup".into(),
        };
        assert_eq!(
            write_note(Side::This, Place::Byte { offset: 216 }, Some(&write)),
            "Byte 216 was written at step 13,232 by meson setup (pid 56). Click to go there."
        );
        assert_eq!(
            write_note(Side::Other, Place::Byte { offset: 1_216 }, Some(&write)),
            "In the other run, byte 1,216 was written at step 13,232 by meson setup (pid 56). \
             Click to show that run there."
        );
        assert_eq!(
            write_note(
                Side::This,
                Place::Line {
                    number: 3,
                    offset: 41
                },
                Some(&write)
            ),
            "Line 3 first differs at byte 41. Byte 41 was written at step 13,232 by meson setup \
             (pid 56). Click to go there."
        );
        assert_eq!(
            write_note(Side::This, Place::Byte { offset: 7 }, None),
            "No recorded write in this run set byte 7."
        );
        assert_eq!(
            write_note(Side::Other, Place::Byte { offset: 7 }, None),
            "No recorded write in the other run set byte 7."
        );
        assert_eq!(
            write_note(
                Side::Other,
                Place::Line {
                    number: 2,
                    offset: 9
                },
                None
            ),
            "In the other run, line 2 first differs at byte 9. No recorded write in the other \
             run set byte 9."
        );
        assert_eq!(unrecorded_note(Writes::Recorded, Writes::Recorded), None);
        assert_eq!(
            unrecorded_note(Writes::Unrecorded, Writes::Recorded),
            Some(
                "This run did not record its file writes, so the step that wrote each byte is not known."
            )
        );
        assert_eq!(
            unrecorded_note(Writes::Recorded, Writes::Unrecorded),
            Some(
                "The other run did not record its file writes, so the step that wrote each of its bytes is not known."
            )
        );
        assert_eq!(
            unrecorded_note(Writes::Unrecorded, Writes::Unrecorded),
            Some(
                "Neither run recorded its file writes, so the step that wrote each byte is not known."
            )
        );
    }

    /// Copies with the same bytes are laid out with nothing marked.
    #[test]
    fn the_same_copies_mark_nothing() {
        let laid = lay_out(b"a\nb\n", b"a\nb\n", &FileDiff::Same, Layout::SideBySide);
        assert_eq!(laid.rows.len(), 2);
        assert_eq!(laid.first, None);
        assert!(laid.rows.iter().all(|r| !r.differs()));
    }
}
