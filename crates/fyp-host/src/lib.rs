//! `fyp-host` — discovers, checks and runs modules.
//!
//! - [`discover`] finds `*/manifest.toml` and validates each manifest
//!   before any code is read.
//! - [`Host::load`] compiles the module's `module.wasm` and links it
//!   against the few WASI functions the host implements itself; every
//!   other import becomes a trap naming it.
//! - [`LoadedModule::run`] runs one action under the manifest's limits
//!   (time, memory, output size) and passes the returned document through
//!   [`revalidate`] before anyone sees it.
//!
//! The security model is ADR 0003 (`docs/adr/0003-securite-modules.md`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod sandbox;
mod wasi;

pub use fyp_plugin_api::exchange::ParamValue;
pub use sandbox::{revalidate, Host, LoadedModule, RunOutput, MODULE_FILE};

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

/// Errors while discovering, loading or running a module. None of them
/// leaves the host in a bad state: the run is over, the documents given
/// to it are untouched.
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
    /// The WebAssembly engine could not be configured.
    #[error("cannot start the WebAssembly engine: {0}")]
    Engine(String),
    /// Code the sandbox does not run: not WebAssembly, too large, no
    /// `_start` or `memory` export, a shared memory, an import that is not
    /// a function or does not match the WASI function of that name.
    #[error("module {module} refused: {message}")]
    BadModule {
        /// Module identifier.
        module: String,
        /// Why.
        message: String,
    },
    /// The manifest asks for a permission this host cannot grant yet.
    #[error(
        "module {module} asks for the permission {permission}, which this host cannot grant yet"
    )]
    PermissionUnavailable {
        /// Module identifier.
        module: String,
        /// The permission, as written in manifests.
        permission: String,
    },
    /// The run needs a permission the manifest does not declare.
    #[error("module {module} does not declare the permission {permission} this run needs")]
    PermissionNotDeclared {
        /// Module identifier.
        module: String,
        /// The permission, as written in manifests.
        permission: String,
    },
    /// The manifest has no such action.
    #[error("module {module} has no action `{action}`")]
    UnknownAction {
        /// Module identifier.
        module: String,
        /// The action asked for.
        action: String,
    },
    /// Fewer documents than the action's `min_inputs`.
    #[error("action `{action}` of {module} needs at least {min} document(s), {given} given")]
    NotEnoughInputs {
        /// Module identifier.
        module: String,
        /// The action.
        action: String,
        /// Its `min_inputs`.
        min: u32,
        /// Documents given.
        given: usize,
    },
    /// A parameter the action does not declare, of the wrong kind, out of
    /// bounds, or a required one missing. Checked before the module starts.
    #[error("action `{action}` of {module}: {message}")]
    BadParameter {
        /// Module identifier.
        module: String,
        /// The action.
        action: String,
        /// What is wrong.
        message: String,
    },
    /// The module ran past its `timeout_ms` and was stopped.
    #[error("module {module} stopped: over its time limit of {limit_ms} ms")]
    Timeout {
        /// Module identifier.
        module: String,
        /// The limit.
        limit_ms: u64,
    },
    /// The module asked for more memory than its `memory_mib`.
    #[error("module {module} stopped: over its memory limit of {limit_mib} MiB")]
    MemoryExceeded {
        /// Module identifier.
        module: String,
        /// The limit.
        limit_mib: u64,
    },
    /// The module wrote more than its `max_output_mib`.
    #[error("module {module} stopped: its answer is over the output limit of {limit_mib} MiB")]
    OutputTooLarge {
        /// Module identifier.
        module: String,
        /// The limit.
        limit_mib: u64,
    },
    /// The module called a function the host does not grant: a WASI
    /// primitive outside the sandbox's set (files, sockets, clock, sleep)
    /// or any import of another namespace.
    #[error("module {module} stopped: it called {import}, which its permissions do not grant")]
    CapabilityDenied {
        /// Module identifier.
        module: String,
        /// `namespace::function` of the import.
        import: String,
    },
    /// The module trapped: panic, stack overflow, out-of-bounds access,
    /// unreachable code.
    #[error("module {module} stopped: {message}{}", stderr_suffix(.diagnostics))]
    Trapped {
        /// Module identifier.
        module: String,
        /// The trap.
        message: String,
        /// What the module wrote on its standard error.
        diagnostics: String,
    },
    /// The module exited with a non-zero code without a usable answer.
    #[error("module {module} exited with code {code} without an answer{}", stderr_suffix(.diagnostics))]
    Exited {
        /// Module identifier.
        module: String,
        /// Its exit code.
        code: i32,
        /// What the module wrote on its standard error.
        diagnostics: String,
    },
    /// The module's standard output is not an answer.
    #[error("module {module}: {message}{}", stderr_suffix(.diagnostics))]
    BadResponse {
        /// Module identifier.
        module: String,
        /// What is wrong with it.
        message: String,
        /// What the module wrote on its standard error.
        diagnostics: String,
    },
    /// The module answered with an error message.
    #[error("module {module} failed: {message}")]
    ModuleFailed {
        /// Module identifier.
        module: String,
        /// Its message.
        message: String,
    },
    /// The document returned by the module did not pass re-validation.
    #[error("document returned by {module} rejected at re-validation: {reason}")]
    Rejected {
        /// Module identifier.
        module: String,
        /// What the core found.
        #[source]
        reason: fyp_core::Error,
    },
    /// The host itself failed (a thread could not start).
    #[error("host failure: {0}")]
    Internal(String),
}

fn stderr_suffix(diagnostics: &str) -> String {
    let trimmed = diagnostics.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" (stderr: {trimmed})")
    }
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
            errors.push(HostError::Io {
                path: root.to_path_buf(),
                source,
            });
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
                errors.push(HostError::Io {
                    path: manifest_path,
                    source,
                });
                continue;
            }
        };
        let manifest =
            match Manifest::from_toml(&text).and_then(|m| m.validate(trusted).map(|()| m)) {
                Ok(m) => m,
                Err(source) => {
                    errors.push(HostError::Manifest {
                        path: manifest_path,
                        source,
                    });
                    continue;
                }
            };
        ok.push(DiscoveredModule {
            dir,
            manifest,
            trusted,
        });
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
