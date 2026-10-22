//! How much of the toolbar under the timeline fits the window: the full
//! row with its words, or a compact one with shorter labels. The full row's
//! width is what it was last laid out at, less its labels that change,
//! plus those labels as they are now, shaped in their fonts.

use gpui::{Pixels, px};

use crate::describe::clip;

/// How many characters of the chosen stride and of the phase the toolbar
/// shows; the rest goes in a note.
pub const MAX_LABEL_CHARS: usize = 12;

/// Which toolbar the window has room for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarFit {
    /// Previous and Next with their names, "Stop at" before the stride,
    /// and the jump and fork buttons' whole names.
    Full,
    /// Previous and Next as chevrons, the stride alone, and short names
    /// on the jump and fork buttons; the full names are in their notes.
    Compact,
}

/// What the toolbar keeps between frames: the toolbar shown, and how
/// wide the full row is without its labels that change, as last laid out.
/// Until the full row has been laid out once, it is shown, to learn that.
#[derive(Clone, Copy, Debug)]
pub struct ToolbarMemory {
    pub shown: ToolbarFit,
    pub full_fixed: Option<Pixels>,
}

impl Default for ToolbarMemory {
    fn default() -> Self {
        ToolbarMemory {
            shown: ToolbarFit::Full,
            full_fixed: None,
        }
    }
}

/// How much room the full row must have to spare before a compact
/// toolbar turns full: about four of the step readout's digits, so the
/// toolbar does not flip as the step count gains a digit or the window is
/// dragged across the edge.
const SWITCH_TO_FULL_MARGIN: f32 = 32.0;

/// The toolbar to show next, given the one shown, how wide the full row
/// is with the labels on screen, and the window's width. A full toolbar
/// turns compact as soon as the full row stops fitting; a compact one
/// turns full only once the full row fits with `SWITCH_TO_FULL_MARGIN` to
/// spare.
pub fn next_fit(shown: ToolbarFit, full_row_width: Pixels, viewport_width: Pixels) -> ToolbarFit {
    let fits = full_row_width <= viewport_width;
    let fits_with_room = full_row_width + px(SWITCH_TO_FULL_MARGIN) <= viewport_width;
    match shown {
        ToolbarFit::Full if fits => ToolbarFit::Full,
        ToolbarFit::Compact if fits_with_room => ToolbarFit::Full,
        _ => ToolbarFit::Compact,
    }
}

/// A label as the toolbar shows it, cut to `MAX_LABEL_CHARS`.
#[derive(Debug, PartialEq, Eq)]
pub struct Clipped {
    pub shown: String,
    /// The whole label, when `shown` is cut short of it.
    pub whole: Option<String>,
}

/// `text` cut to `MAX_LABEL_CHARS`, ending in an ellipsis when cut.
pub fn clip_label(text: &str) -> Clipped {
    let shown = clip(text, MAX_LABEL_CHARS);
    let whole = (shown != text).then(|| text.to_string());
    Clipped { shown, whole }
}

#[cfg(test)]
mod tests {
    // Which toolbar a window gets, from its width and the full row's,
    // and how labels are cut.
    use super::*;

    /// A full row width for the tests below.
    const ROW: f32 = 1345.0;

    #[test]
    fn the_full_row_shows_when_it_fits_and_not_when_it_does_not() {
        // A row narrower than the window by more than the margin is full
        // from either toolbar; a row wider than the window is compact
        // from either.
        let wide = px(ROW + SWITCH_TO_FULL_MARGIN + 1.0);
        let narrow = px(ROW - 1.0);
        for shown in [ToolbarFit::Full, ToolbarFit::Compact] {
            assert_eq!(next_fit(shown, px(ROW), wide), ToolbarFit::Full);
            assert_eq!(next_fit(shown, px(ROW), narrow), ToolbarFit::Compact);
        }
    }

    #[test]
    fn inside_the_margin_the_toolbar_stays_as_it_is() {
        // A row that fits with less than the margin to spare, or exactly
        // fills the window, keeps whichever toolbar is shown.
        for viewport in [ROW, ROW + SWITCH_TO_FULL_MARGIN - 1.0] {
            for shown in [ToolbarFit::Full, ToolbarFit::Compact] {
                assert_eq!(next_fit(shown, px(ROW), px(viewport)), shown);
            }
        }

        // At the margin exactly, a compact toolbar turns full.
        assert_eq!(
            next_fit(
                ToolbarFit::Compact,
                px(ROW),
                px(ROW + SWITCH_TO_FULL_MARGIN)
            ),
            ToolbarFit::Full
        );
    }

    #[test]
    fn labels_are_cut_to_the_cap_and_keep_their_whole_text() {
        // A label at the cap is shown whole; a longer one is cut to the
        // cap with an ellipsis, and the whole text is kept for its note.
        assert_eq!(
            clip_label("installCheck"),
            Clipped {
                shown: "installCheck".to_string(),
                whole: None,
            }
        );
        assert_eq!(
            clip_label("process starts and exits"),
            Clipped {
                shown: "process sta\u{2026}".to_string(),
                whole: Some("process starts and exits".to_string()),
            }
        );
        assert_eq!(clip_label("").shown, "");
    }
}
