//! Two versions of a file, compared as the app shows a file's change
//! between two runs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rewind_trace::file_diff::{self, Sides};

fuzz_target!(|sides: (&[u8], &[u8])| {
    let (a, b) = sides;
    let diff = file_diff::compare(a, b);
    let _ = file_diff::summary("file", a, b, &diff, Sides { a: "a", b: "b" });
    let _ = file_diff::byte_diff(a, b);
});
