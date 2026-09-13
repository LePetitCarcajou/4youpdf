//! What an engine receives and what it answers (`docs/banc-rendu.md`,
//! « Protocole des moteurs »).
//!
//! The bench runs an engine once per document. It writes one [`Request`] as
//! JSON on the engine's standard input, then closes it. The engine writes one
//! [`Reply`] per line on its standard output, flushing each line, and may
//! write anything on its standard error, which the report shows when the
//! engine fails. In order: [`Reply::Engine`]; then [`Reply::Opened`] or
//! [`Reply::OpenFailed`]; after an opening, one [`Reply::Page`] or
//! [`Reply::PageFailed`] per requested page, in any order. [`Reply::Fatal`]
//! may come instead of any of them and ends the run of the document.
//!
//! The times are the engine's to take, around its own calls, so that neither
//! starting a process nor reading the file counts.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Version of this protocol, in [`Request::protocol`] and in
/// [`Reply::Engine`]: an engine refuses a request of another version.
pub const PROTOCOL: u32 = 1;

/// One document to render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// [`PROTOCOL`].
    pub protocol: u32,
    /// The PDF file, as an absolute path.
    pub document: PathBuf,
    /// Password to open it with; empty for most files.
    #[serde(default)]
    pub password: String,
    /// Width of every image, in pixels. The height follows the proportions
    /// of the page as displayed, turned by its `/Rotate`.
    pub width: u32,
    /// How many times to draw and encode each page. Every time is reported;
    /// the image written is the first.
    pub repeat: u32,
    /// The pages and where to write their images.
    pub pages: Vec<PageRequest>,
}

/// One page of a [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageRequest {
    /// Page number, 0-based.
    pub index: usize,
    /// Where to write the PNG image of the page.
    pub output: PathBuf,
}

/// One line of an engine's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    /// Who answers. Always the first line.
    Engine {
        /// The engine's [`PROTOCOL`].
        protocol: u32,
        /// Its name in `engines.toml`.
        name: String,
        /// The version that runs, as precisely as the engine knows it.
        version: String,
        /// Anything else a reader of the report needs: which library was
        /// loaded, from where.
        detail: String,
    },
    /// The document is open.
    Opened {
        /// Time to open it, in milliseconds: from its bytes in memory to a
        /// document whose pages can be drawn.
        ms: f64,
    },
    /// The document does not open; no page follows.
    OpenFailed {
        /// Why, in words.
        error: String,
    },
    /// A page drawn, encoded and written.
    Page {
        /// Its [`PageRequest::index`].
        index: usize,
        /// Width of the image, in pixels.
        width: u32,
        /// Height of the image, in pixels.
        height: u32,
        /// Each time taken to draw the page, in milliseconds: from the page
        /// asked for to its pixels in memory, 8 bits per channel.
        render_ms: Vec<f64>,
        /// Each time taken to encode those pixels as a PNG, in
        /// milliseconds, the way the application's page service encodes
        /// them.
        encode_ms: Vec<f64>,
        /// Whether every repetition gave the same pixels as the first.
        identical_repeats: bool,
    },
    /// A page that could not be drawn or written.
    PageFailed {
        /// Its [`PageRequest::index`].
        index: usize,
        /// Why, in words.
        error: String,
    },
    /// The engine cannot go on: no library, a request it does not
    /// understand.
    Fatal {
        /// Why, in words.
        error: String,
    },
}

impl Reply {
    /// The reply as one line of JSON, without the line break.
    pub fn to_line(&self) -> String {
        // Serializing these types cannot fail: no map with non-string keys,
        // no value that JSON cannot represent except a non-finite time, which
        // serde_json writes as `null` rather than failing.
        serde_json::to_string(self).unwrap_or_else(|e| {
            format!(r#"{{"type":"fatal","error":"réponse impossible à écrire : {e}"}}"#)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_one_json_line_tagged_by_type() {
        let page = Reply::Page {
            index: 3,
            width: 1400,
            height: 1980,
            render_ms: vec![12.5, 11.0],
            encode_ms: vec![40.0, 41.5],
            identical_repeats: true,
        };
        let line = page.to_line();
        assert!(!line.contains('\n'));
        assert!(line.starts_with(r#"{"type":"page","#), "{line}");
        assert_eq!(serde_json::from_str::<Reply>(&line).unwrap(), page);
        let failed: Reply =
            serde_json::from_str(r#"{"type":"page_failed","index":2,"error":"hors de portée"}"#)
                .unwrap();
        assert_eq!(
            failed,
            Reply::PageFailed {
                index: 2,
                error: "hors de portée".into()
            }
        );
        assert!(serde_json::from_str::<Reply>(r#"{"type":"bonjour"}"#).is_err());
    }

    #[test]
    fn a_request_without_password_has_an_empty_one() {
        let request: Request = serde_json::from_str(
            r#"{"protocol":1,"document":"a.pdf","width":800,"repeat":1,"pages":[{"index":0,"output":"0.png"}]}"#,
        )
        .unwrap();
        assert_eq!(request.password, "");
        assert_eq!(request.pages[0].index, 0);
    }
}
