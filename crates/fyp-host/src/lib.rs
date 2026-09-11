//! `fyp-host` — discovers, checks and runs modules.
//!
//! Milestone 0.1: discovery and manifest validation only. Actual execution
//! (Wasmtime sandbox with WASI, resource limits, permission prompts) arrives
//! in milestone 0.2 once the core can round-trip the corpus.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use fyp_plugin_api::{Manifest, ManifestError};
use std::path::{Path, PathBuf};

/// A module found on disk whose manifest passed static validation.
#[derive(Debug, Clone)]
pub struct DiscoveredModule {
    /// Directory containing `manifest.toml`.
    pub dir: PathBuf,
    /// Parsed manifest.
    pub manifest: Manifest,
    /// True if it comes from the trusted (in-repo, signed) location.
    pub trusted: bool,
}

/// Errors during discovery.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// Filesystem error.
    #[error("io error at {}: {source}", path.display())]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Manifest rejected.
    #[error("{}: {source}", path.display())]
    Manifest {
        /// Path to the manifest.
        path: PathBuf,
        /// Why.
        #[source]
        source: ManifestError,
    },
}

/// Scan `root` for `*/manifest.toml`, validate each, and return the accepted
/// modules. Rejected modules are returned as errors so the UI can explain
/// why they are not loaded — silently dropping them would hide problems.
pub fn discover(root: &Path, trusted: bool) -> (Vec<DiscoveredModule>, Vec<HostError>) {
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(source) => {
            errors.push(HostError::Io { path: root.to_path_buf(), source });
            return (ok, errors);
        }
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let manifest_path = dir.join("manifest.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let text = match std::fs::read_to_string(&manifest_path) {
            Ok(t) => t,
            Err(source) => {
                errors.push(HostError::Io { path: manifest_path, source });
                continue;
            }
        };
        let manifest = match Manifest::from_toml(&text).and_then(|m| m.validate(trusted).map(|()| m)) {
            Ok(m) => m,
            Err(source) => {
                errors.push(HostError::Manifest { path: manifest_path, source });
                continue;
            }
        };
        ok.push(DiscoveredModule { dir, manifest, trusted });
    }
    ok.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    (ok, errors)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn discovers_in_repo_plugins() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let (found, errors) = discover(&root, true);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(found.iter().any(|m| m.manifest.id == "org.4youpdf.merge"));
    }
}
