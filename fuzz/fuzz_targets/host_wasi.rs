//! The 14 WASI functions the sandbox provides, called with hostile
//! pointers, lengths and counts, checked against a reference model
//! (`fyp_host::fuzzing`). A panic or a divergence from the model is a crash.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Err(divergence) = fyp_host::fuzzing::check(data) {
        panic!("{divergence}");
    }
});
