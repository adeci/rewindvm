//! The guest kernel's answer to a `running` inspection: a pid, a process's
//! map and the files only the VM has.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rewind_core::maps::Running::parse(data);
});
