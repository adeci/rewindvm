//! An ELF file a process maps, as gdb's helpers read it: where it loads,
//! its dynamic section, the gdb scripts it names and its build ID.
#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use rewind_core::maps;

/// A load address with high bits set, so an offset computed from it can
/// overflow.
const BASE: u64 = 0x7f00_0000_0000;

fuzz_target!(|data: &[u8]| {
    let _ = maps::load_offset(&mut Cursor::new(data), BASE);
    let _ = maps::dynamic_vaddr(&mut Cursor::new(data));
    let _ = maps::gdb_scripts(&mut Cursor::new(data));
    let _ = maps::build_id(&mut Cursor::new(data));
});
