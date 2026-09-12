//! For modules written in Rust: one call answers the host.
//!
//! A module is a WASI command built for `wasm32-wasip1`. Its `main` hands
//! a handler to [`serve`]:
//!
//! ```no_run
//! fn main() -> std::process::ExitCode {
//!     fyp_plugin_api::module::serve(|request| {
//!         // request.action, request.params, request.documents
//!         Err(format!("nothing to do for `{}`", request.action))
//!     })
//! }
//! ```
//!
//! No `#[no_mangle]` export and no `unsafe`: the exchange goes through the
//! standard input and output the host provides (see [`crate::exchange`]).

use std::io::{Read, Write};
use std::process::ExitCode;

use crate::exchange::{Request, Response};

/// Read the request on standard input, give it to `handler`, write its
/// answer on standard output: the document it returns, or its error
/// message.
///
/// Returns [`ExitCode::SUCCESS`] once an answer is written, an error
/// answer included: the answer says what happened.
/// [`ExitCode::FAILURE`] only when no answer could be written.
pub fn serve<F>(handler: F) -> ExitCode
where
    F: FnOnce(Request) -> Result<Vec<u8>, String>,
{
    let response = match read_request() {
        Ok(request) => match handler(request) {
            Ok(document) => Response::Document(document),
            Err(message) => Response::Error(message),
        },
        Err(message) => Response::Error(message),
    };
    let bytes = match response.encode() {
        Ok(bytes) => bytes,
        Err(e) => match Response::Error(format!("cannot encode the answer: {e}")).encode() {
            Ok(bytes) => bytes,
            Err(_) => return ExitCode::FAILURE,
        },
    };
    let mut out = std::io::stdout().lock();
    if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn read_request() -> Result<Request, String> {
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut input)
        .map_err(|e| format!("cannot read the request: {e}"))?;
    Request::decode(&input).map_err(|e| format!("malformed request: {e}"))
}
