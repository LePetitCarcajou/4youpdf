//! `fyp-plugin-api` — the contract between the 4YouPDF host and its modules.
//!
//! This crate is versioned **independently** of the rest of the workspace
//! (see `docs/adr/0002-versionnage.md`). A module compiled against API major
//! version N is refused by a host whose [`API_VERSION`] has a different major.
//!
//! Security model summary (full text in `docs/adr/0003-securite-modules.md`):
//! - a module has **no ambient authority**: it only receives what the host
//!   hands it through this API;
//! - every capability is declared in the manifest and granted per run;
//! - the host re-validates every document a module returns.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// Version of this API. Bump the major on any breaking change to the
/// manifest format, the traits or the exchange types.
pub const API_VERSION: &str = "0.1.0";

/// Where a module's code comes from. Determines the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    /// WebAssembly (WASI), fully sandboxed. **The only runtime allowed for
    /// third-party modules.**
    Wasm,
    /// Native Rust crate compiled into the host. Reserved for modules that
    /// live in this repository and pass review (OCR, rendering, ...).
    Native,
}

/// A capability a module may request. The host grants nothing by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Permission {
    /// Read the document currently open (through the API, never the file).
    ReadDocument,
    /// Produce a modified document (which the host validates before use).
    WriteDocument,
    /// Read files under a directory the user picks at run time.
    ReadDir,
    /// Write files under a directory the user picks at run time.
    WriteDir,
    /// Outbound network access to these exact hosts. Shown in orange in the
    /// UI; confirmed by the user on first use.
    Network {
        /// Allowed hosts, e.g. `["api.example.org"]`. Wildcards are refused.
        hosts: Vec<String>,
    },
    /// Spawn one specific external executable (e.g. `tesseract`).
    Subprocess {
        /// Executable name, resolved by the host, never a path chosen by the module.
        program: String,
    },
}

impl Permission {
    /// Permissions that must be confirmed interactively each first run.
    pub fn is_sensitive(&self) -> bool {
        matches!(
            self,
            Permission::Network { .. } | Permission::Subprocess { .. } | Permission::WriteDir
        )
    }
}

/// Resource ceilings applied to every module invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Wall-clock budget in milliseconds.
    pub timeout_ms: u64,
    /// Maximum memory in mebibytes.
    pub memory_mib: u64,
    /// Maximum size of any document the module may return, in mebibytes.
    pub max_output_mib: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            timeout_ms: 60_000,
            memory_mib: 512,
            max_output_mib: 1024,
        }
    }
}

/// What a module declares about itself. Stored as `manifest.toml` next to
/// the module code. Parsed and checked by the host before anything runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Unique identifier, reverse-DNS style: `org.4youpdf.merge`.
    pub id: String,
    /// Human name shown in the UI.
    pub name: String,
    /// Module version (SemVer).
    pub version: String,
    /// API version the module was built against (SemVer). Major must match.
    pub api_version: String,
    /// SPDX licence identifier. Must be AGPL-compatible for the catalogue.
    pub license: String,
    /// Public source repository. Mandatory for the catalogue: no source, no listing.
    pub source: String,
    /// Execution runtime.
    pub runtime: Runtime,
    /// Capabilities requested. Empty means the module can only compute.
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Resource limits. Defaults are applied when absent.
    #[serde(default)]
    pub limits: Limits,
    /// Actions the module contributes to the command palette and pipeline.
    #[serde(default)]
    pub actions: Vec<Action>,
}

/// One user-facing action contributed by a module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// Stable identifier, e.g. `merge`.
    pub id: String,
    /// Label in the UI (French default; translations come later).
    pub label: String,
    /// Which pipeline slot it fits: `pages`, `process`, `security`, `conformance`.
    pub category: String,
    /// Minimum number of input documents.
    #[serde(default = "one")]
    pub min_inputs: u32,
}

fn one() -> u32 {
    1
}

/// Reasons a manifest can be rejected before the module is even loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// `api_version` major differs from the host's [`API_VERSION`].
    IncompatibleApi {
        /// What the module asked for.
        wanted: String,
        /// What the host provides.
        have: String,
    },
    /// A third-party module declared `runtime = "native"`.
    NativeNotAllowed,
    /// A network permission used a wildcard or an empty host list.
    BadNetworkHosts,
    /// A `manifest.toml` field failed to parse.
    Invalid(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::IncompatibleApi { wanted, have } => {
                write!(f, "module built for API {wanted}, host provides {have}")
            }
            ManifestError::NativeNotAllowed => {
                write!(f, "third-party modules must use the wasm runtime")
            }
            ManifestError::BadNetworkHosts => write!(f, "network permission must list exact hosts"),
            ManifestError::Invalid(m) => write!(f, "invalid manifest: {m}"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    /// Parse a `manifest.toml`.
    pub fn from_toml(text: &str) -> Result<Manifest, ManifestError> {
        toml::from_str(text).map_err(|e| ManifestError::Invalid(e.to_string()))
    }

    /// Static checks that do not require loading any code.
    /// `trusted` is true only for modules shipped in this repository.
    pub fn validate(&self, trusted: bool) -> Result<(), ManifestError> {
        let wanted = semver::Version::parse(&self.api_version)
            .map_err(|e| ManifestError::Invalid(format!("api_version: {e}")))?;
        let have = semver::Version::parse(API_VERSION)
            .map_err(|e| ManifestError::Invalid(format!("host API_VERSION: {e}")))?;
        // Pre-1.0: minor acts as the compatibility boundary (SemVer convention).
        let compatible = if have.major == 0 {
            wanted.major == 0 && wanted.minor == have.minor
        } else {
            wanted.major == have.major
        };
        if !compatible {
            return Err(ManifestError::IncompatibleApi {
                wanted: self.api_version.clone(),
                have: API_VERSION.to_string(),
            });
        }
        if self.runtime == Runtime::Native && !trusted {
            return Err(ManifestError::NativeNotAllowed);
        }
        for p in &self.permissions {
            if let Permission::Network { hosts } = p {
                if hosts.is_empty() || hosts.iter().any(|h| h.contains('*') || h.is_empty()) {
                    return Err(ManifestError::BadNetworkHosts);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const MERGE: &str = r#"
id = "org.4youpdf.merge"
name = "Fusionner"
version = "0.1.0"
api_version = "0.1.0"
license = "AGPL-3.0-or-later"
source = "https://github.com/4youpdf/4youpdf/tree/main/plugins/merge"
runtime = "wasm"
permissions = [{ kind = "read_document" }, { kind = "write_document" }]

[[actions]]
id = "merge"
label = "Fusionner"
category = "pages"
min_inputs = 2
"#;

    #[test]
    fn parses_and_validates() {
        let m = Manifest::from_toml(MERGE).expect("manifest");
        assert_eq!(m.id, "org.4youpdf.merge");
        assert_eq!(m.limits, Limits::default());
        assert_eq!(m.actions[0].min_inputs, 2);
        m.validate(false).expect("valid");
    }

    #[test]
    fn refuses_native_third_party() {
        let text = MERGE.replace("runtime = \"wasm\"", "runtime = \"native\"");
        let m = Manifest::from_toml(&text).expect("manifest");
        assert_eq!(m.validate(false), Err(ManifestError::NativeNotAllowed));
        m.validate(true).expect("trusted native ok");
    }

    #[test]
    fn refuses_incompatible_api() {
        let text = MERGE.replace("api_version = \"0.1.0\"", "api_version = \"0.9.0\"");
        let m = Manifest::from_toml(&text).expect("manifest");
        assert!(matches!(
            m.validate(false),
            Err(ManifestError::IncompatibleApi { .. })
        ));
    }

    #[test]
    fn refuses_wildcard_hosts() {
        let text = MERGE.replace(
            "permissions = [{ kind = \"read_document\" }, { kind = \"write_document\" }]",
            "permissions = [{ kind = \"network\", hosts = [\"*.example.org\"] }]",
        );
        let m = Manifest::from_toml(&text).expect("manifest");
        assert_eq!(m.validate(false), Err(ManifestError::BadNetworkHosts));
    }
}
