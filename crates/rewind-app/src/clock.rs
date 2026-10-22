//! The wall clock and the monotonic clock. wasm32-unknown-unknown's std
//! has neither, and panics when asked, so the browser build reads the
//! browser's: `Date.now()` and `performance.now()`.

#[cfg(not(target_family = "wasm"))]
pub use std::time::Instant;
#[cfg(target_family = "wasm")]
pub use web_time::Instant;

/// The time now, as std's `SystemTime`, which the rest of the app keeps.
#[cfg(not(target_family = "wasm"))]
pub fn now() -> std::time::SystemTime {
    std::time::SystemTime::now()
}

/// The time now, from the browser's clock, as std's `SystemTime`.
#[cfg(target_family = "wasm")]
pub fn now() -> std::time::SystemTime {
    const MILLIS_PER_SECOND: f64 = 1000.0;
    let seconds = js_sys::Date::now() / MILLIS_PER_SECOND;
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs_f64(seconds)
}
