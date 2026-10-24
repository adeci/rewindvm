//! One record as the guest writes it, without a trace file's framing
//! around it: as an event, and as read and write records.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rewind_trace::Event::decode(0, data);
    let _ = rewind_trace::io_records::decode(&[(0, data.to_vec())]);
});
