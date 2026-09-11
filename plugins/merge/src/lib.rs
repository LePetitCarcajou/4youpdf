//! Merge module. Built for `wasm32-wasip1` and loaded by `fyp-host`.
//!
//! Milestone 0.1: the module only declares the API version it targets so the
//! host can check compatibility. The WASI export surface (`fyp_run`, memory
//! exchange) is defined together with the Wasmtime loader in milestone 0.2,
//! and the page-tree merge itself needs the document layer of `fyp-core`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// API version this module targets, as `(major, minor)`.
pub fn api_version() -> (u32, u32) {
    (0, 1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn declares_current_api() {
        let mut it = fyp_plugin_api::API_VERSION.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        let host = (it.next().unwrap_or(0), it.next().unwrap_or(0));
        assert_eq!(super::api_version(), host);
    }
}
