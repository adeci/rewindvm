//! A keyframe file: registers, devices and the pages it names.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rewind_core::keyframes::decode(data);
});
