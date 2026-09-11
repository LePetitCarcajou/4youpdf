#![no_main]
use libfuzzer_sys::fuzz_target;

// The contract under test: any byte sequence yields Ok or Err, never a panic.
fuzz_target!(|data: &[u8]| {
    let _ = fyp_core::parser::Parser::new(data).parse_object();
    let _ = fyp_core::parser::Parser::new(data).parse_indirect();
});
