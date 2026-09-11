//! The PDF object model (ISO 32000-2:2020, clause 7.3).

use std::collections::BTreeMap;

/// A PDF name object, e.g. `/Type`. Stored without the leading slash and
/// with `#xx` escapes already decoded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(
    /// Raw bytes of the name, without the leading `/`.
    pub Vec<u8>,
);

impl Name {
    /// Build a name from a string slice.
    pub fn new(s: &str) -> Self {
        Name(s.as_bytes().to_vec())
    }

    /// Lossy UTF-8 view, for display and debugging.
    pub fn as_str_lossy(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }
}

/// Reference to an indirect object: `12 0 R`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjRef {
    /// Object number.
    pub num: u32,
    /// Generation number.
    pub gen: u16,
}

/// A PDF dictionary. Keys are names; order is not significant in PDF, so a
/// `BTreeMap` gives deterministic output when writing.
pub type Dict = BTreeMap<Name, Object>;

/// Any direct PDF object.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    /// The `null` object.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// Integer number.
    Integer(i64),
    /// Real number.
    Real(f64),
    /// Literal `(...)` or hexadecimal `<...>` string, raw bytes after unescaping.
    String(Vec<u8>),
    /// Name object.
    Name(Name),
    /// Array of objects.
    Array(Vec<Object>),
    /// Dictionary.
    Dict(Dict),
    /// Stream: dictionary plus raw (still encoded) data.
    Stream {
        /// Stream dictionary (`/Length`, `/Filter`, ...).
        dict: Dict,
        /// Raw bytes as found in the file, filters not yet applied.
        data: Vec<u8>,
    },
    /// Indirect reference.
    Reference(ObjRef),
}

impl Object {
    /// Return the dictionary if this object is a dictionary or a stream.
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Object::Dict(d) | Object::Stream { dict: d, .. } => Some(d),
            _ => None,
        }
    }

    /// Return the integer value, if any.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Object::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Return the name, if any.
    pub fn as_name(&self) -> Option<&Name> {
        match self {
            Object::Name(n) => Some(n),
            _ => None,
        }
    }
}
