//! The standard filters (`fyp_core::filters`) on arbitrary bytes, apart
//! from any document: the chain of a stream dictionary written at the front
//! of the input, then each decoder and the predictors called directly with
//! hostile parameters. Any byte sequence yields Ok or Err, never a panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

use fyp_core::filters::{self, DecodeLimits, Predictor};
use fyp_core::object::Object;
use fyp_core::parser::Parser;

/// Below the 256 MiB of the product: same check, bomb caught sooner.
const LIMITS: DecodeLimits = DecodeLimits {
    max_output: 4 << 20,
};

fuzz_target!(|data: &[u8]| {
    // A stream dictionary at the front, its data after it: `/Filter` and
    // `/DecodeParms` as read from a file, names, arrays and abbreviations
    // included; a reference in them stays what it is (the document layer
    // is not here).
    let mut parser = Parser::new(data);
    match parser.parse_object() {
        Ok(Object::Dict(dict)) => {
            let rest = data.get(parser.pos()..).unwrap_or_default();
            let _ = filters::decode_stream_with(&dict, rest, |o| Ok(o.clone()), LIMITS);
        }
        Ok(Object::Stream { dict, data }) => {
            let _ = filters::decode_stream_with(&dict, &data, |o| Ok(o.clone()), LIMITS);
        }
        _ => {}
    }
    // Each decoder directly, the first byte choosing which; the predictors
    // take their parameters from the next six bytes, out of range included.
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    match selector % 6 {
        0 => {
            let _ = filters::flate_decode(rest, LIMITS);
        }
        1 => {
            let _ = filters::ascii_hex_decode(rest);
        }
        2 => {
            let _ = filters::ascii85_decode(rest);
        }
        3 => {
            let _ = filters::run_length_decode(rest, LIMITS);
        }
        _ => {
            let Some((parms, rest)) = rest.split_first_chunk::<6>() else {
                return;
            };
            let p = Predictor {
                predictor: parms[0],
                colors: usize::from(u16::from_le_bytes([parms[1], parms[2]])),
                bits_per_component: usize::from(parms[3]),
                columns: usize::from(u16::from_le_bytes([parms[4], parms[5]])),
            };
            let _ = filters::apply_predictor(rest, p);
        }
    }
});
