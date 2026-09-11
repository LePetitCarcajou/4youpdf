//! Tokenizer for PDF syntax (ISO 32000-2:2020, clause 7.2).
//!
//! The lexer is byte-oriented: PDF is a binary format and must never be
//! decoded as UTF-8 before tokenizing.

use crate::{Error, Result};

/// A single lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Integer literal.
    Integer(i64),
    /// Real literal.
    Real(f64),
    /// `(...)` string, unescaped.
    LiteralString(Vec<u8>),
    /// `<...>` string, decoded.
    HexString(Vec<u8>),
    /// `/Name`, with `#xx` escapes decoded.
    Name(Vec<u8>),
    /// `[`
    ArrayOpen,
    /// `]`
    ArrayClose,
    /// `<<`
    DictOpen,
    /// `>>`
    DictClose,
    /// `{`
    BraceOpen,
    /// `}`
    BraceClose,
    /// Bare keyword: `obj`, `endobj`, `stream`, `R`, `true`, `null`, ...
    Keyword(Vec<u8>),
    /// End of input.
    Eof,
}

/// Whitespace as defined by ISO 32000-2 table 1.
#[inline]
pub fn is_whitespace(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

/// Delimiters as defined by ISO 32000-2 table 2.
#[inline]
pub fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[inline]
fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Streaming tokenizer over a byte slice.
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Create a lexer positioned at the start of `input`.
    pub fn new(input: &'a [u8]) -> Self {
        Lexer { input, pos: 0 }
    }

    /// Create a lexer positioned at byte `offset`.
    pub fn at(input: &'a [u8], offset: usize) -> Self {
        Lexer {
            input,
            pos: offset.min(input.len()),
        }
    }

    /// Current byte offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Move the cursor (used by the parser after reading raw stream data).
    pub fn seek(&mut self, offset: usize) {
        self.pos = offset.min(self.input.len());
    }

    /// The underlying input.
    pub fn input(&self) -> &'a [u8] {
        self.input
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.input.get(self.pos + ahead).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    /// Skip whitespace and `%` comments.
    pub fn skip_whitespace_and_comments(&mut self) {
        while let Some(b) = self.peek() {
            if is_whitespace(b) {
                self.pos += 1;
            } else if b == b'%' {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// Read the next token.
    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace_and_comments();
        let start = self.pos;
        let Some(b) = self.peek() else {
            return Ok(Token::Eof);
        };
        match b {
            b'[' => {
                self.pos += 1;
                Ok(Token::ArrayOpen)
            }
            b']' => {
                self.pos += 1;
                Ok(Token::ArrayClose)
            }
            b'{' => {
                self.pos += 1;
                Ok(Token::BraceOpen)
            }
            b'}' => {
                self.pos += 1;
                Ok(Token::BraceClose)
            }
            b'<' => {
                if self.peek_at(1) == Some(b'<') {
                    self.pos += 2;
                    Ok(Token::DictOpen)
                } else {
                    self.pos += 1;
                    self.hex_string()
                }
            }
            b'>' => {
                if self.peek_at(1) == Some(b'>') {
                    self.pos += 2;
                    Ok(Token::DictClose)
                } else {
                    Err(Error::Syntax {
                        offset: start,
                        message: "stray '>'".into(),
                    })
                }
            }
            b'(' => {
                self.pos += 1;
                self.literal_string()
            }
            b'/' => {
                self.pos += 1;
                self.name()
            }
            b')' => Err(Error::Syntax {
                offset: start,
                message: "stray ')'".into(),
            }),
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => self.keyword(),
        }
    }

    fn number(&mut self) -> Result<Token> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = &self.input[start..self.pos];
        let s = std::str::from_utf8(text).map_err(|_| Error::Syntax {
            offset: start,
            message: "non-ASCII number".into(),
        })?;
        if !s.contains('.') {
            if let Ok(i) = s.parse::<i64>() {
                return Ok(Token::Integer(i));
            }
        }
        // Tolerant real parsing: PDF allows ".5", "-.002", "34.5", and real
        // files contain oddities like "--5" or "5." which we accept leniently.
        let cleaned: String = s
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        let cleaned = cleaned.trim_start_matches("--").to_string();
        let cleaned = if cleaned.starts_with('.') {
            format!("0{cleaned}")
        } else {
            cleaned
        };
        let cleaned = if cleaned.ends_with('.') {
            format!("{cleaned}0")
        } else {
            cleaned
        };
        cleaned
            .parse::<f64>()
            .map(Token::Real)
            .map_err(|_| Error::Syntax {
                offset: start,
                message: format!("bad number {s:?}"),
            })
    }

    fn keyword(&mut self) -> Result<Token> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            // A delimiter we did not handle above; consume it to guarantee progress.
            self.pos += 1;
            return Err(Error::Syntax {
                offset: start,
                message: "unexpected delimiter".into(),
            });
        }
        Ok(Token::Keyword(self.input[start..self.pos].to_vec()))
    }

    fn name(&mut self) -> Result<Token> {
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if !is_regular(b) {
                break;
            }
            self.pos += 1;
            if b == b'#' {
                let hi = self.peek().and_then(hex_value);
                let lo = self.peek_at(1).and_then(hex_value);
                if let (Some(h), Some(l)) = (hi, lo) {
                    self.pos += 2;
                    out.push((h << 4) | l);
                    continue;
                }
            }
            out.push(b);
        }
        Ok(Token::Name(out))
    }

    fn hex_string(&mut self) -> Result<Token> {
        let start = self.pos;
        let mut out = Vec::new();
        let mut pending: Option<u8> = None;
        loop {
            let Some(b) = self.bump() else {
                return Err(Error::UnexpectedEof);
            };
            if b == b'>' {
                break;
            }
            if is_whitespace(b) {
                continue;
            }
            let Some(v) = hex_value(b) else {
                return Err(Error::Syntax {
                    offset: start,
                    message: "non-hex byte in hex string".into(),
                });
            };
            match pending.take() {
                None => pending = Some(v),
                Some(h) => out.push((h << 4) | v),
            }
        }
        if let Some(h) = pending {
            out.push(h << 4); // odd digit count: final digit is assumed 0
        }
        Ok(Token::HexString(out))
    }

    fn literal_string(&mut self) -> Result<Token> {
        let mut out = Vec::new();
        let mut depth = 1usize;
        loop {
            let Some(b) = self.bump() else {
                return Err(Error::UnexpectedEof);
            };
            match b {
                b'\\' => {
                    let Some(e) = self.bump() else {
                        return Err(Error::UnexpectedEof);
                    };
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'(' => out.push(b'('),
                        b')' => out.push(b')'),
                        b'\\' => out.push(b'\\'),
                        b'\r' => {
                            // line continuation; swallow optional \n
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((v & 0xFF) as u8);
                        }
                        other => out.push(other), // unknown escape: backslash ignored
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b);
                }
                b'\r' => {
                    // EOL inside a string is normalised to \n
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                _ => out.push(b),
            }
        }
        Ok(Token::LiteralString(out))
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn all(input: &[u8]) -> Vec<Token> {
        let mut lx = Lexer::new(input);
        let mut v = Vec::new();
        loop {
            match lx.next_token() {
                Ok(Token::Eof) => break,
                Ok(t) => v.push(t),
                Err(e) => panic!("lexer error: {e}"),
            }
        }
        v
    }

    #[test]
    fn numbers() {
        assert_eq!(
            all(b"42 -17 +3"),
            vec![Token::Integer(42), Token::Integer(-17), Token::Integer(3)]
        );
        assert_eq!(
            all(b"3.5 -.002 .5 6."),
            vec![
                Token::Real(3.5),
                Token::Real(-0.002),
                Token::Real(0.5),
                Token::Real(6.0)
            ]
        );
    }

    #[test]
    fn names_with_escapes() {
        assert_eq!(
            all(b"/Type /A#20B /"),
            vec![
                Token::Name(b"Type".to_vec()),
                Token::Name(b"A B".to_vec()),
                Token::Name(Vec::new()),
            ]
        );
    }

    #[test]
    fn strings() {
        assert_eq!(
            all(b"(hello (nested) \\n\\101)"),
            vec![Token::LiteralString(b"hello (nested) \nA".to_vec())]
        );
        assert_eq!(
            all(b"<48656C6C6F 7>"),
            vec![Token::HexString(b"Hello\x70".to_vec())]
        );
    }

    #[test]
    fn delimiters_and_keywords() {
        assert_eq!(
            all(b"<< /K [1 0 R] >> obj"),
            vec![
                Token::DictOpen,
                Token::Name(b"K".to_vec()),
                Token::ArrayOpen,
                Token::Integer(1),
                Token::Integer(0),
                Token::Keyword(b"R".to_vec()),
                Token::ArrayClose,
                Token::DictClose,
                Token::Keyword(b"obj".to_vec()),
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            all(b"1 % comment\n2"),
            vec![Token::Integer(1), Token::Integer(2)]
        );
    }

    #[test]
    fn never_panics_on_garbage() {
        // Whatever the input, the lexer must return a token or an error, never panic.
        let inputs: &[&[u8]] = &[
            b")",
            b">",
            b"(unterminated",
            b"<zz>",
            b"\xff\xfe\x00",
            b"",
            b"#",
        ];
        for input in inputs {
            let mut lx = Lexer::new(input);
            for _ in 0..8 {
                if matches!(lx.next_token(), Ok(Token::Eof)) {
                    break;
                }
            }
        }
    }
}
