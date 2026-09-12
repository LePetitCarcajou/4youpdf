//! The merge module as a WASI command: the request on standard input, the
//! answer on standard output (`fyp_plugin_api::exchange`).

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    fyp_plugin_api::module::serve(|request| fyp_plugin_merge::handle(&request))
}
