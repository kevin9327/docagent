#![no_main]
// cargo-fuzz target. Also exercised by the unit test in docagent-hwp5
// (`garbage_bytes_are_rejected`) and the panic-freedom loop below is
// duplicated as a std test so CI runs without cargo-fuzz.

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = docagent_hwp5::read(data);
});
