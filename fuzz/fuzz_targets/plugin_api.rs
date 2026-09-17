//! What the host reads from a module without trusting it, and what a module
//! reads from the host (`fyp-plugin-api`): a `manifest.toml`, and the two
//! messages of the exchange, decoded then encoded again. Any byte sequence
//! yields Ok or Err, never a panic. Seeded from plugins/merge.
#![no_main]
use libfuzzer_sys::fuzz_target;

use fyp_plugin_api::exchange::{Request, Response};
use fyp_plugin_api::Manifest;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(manifest) = Manifest::from_toml(text) {
            let _ = manifest.validate(false);
            let _ = manifest.validate(true);
        }
    }
    if let Ok(request) = Request::decode(data) {
        let _ = request.encode();
    }
    if let Ok(response) = Response::decode(data) {
        let _ = response.encode();
    }
    let _ = Response::decode_owned(data.to_vec());
});
