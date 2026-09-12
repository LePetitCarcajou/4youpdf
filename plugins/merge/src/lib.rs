//! Merge module: concatenates the documents it receives, in order, with
//! `fyp_core::ops::merge`.
//!
//! Built for `wasm32-wasip1` as a WASI command (`src/main.rs`) and run by
//! `fyp-host` in its sandbox. It declares `read_document` and
//! `write_document` only: it reads the documents the host hands it on
//! standard input and answers with the merged document on standard output,
//! which the host re-validates. The core is compiled into the module; it
//! gives the module no authority, only code.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use fyp_core::document::Document;
use fyp_core::ops;
use fyp_plugin_api::exchange::Request;

/// Identifier of the action, as declared in `manifest.toml`.
pub const MERGE: &str = "merge";

/// Answer one request: the merged document, or why it cannot be made.
pub fn handle(request: &Request) -> Result<Vec<u8>, String> {
    if request.action != MERGE {
        return Err(format!("unknown action `{}`", request.action));
    }
    if let Some(name) = request.params.keys().next() {
        return Err(format!("unexpected parameter `{name}`"));
    }
    let documents = request
        .documents
        .iter()
        .enumerate()
        .map(|(i, bytes)| Document::open(bytes).map_err(|e| format!("document {}: {e}", i + 1)))
        .collect::<Result<Vec<_>, _>>()?;
    ops::merge(&documents).map_err(|e| format!("merge: {e}"))
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use fyp_plugin_api::exchange::ParamValue;
    use fyp_plugin_api::{Manifest, Permission};
    use std::collections::BTreeMap;
    use std::path::Path;

    const MANIFEST: &str = include_str!("../manifest.toml");

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn manifest_declares_the_action_and_document_permissions_only() {
        let m = Manifest::from_toml(MANIFEST).unwrap();
        m.validate(false).unwrap();
        assert_eq!(m.api_version, fyp_plugin_api::API_VERSION);
        assert_eq!(
            m.permissions,
            [Permission::ReadDocument, Permission::WriteDocument]
        );
        let action = m.actions.iter().find(|a| a.id == MERGE).unwrap();
        assert_eq!(action.min_inputs, 2);
        assert!(action.params.is_empty());
    }

    #[test]
    fn merges_like_the_core() {
        let files = vec![fixture("minimal.pdf"), fixture("objstm.pdf")];
        let docs: Vec<Document<'_>> = files.iter().map(|f| Document::open(f).unwrap()).collect();
        let request = Request {
            action: MERGE.into(),
            params: BTreeMap::new(),
            documents: files.clone(),
        };
        assert_eq!(handle(&request).unwrap(), ops::merge(&docs).unwrap());
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        let files = vec![fixture("minimal.pdf"), b"not a pdf".to_vec()];
        let mut request = Request {
            action: "split".into(),
            params: BTreeMap::new(),
            documents: files,
        };
        assert!(handle(&request).unwrap_err().contains("unknown action"));
        request.action = MERGE.into();
        assert!(handle(&request).unwrap_err().starts_with("document 2:"));
        request
            .params
            .insert("every".into(), ParamValue::Integer(2));
        assert!(handle(&request)
            .unwrap_err()
            .contains("unexpected parameter"));
    }
}
