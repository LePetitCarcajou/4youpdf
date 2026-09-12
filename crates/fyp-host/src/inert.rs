//! Text a module controls, on its way to a person: its error message, its
//! standard error, the names of its imports, the fields of its manifest.
//!
//! A terminal obeys what it is sent: ESC starts a command (clear the
//! screen, retitle the window, hide what follows), `\r` goes back over the
//! line. Unicode bidirectional controls reorder the text around them, so
//! that `fdp.exe` shows as `exe.pdf`. None of that must reach a person
//! from a module: such characters are replaced with U+FFFD, and the text
//! is cut to a size a window can show.

use fyp_plugin_api::{Manifest, ManifestError, Permission};

/// Longest error message kept, in bytes.
pub(crate) const MAX_MESSAGE: usize = 4 << 10;
/// Longest standard error or trap description kept, in bytes.
pub(crate) const MAX_DIAGNOSTICS: usize = 64 << 10;

/// Whether `c` acts on how text is displayed instead of being displayed:
/// C0 and C1 controls, bidirectional marks, embeddings, overrides and
/// isolates (Unicode Standard Annex #9).
fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// `text` with every hidden character replaced, line feeds and tabs kept,
/// cut to about `max` bytes on a character boundary.
pub(crate) fn lines(text: &str, max: usize) -> String {
    shown(text, true, max)
}

/// `text` on one line: line feeds and tabs replaced too.
pub(crate) fn line(text: &str) -> String {
    shown(text, false, MAX_MESSAGE)
}

fn shown(text: &str, multiline: bool, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max));
    for c in text.chars() {
        let kept = multiline && (c == '\n' || c == '\t');
        let c = if is_hidden(c) && !kept { '\u{fffd}' } else { c };
        if out.len() + c.len_utf8() > max {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

/// Refuse a manifest with a hidden character in a field a person reads.
/// `Manifest::validate` accepts them; the host does not show them.
pub(crate) fn check_manifest(manifest: &Manifest) -> Result<(), ManifestError> {
    let mut fields: Vec<(&str, &str)> = vec![
        ("id", &manifest.id),
        ("name", &manifest.name),
        ("version", &manifest.version),
        ("api_version", &manifest.api_version),
        ("license", &manifest.license),
        ("source", &manifest.source),
    ];
    for permission in &manifest.permissions {
        match permission {
            Permission::Network { hosts } => {
                fields.extend(hosts.iter().map(|h| ("permissions.hosts", h.as_str())));
            }
            Permission::Subprocess { program } => fields.push(("permissions.program", program)),
            _ => {}
        }
    }
    for action in &manifest.actions {
        fields.push(("actions.id", &action.id));
        fields.push(("actions.label", &action.label));
        fields.push(("actions.category", &action.category));
        for param in &action.params {
            fields.push(("actions.params.id", &param.id));
            fields.push(("actions.params.label", &param.label));
        }
    }
    match fields
        .iter()
        .find(|(_, value)| value.chars().any(is_hidden))
    {
        Some((field, _)) => Err(ManifestError::Invalid(format!(
            "`{field}` contains a control or bidirectional formatting character"
        ))),
        None => Ok(()),
    }
}

/// A manifest error whose text quotes the manifest, made inert.
pub(crate) fn manifest_error(error: ManifestError) -> ManifestError {
    match error {
        ManifestError::IncompatibleApi { wanted, have } => ManifestError::IncompatibleApi {
            wanted: line(&wanted),
            have,
        },
        ManifestError::BadIdentifier(m) => ManifestError::BadIdentifier(line(&m)),
        ManifestError::BadParam(m) => ManifestError::BadParam(line(&m)),
        ManifestError::Invalid(m) => ManifestError::Invalid(lines(&m, MAX_MESSAGE)),
        other => other,
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn hidden_characters_are_replaced_and_text_is_cut() {
        assert_eq!(line("a\u{1b}[2Jb\rc\nd"), "a\u{fffd}[2Jb\u{fffd}c\u{fffd}d");
        assert_eq!(lines("a\nb\tc\u{85}", 100), "a\nb\tc\u{fffd}");
        assert_eq!(line("fdp\u{202e}.exe"), "fdp\u{fffd}.exe");
        let cut = lines(&"é".repeat(10), 5);
        assert_eq!(cut, "éé…");
        assert_eq!(line("Fusion : pas de page"), "Fusion : pas de page");
    }
}
