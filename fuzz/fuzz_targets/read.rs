#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| graven_fuzz::read_all(data));
