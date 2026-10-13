//! What a job asserted with the Rewind SDK, on the timeline: a red mark
//! above the track at each assertion that broke, which goes to its step
//! on a click and says what broke when pointed at, and under the track a
//! strip with a span per test the job marked, green where it passed and
//! red where it failed.

use gpui::{Context, Div, MouseButton, SharedString, Stateful, div, prelude::*, px, relative, rgb};
use rewind_trace::sdk::TestResult;

use crate::describe::thousands;
use crate::model::TestSpan;
use crate::theme::{self, size};
use crate::ui::scrubber::Scrubber;
use crate::ui::widgets::tooltip;
use crate::view::View;

/// An assertion's mark: a square a little larger than a bookmark's, in
/// the failure's red.
const MARK_SIZE: f32 = 8.0;

/// The test strip's height, its gap from the track, and the narrowest a
/// test's span is drawn.
const STRIP_HEIGHT: f32 = 6.0;
const STRIP_GAP: f32 = 4.0;
const SPAN_MIN_WIDTH: f32 = 2.0;

impl Scrubber {
    /// The marks of the assertions that broke that the track shows.
    pub(super) fn assertion_marks(&self, view: View, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        self.session()
            .run
            .timeline
            .assertions
            .iter()
            .enumerate()
            .filter(|(_, a)| view.contains(a.step))
            .map(|(i, a)| {
                let step = a.step;
                let note = format!("Step {}: {}", thousands(step), a.text);
                div()
                    .id(SharedString::from(format!("assertion-mark-{i}")))
                    .absolute()
                    .left(relative(view.fraction_of(step)))
                    .ml(px(-MARK_SIZE / 2.0))
                    .top(px(-size::MARKER_OVERHANG - MARK_SIZE))
                    .size(px(MARK_SIZE))
                    .rounded(px(2.0))
                    .bg(rgb(theme::RED))
                    .cursor_pointer()
                    .tooltip(tooltip(note))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_to(step, cx)))
            })
            .collect()
    }

    /// The strip of the run's tests under the track, when the job marked
    /// any: a span per test over the steps it ran, as wide as the track
    /// shows them.
    pub(super) fn render_test_strip(&self, view: View, cx: &mut Context<Self>) -> Option<Div> {
        let t = &self.session().run.timeline;
        if t.tests.is_empty() {
            return None;
        }
        let mut strip = div()
            .relative()
            .w_full()
            .h(px(STRIP_HEIGHT))
            .mt(px(STRIP_GAP));
        for (i, test) in t.tests.iter().enumerate() {
            let end = test.end.unwrap_or(t.total);
            if end < view.lo || test.begin > view.hi {
                continue;
            }
            let from = view.fraction_of(test.begin.max(view.lo));
            let to = view.fraction_of(end.min(view.hi));
            let begin = test.begin;
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("test-span-{i}")))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(from))
                    .w(relative((to - from).max(0.0)))
                    .min_w(px(SPAN_MIN_WIDTH))
                    .rounded(px(1.0))
                    .bg(rgb(test_color(test)))
                    .cursor_pointer()
                    .tooltip(tooltip(test_note(test)))
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_to(begin, cx))),
            );
        }
        Some(strip)
    }
}

/// A test's color: green passed, red failed, grey skipped or unfinished.
fn test_color(test: &TestSpan) -> u32 {
    match test.result {
        Some(TestResult::Pass) => theme::GREEN_SOFT,
        Some(TestResult::Fail) => theme::RED,
        Some(TestResult::Skip) | None => theme::FAINT,
    }
}

/// What a test's span says when pointed at.
fn test_note(test: &TestSpan) -> String {
    let how = match test.result {
        Some(TestResult::Pass) => "passed",
        Some(TestResult::Fail) => "failed",
        Some(TestResult::Skip) => "was skipped",
        None => "did not finish",
    };
    match test.end {
        Some(end) => format!(
            "Test {} {how}, steps {} to {}",
            test.name,
            thousands(test.begin),
            thousands(end)
        ),
        None => format!(
            "Test {} {how}; it began at step {}",
            test.name,
            thousands(test.begin)
        ),
    }
}
