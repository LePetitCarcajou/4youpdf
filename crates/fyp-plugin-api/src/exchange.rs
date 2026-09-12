//! What travels between the host and a module during one run.
//!
//! A module is a WASI command. The host writes one [`Request`] on its
//! standard input, runs `_start`, and reads one [`Response`] from its
//! standard output. Nothing else crosses the sandbox boundary: no file, no
//! socket, no clock, no environment (ADR 0003).
//!
//! The encoding is a small binary format, so a module in any language can
//! implement it without a serialisation library. Integers are
//! little-endian; a *string* or *bytes* field is a `u32` length followed
//! by that many bytes (UTF-8 for a string).
//!
//! ```text
//! request  = "FYPQ" u32:version string:action
//!            u32:count (string:name value)*   names in increasing byte order
//!            u32:count (bytes:document)*      documents, in order
//! value    = 0x00 u8:(0|1)                    boolean
//!          | 0x01 i64                         integer
//!          | 0x02 string                      text
//! response = "FYPR" u32:version
//!            ( 0x00 bytes:document | 0x01 string:message )
//! ```
//!
//! Decoding never trusts a length: every count and size is checked against
//! the bytes actually present, nothing is allocated ahead from a count,
//! and trailing bytes are an error.

use std::collections::BTreeMap;
use std::fmt;

use crate::ParamKind;

/// Version of the encoding, written after the magic of both messages.
pub const PROTOCOL_VERSION: u32 = 1;

const REQUEST_MAGIC: &[u8; 4] = b"FYPQ";
const RESPONSE_MAGIC: &[u8; 4] = b"FYPR";

const TAG_BOOLEAN: u8 = 0;
const TAG_INTEGER: u8 = 1;
const TAG_TEXT: u8 = 2;
const TAG_DOCUMENT: u8 = 0;
const TAG_ERROR: u8 = 1;

/// The value of one parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    /// For a `boolean` parameter.
    Boolean(bool),
    /// For an `integer` parameter.
    Integer(i64),
    /// For a `text` parameter.
    Text(String),
}

impl ParamValue {
    /// The kind of parameter this value fits.
    pub fn kind(&self) -> ParamKind {
        match self {
            ParamValue::Boolean(_) => ParamKind::Boolean,
            ParamValue::Integer(_) => ParamKind::Integer,
            ParamValue::Text(_) => ParamKind::Text,
        }
    }

    /// A value of `kind` typed by a person, e.g. on the command line:
    /// `true` or `false`, a decimal integer, any text.
    pub fn parse(kind: ParamKind, text: &str) -> Result<ParamValue, String> {
        match kind {
            ParamKind::Boolean => match text.trim() {
                "true" => Ok(ParamValue::Boolean(true)),
                "false" => Ok(ParamValue::Boolean(false)),
                other => Err(format!("`{other}` is not a boolean (true or false)")),
            },
            ParamKind::Integer => text
                .trim()
                .parse()
                .map(ParamValue::Integer)
                .map_err(|_| format!("`{}` is not an integer", text.trim())),
            ParamKind::Text => Ok(ParamValue::Text(text.to_string())),
        }
    }
}

/// What the host sends: the action to run, its parameters, the documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Identifier of the action, as declared in the manifest.
    pub action: String,
    /// Parameters given for this run, already checked by the host against
    /// the manifest.
    pub params: BTreeMap<String, ParamValue>,
    /// The documents, in the order the user chose.
    pub documents: Vec<Vec<u8>>,
}

/// What the module answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// The resulting document. The host re-validates it before using it.
    Document(Vec<u8>),
    /// Why the action could not be done, for the user.
    Error(String),
}

/// A message that cannot be encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeError {
    /// The message does not start with the expected magic.
    BadMagic,
    /// Written for another version of the encoding.
    UnsupportedVersion(u32),
    /// Ends before a field announced by a length or a count.
    Truncated,
    /// A string is not UTF-8.
    BadUtf8,
    /// A tag byte this version does not define.
    BadTag(u8),
    /// Parameter names repeated or not in increasing order.
    BadParamOrder,
    /// Bytes after the end of the message.
    TrailingBytes,
    /// A field longer than a `u32` length can describe.
    TooLarge,
}

impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExchangeError::BadMagic => write!(f, "not a 4YouPDF exchange message"),
            ExchangeError::UnsupportedVersion(v) => {
                write!(f, "exchange version {v}, expected {PROTOCOL_VERSION}")
            }
            ExchangeError::Truncated => write!(f, "message truncated"),
            ExchangeError::BadUtf8 => write!(f, "string is not UTF-8"),
            ExchangeError::BadTag(t) => write!(f, "unknown tag {t}"),
            ExchangeError::BadParamOrder => {
                write!(f, "parameter names repeated or out of order")
            }
            ExchangeError::TrailingBytes => write!(f, "bytes after the end of the message"),
            ExchangeError::TooLarge => write!(f, "field longer than 4 GiB"),
        }
    }
}

impl std::error::Error for ExchangeError {}

/// Encode a request without copying the documents first: what
/// [`Request::encode`] does, for a host holding borrowed documents.
pub fn encode_request(
    action: &str,
    params: &BTreeMap<String, ParamValue>,
    documents: &[&[u8]],
) -> Result<Vec<u8>, ExchangeError> {
    let size = documents
        .iter()
        .fold(64 + action.len(), |n, d| n.saturating_add(d.len() + 4));
    let mut out = Out(Vec::with_capacity(size));
    out.0.extend_from_slice(REQUEST_MAGIC);
    out.u32(PROTOCOL_VERSION);
    out.bytes(action.as_bytes())?;
    out.count(params.len())?;
    for (name, value) in params {
        out.bytes(name.as_bytes())?;
        match value {
            ParamValue::Boolean(b) => {
                out.0.push(TAG_BOOLEAN);
                out.0.push(u8::from(*b));
            }
            ParamValue::Integer(i) => {
                out.0.push(TAG_INTEGER);
                out.0.extend_from_slice(&i.to_le_bytes());
            }
            ParamValue::Text(t) => {
                out.0.push(TAG_TEXT);
                out.bytes(t.as_bytes())?;
            }
        }
    }
    out.count(documents.len())?;
    for document in documents {
        out.bytes(document)?;
    }
    Ok(out.0)
}

impl Request {
    /// The bytes the host writes on the module's standard input.
    pub fn encode(&self) -> Result<Vec<u8>, ExchangeError> {
        let documents: Vec<&[u8]> = self.documents.iter().map(Vec::as_slice).collect();
        encode_request(&self.action, &self.params, &documents)
    }

    /// Read a request, as a module does from its standard input.
    pub fn decode(bytes: &[u8]) -> Result<Request, ExchangeError> {
        let mut input = In { bytes, pos: 0 };
        input.header(REQUEST_MAGIC)?;
        let action = input.string()?;
        let mut params = BTreeMap::new();
        for _ in 0..input.u32()? {
            let name = input.string()?;
            if params.keys().next_back().is_some_and(|last| *last >= name) {
                return Err(ExchangeError::BadParamOrder);
            }
            let value = match input.u8()? {
                TAG_BOOLEAN => match input.u8()? {
                    0 => ParamValue::Boolean(false),
                    1 => ParamValue::Boolean(true),
                    other => return Err(ExchangeError::BadTag(other)),
                },
                TAG_INTEGER => ParamValue::Integer(i64::from_le_bytes(input.array()?)),
                TAG_TEXT => ParamValue::Text(input.string()?),
                other => return Err(ExchangeError::BadTag(other)),
            };
            params.insert(name, value);
        }
        let mut documents = Vec::new();
        for _ in 0..input.u32()? {
            documents.push(input.bytes()?.to_vec());
        }
        input.finish()?;
        Ok(Request {
            action,
            params,
            documents,
        })
    }
}

impl Response {
    /// The bytes a module writes on its standard output.
    pub fn encode(&self) -> Result<Vec<u8>, ExchangeError> {
        let (tag, payload) = match self {
            Response::Document(d) => (TAG_DOCUMENT, d.as_slice()),
            Response::Error(m) => (TAG_ERROR, m.as_bytes()),
        };
        let mut out = Out(Vec::with_capacity(payload.len().saturating_add(13)));
        out.0.extend_from_slice(RESPONSE_MAGIC);
        out.u32(PROTOCOL_VERSION);
        out.0.push(tag);
        out.bytes(payload)?;
        Ok(out.0)
    }

    /// Read a response, as the host does from the module's standard output.
    pub fn decode(bytes: &[u8]) -> Result<Response, ExchangeError> {
        let mut input = In { bytes, pos: 0 };
        input.header(RESPONSE_MAGIC)?;
        let response = match input.u8()? {
            TAG_DOCUMENT => Response::Document(input.bytes()?.to_vec()),
            TAG_ERROR => Response::Error(input.string()?),
            other => return Err(ExchangeError::BadTag(other)),
        };
        input.finish()?;
        Ok(response)
    }
}

struct Out(Vec<u8>);

impl Out {
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn count(&mut self, n: usize) -> Result<(), ExchangeError> {
        self.u32(u32::try_from(n).map_err(|_| ExchangeError::TooLarge)?);
        Ok(())
    }

    fn bytes(&mut self, data: &[u8]) -> Result<(), ExchangeError> {
        self.count(data.len())?;
        self.0.extend_from_slice(data);
        Ok(())
    }
}

struct In<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ExchangeError> {
        let end = self.pos.checked_add(n).ok_or(ExchangeError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(ExchangeError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ExchangeError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ExchangeError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, ExchangeError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, ExchangeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn bytes(&mut self) -> Result<&'a [u8], ExchangeError> {
        let len = usize::try_from(self.u32()?).map_err(|_| ExchangeError::Truncated)?;
        self.take(len)
    }

    fn string(&mut self) -> Result<String, ExchangeError> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|_| ExchangeError::BadUtf8)
    }

    fn header(&mut self, magic: &[u8; 4]) -> Result<(), ExchangeError> {
        if self.take(4).ok() != Some(magic.as_slice()) {
            return Err(ExchangeError::BadMagic);
        }
        match self.u32()? {
            PROTOCOL_VERSION => Ok(()),
            other => Err(ExchangeError::UnsupportedVersion(other)),
        }
    }

    fn finish(&self) -> Result<(), ExchangeError> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(ExchangeError::TrailingBytes)
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample() -> Request {
        let mut params = BTreeMap::new();
        params.insert("every".to_string(), ParamValue::Integer(-7));
        params.insert("keep".to_string(), ParamValue::Boolean(true));
        params.insert("title".to_string(), ParamValue::Text("Été".to_string()));
        Request {
            action: "split".to_string(),
            params,
            documents: vec![b"%PDF-1.7 first".to_vec(), Vec::new()],
        }
    }

    #[test]
    fn round_trips() {
        let request = sample();
        assert_eq!(Request::decode(&request.encode().unwrap()), Ok(request));
        for response in [
            Response::Document(b"%PDF".to_vec()),
            Response::Error("pas de page".to_string()),
        ] {
            assert_eq!(Response::decode(&response.encode().unwrap()), Ok(response));
        }
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = sample().encode().unwrap();
        for end in 0..bytes.len() {
            assert!(
                Request::decode(&bytes[..end]).is_err(),
                "prefix of {end} bytes"
            );
        }
        let response = Response::Document(vec![1, 2, 3]).encode().unwrap();
        for end in 0..response.len() {
            assert!(Response::decode(&response[..end]).is_err());
        }
    }

    #[test]
    fn hostile_messages_are_errors() {
        let mut bytes = sample().encode().unwrap();
        bytes.push(0);
        assert_eq!(Request::decode(&bytes), Err(ExchangeError::TrailingBytes));
        // A document count of four billion with nothing behind it.
        let mut huge = Vec::from(*REQUEST_MAGIC);
        huge.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        huge.extend_from_slice(&0u32.to_le_bytes()); // empty action
        huge.extend_from_slice(&0u32.to_le_bytes()); // no parameter
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Request::decode(&huge), Err(ExchangeError::Truncated));
        // A length pointing past the end.
        let mut long = Vec::from(*RESPONSE_MAGIC);
        long.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        long.push(TAG_DOCUMENT);
        long.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Response::decode(&long), Err(ExchangeError::Truncated));
        assert_eq!(Response::decode(b"%PDF-1.7"), Err(ExchangeError::BadMagic));
        let mut future = Vec::from(*RESPONSE_MAGIC);
        future.extend_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            Response::decode(&future),
            Err(ExchangeError::UnsupportedVersion(2))
        );
        let mut tag = Response::Error(String::new()).encode().unwrap();
        tag[8] = 9;
        assert_eq!(Response::decode(&tag), Err(ExchangeError::BadTag(9)));
    }

    #[test]
    fn parameter_names_must_be_sorted_and_unique() {
        let mut bytes = Vec::from(*REQUEST_MAGIC);
        bytes.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for _ in 0..2 {
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.push(b'a');
            bytes.extend_from_slice(&[TAG_BOOLEAN, 1]);
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(Request::decode(&bytes), Err(ExchangeError::BadParamOrder));
    }

    #[test]
    fn parses_typed_values() {
        assert_eq!(
            ParamValue::parse(ParamKind::Integer, " 42 "),
            Ok(ParamValue::Integer(42))
        );
        assert!(ParamValue::parse(ParamKind::Integer, "4.2").is_err());
        assert_eq!(
            ParamValue::parse(ParamKind::Boolean, "false"),
            Ok(ParamValue::Boolean(false))
        );
        assert!(ParamValue::parse(ParamKind::Boolean, "oui").is_err());
        assert_eq!(
            ParamValue::parse(ParamKind::Text, " a b "),
            Ok(ParamValue::Text(" a b ".to_string()))
        );
    }
}
