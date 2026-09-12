//! Object parser: turns tokens into [`Object`] values.
//!
//! This handles *direct* objects, `n g R` references and stream bodies whose
//! `/Length` is a direct integer. Resolving indirect `/Length` values and
//! locating objects through the cross-reference table belong to the document
//! layer (next milestone).

use crate::lexer::{Lexer, Token};
use crate::object::{Dict, Name, ObjRef, Object};
use crate::{Error, Result};

/// Maximum nesting depth of arrays/dictionaries. Hostile files can nest
/// millions of `[` to blow the stack; we refuse well before that.
pub const MAX_DEPTH: usize = 256;

/// Recursive-descent parser over a [`Lexer`].
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    peeked: Vec<(Token, usize)>,
}

impl<'a> Parser<'a> {
    /// Parser over the whole input.
    pub fn new(input: &'a [u8]) -> Self {
        Parser {
            lexer: Lexer::new(input),
            peeked: Vec::new(),
        }
    }

    /// Parser starting at `offset`.
    pub fn at(input: &'a [u8], offset: usize) -> Self {
        Parser {
            lexer: Lexer::at(input, offset),
            peeked: Vec::new(),
        }
    }

    fn next_tok(&mut self) -> Result<(Token, usize)> {
        if let Some(t) = self.peeked.pop() {
            return Ok(t);
        }
        let start = {
            self.lexer.skip_whitespace_and_comments();
            self.lexer.pos()
        };
        let tok = self.lexer.next_token()?;
        Ok((tok, start))
    }

    fn push_back(&mut self, t: (Token, usize)) {
        self.peeked.push(t);
    }

    /// Parse one object. Handles `n g R` lookahead.
    pub fn parse_object(&mut self) -> Result<Object> {
        self.parse_with_depth(0)
    }

    fn parse_with_depth(&mut self, depth: usize) -> Result<Object> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        let (tok, offset) = self.next_tok()?;
        match tok {
            Token::Eof => Err(Error::UnexpectedEof),
            Token::Integer(first) => self.maybe_reference(first, offset),
            Token::Real(r) => Ok(Object::Real(r)),
            Token::LiteralString(s) | Token::HexString(s) => Ok(Object::String(s)),
            Token::Name(n) => Ok(Object::Name(Name(n))),
            Token::ArrayOpen => {
                let mut items = Vec::new();
                loop {
                    let (t, o) = self.next_tok()?;
                    match t {
                        Token::ArrayClose => break,
                        Token::Eof => return Err(Error::UnexpectedEof),
                        other => {
                            self.push_back((other, o));
                            items.push(self.parse_with_depth(depth + 1)?);
                        }
                    }
                }
                Ok(Object::Array(items))
            }
            Token::DictOpen => {
                let dict = self.parse_dict_body(depth)?;
                // A dictionary followed by `stream` is a stream object.
                // Tolerance: `stream` glued to its data (no EOL) lexes as a
                // single keyword such as `streamBT`; the body is re-read
                // from raw bytes at `o` anyway.
                let (t, o) = self.next_tok()?;
                if matches!(t, Token::Keyword(ref k) if k.starts_with(b"stream")) {
                    self.parse_stream_body(dict, o)
                } else {
                    self.push_back((t, o));
                    Ok(Object::Dict(dict))
                }
            }
            Token::Keyword(k) => match k.as_slice() {
                b"true" => Ok(Object::Bool(true)),
                b"false" => Ok(Object::Bool(false)),
                b"null" => Ok(Object::Null),
                other => Err(Error::Syntax {
                    offset,
                    message: format!("unexpected keyword {:?}", String::from_utf8_lossy(other)),
                }),
            },
            Token::ArrayClose | Token::DictClose | Token::BraceOpen | Token::BraceClose => {
                Err(Error::Syntax {
                    offset,
                    message: "unexpected delimiter".into(),
                })
            }
        }
    }

    /// After an integer: is it `n g R` (reference) or a plain integer?
    fn maybe_reference(&mut self, first: i64, _offset: usize) -> Result<Object> {
        let second = self.next_tok()?;
        if let (Token::Integer(gen), o2) = (&second.0, second.1) {
            let gen = *gen;
            let third = self.next_tok()?;
            if matches!(third.0, Token::Keyword(ref k) if k == b"R") {
                if !(0..=i64::from(u32::MAX)).contains(&first)
                    || !(0..=i64::from(u16::MAX)).contains(&gen)
                {
                    return Err(Error::Syntax {
                        offset: o2,
                        message: "reference out of range".into(),
                    });
                }
                // Values fit: checked just above.
                let num = u32::try_from(first).unwrap_or(0);
                let gen = u16::try_from(gen).unwrap_or(0);
                return Ok(Object::Reference(ObjRef { num, gen }));
            }
            self.push_back(third);
            self.push_back((Token::Integer(gen), o2));
            return Ok(Object::Integer(first));
        }
        self.push_back(second);
        Ok(Object::Integer(first))
    }

    fn parse_dict_body(&mut self, depth: usize) -> Result<Dict> {
        let mut dict = Dict::new();
        loop {
            let (t, _) = self.next_tok()?;
            match t {
                Token::DictClose => return Ok(dict),
                Token::Eof => return Err(Error::UnexpectedEof),
                Token::Name(key) => {
                    let value = self.parse_with_depth(depth + 1)?;
                    dict.insert(Name(key), value);
                }
                // Tolerance: real-world files contain junk in dictionaries.
                // Skip the token rather than failing the whole document.
                _ => {}
            }
        }
    }

    /// Read raw stream bytes. Requires a direct integer `/Length`; an
    /// indirect `/Length` is resolved at the document layer, so here we fall
    /// back to scanning for `endstream`.
    fn parse_stream_body(&mut self, dict: Dict, keyword_offset: usize) -> Result<Object> {
        let input = self.lexer.input();
        // After `stream` comes CRLF or LF (ISO 32000-2, 7.3.8.1).
        let mut start = keyword_offset + b"stream".len();
        // Tolerance: some writers put spaces before that EOL. Skip them only
        // when an EOL really follows, so data bytes are never swallowed.
        let mut p = start;
        while matches!(input.get(p), Some(b' ' | b'\t')) {
            p += 1;
        }
        if matches!(input.get(p), Some(b'\r' | b'\n')) {
            start = p;
        }
        if input.get(start) == Some(&b'\r') {
            start += 1;
        }
        if input.get(start) == Some(&b'\n') {
            start += 1;
        }
        let declared = dict.get(&Name::new("Length")).and_then(Object::as_i64);
        // Trust the declared length only if `endstream` really follows it,
        // separated at most by one EOL marker: CR, LF or CRLF (ISO 32000-2,
        // 7.3.8.1). Any other byte there is data, so the length is wrong and
        // we scan.
        let end = declared
            .and_then(|len| usize::try_from(len).ok())
            .and_then(|len| start.checked_add(len))
            .filter(|&end| end <= input.len())
            .filter(|&end| {
                let rest = input.get(end..).unwrap_or_default();
                let rest = rest
                    .strip_prefix(b"\r\n")
                    .or_else(|| rest.strip_prefix(b"\n"))
                    .or_else(|| rest.strip_prefix(b"\r"))
                    .unwrap_or(rest);
                rest.starts_with(b"endstream")
            });
        let end = match end {
            Some(e) => e,
            None => {
                let rel = find(&input[start..], b"endstream").ok_or(Error::UnexpectedEof)?;
                let mut e = start + rel;
                // Strip the EOL that precedes `endstream`.
                if e > start && input[e - 1] == b'\n' {
                    e -= 1;
                }
                if e > start && input[e - 1] == b'\r' {
                    e -= 1;
                }
                e
            }
        };
        let data = input[start..end].to_vec();
        // Position the lexer after `endstream`.
        let after = find(&input[end..], b"endstream")
            .map(|i| end + i + b"endstream".len())
            .unwrap_or(input.len());
        self.lexer.seek(after);
        self.peeked.clear();
        Ok(Object::Stream { dict, data })
    }

    /// Parse an indirect object definition `n g obj ... endobj` at the
    /// current position. Returns the reference and the object.
    pub fn parse_indirect(&mut self) -> Result<(ObjRef, Object)> {
        let r = self.parse_indirect_header()?;
        let obj = self.parse_object()?;
        // `endobj` is expected but missing ones are common; tolerate.
        let (t, o) = self.next_tok()?;
        if !matches!(t, Token::Keyword(ref k) if k == b"endobj") {
            self.push_back((t, o));
        }
        Ok((r, obj))
    }

    /// Parse just the `n g obj` header at the current position, leaving
    /// the parser on the object that follows. Cheap: three tokens.
    pub fn parse_indirect_header(&mut self) -> Result<ObjRef> {
        let (t1, o1) = self.next_tok()?;
        let (t2, _) = self.next_tok()?;
        let (t3, _) = self.next_tok()?;
        let (num, gen) = match (t1, t2, t3) {
            (Token::Integer(n), Token::Integer(g), Token::Keyword(k)) if k == b"obj" => (n, g),
            _ => {
                return Err(Error::Syntax {
                    offset: o1,
                    message: "expected `n g obj`".into(),
                })
            }
        };
        let num = u32::try_from(num).map_err(|_| Error::Syntax {
            offset: o1,
            message: "bad object number".into(),
        })?;
        let gen = u16::try_from(gen).map_err(|_| Error::Syntax {
            offset: o1,
            message: "bad generation".into(),
        })?;
        Ok(ObjRef { num, gen })
    }

    /// Byte offset of the next token to be read: after the last consumed
    /// token, or at a token that was peeked and pushed back.
    pub fn pos(&self) -> usize {
        self.peeked
            .last()
            .map_or_else(|| self.lexer.pos(), |(_, offset)| *offset)
    }
}

/// Byte-slice search. Returns the index of the first occurrence.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn parse(input: &[u8]) -> Object {
        Parser::new(input).parse_object().expect("parse")
    }

    #[test]
    fn scalars() {
        assert_eq!(parse(b"true"), Object::Bool(true));
        assert_eq!(parse(b"null"), Object::Null);
        assert_eq!(parse(b"12"), Object::Integer(12));
        assert_eq!(parse(b"-1.5"), Object::Real(-1.5));
        assert_eq!(parse(b"/Name"), Object::Name(Name::new("Name")));
        assert_eq!(parse(b"(str)"), Object::String(b"str".to_vec()));
    }

    #[test]
    fn references_vs_integers() {
        assert_eq!(
            parse(b"12 0 R"),
            Object::Reference(ObjRef { num: 12, gen: 0 })
        );
        assert_eq!(
            parse(b"[1 2 3]"),
            Object::Array(vec![
                Object::Integer(1),
                Object::Integer(2),
                Object::Integer(3)
            ])
        );
        assert_eq!(
            parse(b"[1 2 R 3]"),
            Object::Array(vec![
                Object::Reference(ObjRef { num: 1, gen: 2 }),
                Object::Integer(3)
            ])
        );
    }

    #[test]
    fn dictionary() {
        let obj = parse(b"<< /Type /Page /Count 3 /Kids [4 0 R] >>");
        let d = obj.as_dict().expect("dict");
        assert_eq!(
            d.get(&Name::new("Type")).and_then(Object::as_name),
            Some(&Name::new("Page"))
        );
        assert_eq!(d.get(&Name::new("Count")).and_then(Object::as_i64), Some(3));
        assert!(matches!(d.get(&Name::new("Kids")), Some(Object::Array(_))));
    }

    #[test]
    fn stream_with_correct_length() {
        let obj = parse(b"<< /Length 5 >>\nstream\nhello\nendstream");
        match obj {
            Object::Stream { dict, data } => {
                assert_eq!(
                    dict.get(&Name::new("Length")).and_then(Object::as_i64),
                    Some(5)
                );
                assert_eq!(data, b"hello");
            }
            other => panic!("expected stream, got {other:?}"),
        }
    }

    #[test]
    fn stream_with_wrong_length_recovers() {
        let obj = parse(b"<< /Length 999 >>\r\nstream\r\nabc\r\nendstream");
        match obj {
            Object::Stream { data, .. } => assert_eq!(data, b"abc"),
            other => panic!("expected stream, got {other:?}"),
        }
    }

    #[test]
    fn stream_with_slightly_short_length_is_not_truncated() {
        // `/Length 4` lands on `o`, two bytes before `endstream`: those bytes
        // are data, not an EOL, so the length must be rejected.
        let obj = parse(b"<< /Length 4 >>\nstream\nhello\nendstream");
        match obj {
            Object::Stream { data, .. } => assert_eq!(data, b"hello"),
            other => panic!("expected stream, got {other:?}"),
        }
    }

    #[test]
    fn stream_keyword_followed_by_spaces_before_eol() {
        let obj = parse(b"<< /Length 5 >>\nstream \t\r\nhello\nendstream");
        match obj {
            Object::Stream { data, .. } => assert_eq!(data, b"hello"),
            other => panic!("expected stream, got {other:?}"),
        }
    }

    #[test]
    fn stream_keyword_glued_to_data() {
        // No EOL after `stream`: the lexer sees a single `streamabc` keyword.
        let obj = parse(b"<< /Length 3 >>\nstreamabc\nendstream");
        match obj {
            Object::Stream { data, .. } => assert_eq!(data, b"abc"),
            other => panic!("expected stream, got {other:?}"),
        }
    }

    #[test]
    fn indirect_object() {
        let (r, obj) = Parser::new(b"7 0 obj\n<< /A 1 >>\nendobj")
            .parse_indirect()
            .expect("indirect");
        assert_eq!(r, ObjRef { num: 7, gen: 0 });
        assert!(obj.as_dict().is_some());
    }

    #[test]
    fn depth_limit_is_enforced() {
        let deep = vec![b'['; MAX_DEPTH + 10];
        assert_eq!(Parser::new(&deep).parse_object(), Err(Error::TooDeep));
    }

    #[test]
    fn errors_not_panics() {
        for input in [&b"<<"[..], b"[", b"]", b">>", b"12 0", b"", b"foo"] {
            let _ = Parser::new(input).parse_object();
        }
    }
}
