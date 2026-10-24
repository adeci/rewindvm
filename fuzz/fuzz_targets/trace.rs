//! A trace file, as the app and the browser viewer read one: its records,
//! the events among them, and the read and write records.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(records) = rewind_trace::records_from(data) else {
        return;
    };
    let _ = rewind_trace::Trace::decode(&records);
    let _ = rewind_trace::io_records::decode(&records);
});
