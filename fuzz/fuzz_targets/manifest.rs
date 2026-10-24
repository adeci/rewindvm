//! A run's manifest, read by its version first.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rewind_trace::manifest::Manifest::parse(data);
});
