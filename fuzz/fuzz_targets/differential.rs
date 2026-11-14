#![no_main]
use libfuzzer_sys::fuzz_target;

// If `verified` accepts, verified view == checked reader (see `graven_fuzz::differential`).
fuzz_target!(|data: &[u8]| graven_fuzz::differential(data));
