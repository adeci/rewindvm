//! The block a view shows while the engine brings runs back to a step:
//! the view's own sentence, and for a long replay its line and a bar, a
//! note that the next look nearby is quicker, and a Cancel button (see
//! [`crate::waiting`]).

use gpui::{App, ClickEvent, Div, Window, div, prelude::*, px, relative, rgb};

use crate::theme::{self, size};
use crate::ui::widgets::{Availability, ButtonStyle, button};
use crate::waiting::{QUICKER_NEXT_TIME, Row};

/// The Cancel button's label.
const CANCEL: &str = "Cancel";

/// `heading`, when there is one, and under it `rows`, each a line and a
/// bar, then the note and a Cancel button that runs `cancel`. With no
/// rows, only `heading`.
pub(super) fn waiting_block(
    id: &'static str,
    heading: Option<String>,
    rows: Vec<Row>,
    cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let block = div()
        .flex()
        .flex_col()
        .gap(px(size::CARD_GAP))
        .text_size(px(size::TEXT_UI))
        .text_color(rgb(theme::MUTED))
        .when_some(heading, |block, heading| block.child(heading));
    if rows.is_empty() {
        return block;
    }

    // A line and a bar for each long replay.
    let block = rows.into_iter().fold(block, |block, row| {
        block.child(
            div()
                .flex()
                .flex_col()
                .gap(px(size::CARD_GAP / 2.0))
                .child(div().text_color(rgb(theme::SOFT)).child(row.text))
                .child(bar(row.fraction)),
        )
    });

    // The note, and the way out.
    block
        .child(
            div()
                .text_size(px(size::TEXT_SMALL))
                .child(QUICKER_NEXT_TIME),
        )
        .child(
            div().flex().child(
                button(id, ButtonStyle::Neutral, Availability::Enabled)
                    .child(CANCEL)
                    .on_click(cancel),
            ),
        )
}

/// A bar `fraction` of the way full.
fn bar(fraction: f32) -> Div {
    div()
        .w_full()
        .h(px(size::PROGRESS_BAR_HEIGHT))
        .rounded(px(size::RADIUS_PROGRESS_BAR))
        .bg(rgb(theme::LINE_2))
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded(px(size::RADIUS_PROGRESS_BAR))
                .bg(rgb(theme::AMBER)),
        )
}
