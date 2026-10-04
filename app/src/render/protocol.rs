//! What the window and the rendering worker say to each other (ADR 0008),
//! over the standard input and output of the worker. Nothing here knows
//! PDFium.
//!
//! A frame is one byte that names it, the length of its payload on four
//! bytes, then the payload; every integer is little-endian. The window
//! writes [`Request`]s and reads [`Reply`]s, one reply per request, in
//! order. The first request is [`Request::Hello`], answered by
//! [`Reply::Ready`], or by [`Reply::Refused`] when the worker speaks
//! another [`VERSION`].
//!
//! The window does not trust the worker: [`read_reply`] compares every
//! length it is told with a ceiling before it allocates anything, and
//! checks the dimensions of an image against its length before it reads
//! the pixels.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// Version of this protocol: a peer that speaks another one is refused.
pub const VERSION: u32 = 1;

/// What a [`Request::Hello`] starts with.
pub const MAGIC: [u8; 4] = *b"FYPR";

/// Narrowest image asked for, in pixels.
pub const MIN_WIDTH: u32 = 16;

/// Widest image asked for, in pixels.
pub const MAX_WIDTH: u32 = 4096;

/// Tallest image accepted, in pixels: four times the widest.
pub const MAX_HEIGHT: u32 = 16_384;

/// Most pixels accepted in an image: a page twice as tall as wide at
/// [`MAX_WIDTH`], 128 MiB of RGBA. An A4 page at that width has 4096 × 5793
/// pixels, 95 MiB.
pub const MAX_PIXELS: u64 = 4096 * 8192;

/// Longest text in a frame, in bytes: a status or the reason of a failure.
pub const MAX_TEXT: usize = 8 * 1024;

/// Longest [`Request::Hello`], in bytes.
const MAX_HELLO: u32 = 64 * 1024;

/// Tag and length.
const HEADER: usize = 5;

/// Sequence, width and height, before the pixels of an image.
const BITMAP_HEAD: u32 = 16;

/// Sequence, document and length of the password, before the password and
/// the bytes of a document.
const DOCUMENT_HEAD: usize = 20;

const HELLO: u8 = 0x01;
const DOCUMENT: u8 = 0x02;
const DRAW: u8 = 0x03;
const READY: u8 = 0x81;
const REFUSED: u8 = 0x82;
const OPENED: u8 = 0x83;
const BITMAP: u8 = 0x84;
const FAILED: u8 = 0x85;

/// What the window asks of the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// The handshake, first and once.
    Hello {
        /// The [`VERSION`] the window speaks.
        version: u32,
        /// The directories where to look for the PDFium library, in order.
        candidates: Vec<PathBuf>,
    },
    /// A document to load in place of the one the worker holds. The
    /// password travels here and nowhere else.
    Document {
        /// Echoed by the reply.
        sequence: u64,
        /// What [`Request::Draw`] will call this document.
        id: u64,
        /// Empty for a file that needs none.
        password: String,
        /// The file.
        bytes: Vec<u8>,
    },
    /// A page of the document loaded to draw.
    Draw {
        /// Echoed by the reply.
        sequence: u64,
        /// The document meant.
        id: u64,
        /// The page, 0-based.
        page: u32,
        /// Width of the image, in pixels.
        width: u32,
    },
}

/// What the worker answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The handshake is accepted.
    Ready {
        /// The [`VERSION`] the worker speaks.
        version: u32,
        /// Whether PDFium is bound; a worker that has none exits.
        available: bool,
        /// Where the library was found, or where it was looked for.
        detail: String,
    },
    /// The handshake is refused: the worker speaks another version, and
    /// exits.
    Refused {
        /// The [`VERSION`] the worker speaks.
        version: u32,
    },
    /// The document of a [`Request::Document`] is loaded.
    Opened {
        /// The sequence of the request.
        sequence: u64,
    },
    /// The page of a [`Request::Draw`], as RGBA, 8 bits per channel, row
    /// by row from the top.
    Bitmap {
        /// The sequence of the request.
        sequence: u64,
        /// Width of the image, in pixels.
        width: u32,
        /// Height of the image, in pixels.
        height: u32,
        /// `width` × `height` × 4 bytes.
        pixels: Vec<u8>,
    },
    /// The request could not be served; the worker goes on.
    Failed {
        /// The sequence of the request.
        sequence: u64,
        /// Why, in words for the user.
        message: String,
    },
}

/// Why a frame was not read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The other side closed its end between two frames.
    Closed,
    /// The other side closed its end in the middle of a frame.
    Truncated,
    /// Reading or writing failed otherwise.
    Io(String),
    /// A frame announces more than its kind may hold.
    TooLong {
        /// The byte that names the frame.
        tag: u8,
        /// The length it announces.
        length: u64,
        /// The most it may hold.
        ceiling: u64,
    },
    /// A frame of a kind this side does not read.
    UnknownFrame(u8),
    /// A frame whose payload is not what its kind holds.
    Malformed(&'static str),
    /// An image whose dimensions are out of bounds or do not match its
    /// length.
    Dimensions {
        /// The width it announces.
        width: u32,
        /// The height it announces.
        height: u32,
        /// The length of its frame.
        length: u32,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Closed => write!(f, "canal fermé"),
            ProtocolError::Truncated => write!(f, "trame tronquée"),
            ProtocolError::Io(e) => write!(f, "erreur du canal : {e}"),
            ProtocolError::TooLong {
                tag,
                length,
                ceiling,
            } => write!(f, "trame {tag:#04x} de {length} octets, plafond {ceiling}"),
            ProtocolError::UnknownFrame(tag) => write!(f, "trame inconnue {tag:#04x}"),
            ProtocolError::Malformed(what) => write!(f, "trame mal formée : {what}"),
            ProtocolError::Dimensions {
                width,
                height,
                length,
            } => write!(
                f,
                "image annoncée de {width} × {height} pixels dans une trame de {length} octets"
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(e: io::Error) -> ProtocolError {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            ProtocolError::Truncated
        } else {
            ProtocolError::Io(e.to_string())
        }
    }
}

/// `text`, cut to [`MAX_TEXT`] bytes at most, between two characters.
pub fn clip(text: &str) -> &str {
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

impl Request {
    /// Write the request as one frame. The window sends a document with
    /// [`document_head`] instead, which does not copy its bytes.
    pub fn write_to(&self, out: &mut impl Write) -> Result<(), ProtocolError> {
        match self {
            Request::Hello {
                version,
                candidates,
            } => out.write_all(&hello_frame(*version, candidates)?)?,
            Request::Document {
                sequence,
                id,
                password,
                bytes,
            } => {
                out.write_all(&document_head(*sequence, *id, password, bytes.len())?)?;
                out.write_all(bytes)?;
            }
            Request::Draw {
                sequence,
                id,
                page,
                width,
            } => out.write_all(&draw_frame(*sequence, *id, *page, *width))?,
        }
        out.flush()?;
        Ok(())
    }
}

/// The frame of a [`Request::Hello`].
pub fn hello_frame(version: u32, candidates: &[PathBuf]) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&MAGIC);
    payload.extend_from_slice(&version.to_le_bytes());
    payload
        .extend_from_slice(&length(HELLO, candidates.len(), u64::from(MAX_HELLO))?.to_le_bytes());
    for candidate in candidates {
        let bytes = path_bytes(candidate);
        payload.extend_from_slice(&length(HELLO, bytes.len(), u64::from(MAX_HELLO))?.to_le_bytes());
        payload.extend_from_slice(&bytes);
    }
    let announced = length(HELLO, payload.len(), u64::from(MAX_HELLO))?;
    Ok(frame(HELLO, announced, &payload))
}

/// The frame of a [`Request::Draw`].
pub fn draw_frame(sequence: u64, id: u64, page: u32, width: u32) -> Vec<u8> {
    let mut payload = [0_u8; 24];
    let (head, tail) = payload.split_at_mut(16);
    let (first, second) = head.split_at_mut(8);
    first.copy_from_slice(&sequence.to_le_bytes());
    second.copy_from_slice(&id.to_le_bytes());
    let (third, fourth) = tail.split_at_mut(4);
    third.copy_from_slice(&page.to_le_bytes());
    fourth.copy_from_slice(&width.to_le_bytes());
    frame(DRAW, 24, &payload)
}

/// The start of the frame of a [`Request::Document`] of `size` bytes: all
/// of it but the bytes of the document, which follow as they are. Refused
/// when the frame would not fit the four bytes of its length.
pub fn document_head(
    sequence: u64,
    id: u64,
    password: &str,
    size: usize,
) -> Result<Vec<u8>, ProtocolError> {
    let ceiling = u64::from(u32::MAX);
    let total = (DOCUMENT_HEAD as u64)
        .saturating_add(password.len() as u64)
        .saturating_add(size as u64);
    let announced = u32::try_from(total).map_err(|_| ProtocolError::TooLong {
        tag: DOCUMENT,
        length: total,
        ceiling,
    })?;
    let mut head = Vec::with_capacity(HEADER + DOCUMENT_HEAD + password.len());
    head.push(DOCUMENT);
    head.extend_from_slice(&announced.to_le_bytes());
    head.extend_from_slice(&sequence.to_le_bytes());
    head.extend_from_slice(&id.to_le_bytes());
    head.extend_from_slice(&length(DOCUMENT, password.len(), ceiling)?.to_le_bytes());
    head.extend_from_slice(password.as_bytes());
    Ok(head)
}

impl Reply {
    /// Write the reply as one frame. A text longer than [`MAX_TEXT`] is cut.
    pub fn write_to(&self, out: &mut impl Write) -> Result<(), ProtocolError> {
        match self {
            Reply::Ready {
                version,
                available,
                detail,
            } => {
                let detail = clip(detail).as_bytes();
                let mut payload = Vec::with_capacity(5 + detail.len());
                payload.extend_from_slice(&version.to_le_bytes());
                payload.push(u8::from(*available));
                payload.extend_from_slice(detail);
                let announced = length(READY, payload.len(), u64::from(u32::MAX))?;
                out.write_all(&frame(READY, announced, &payload))?;
            }
            Reply::Refused { version } => {
                out.write_all(&frame(REFUSED, 4, &version.to_le_bytes()))?;
            }
            Reply::Opened { sequence } => {
                out.write_all(&frame(OPENED, 8, &sequence.to_le_bytes()))?;
            }
            Reply::Bitmap {
                sequence,
                width,
                height,
                pixels,
            } => {
                let ceiling = u64::from(u32::MAX);
                let total = u64::from(BITMAP_HEAD).saturating_add(pixels.len() as u64);
                let announced = u32::try_from(total).map_err(|_| ProtocolError::TooLong {
                    tag: BITMAP,
                    length: total,
                    ceiling,
                })?;
                let mut head = Vec::with_capacity(HEADER + BITMAP_HEAD as usize);
                head.push(BITMAP);
                head.extend_from_slice(&announced.to_le_bytes());
                head.extend_from_slice(&sequence.to_le_bytes());
                head.extend_from_slice(&width.to_le_bytes());
                head.extend_from_slice(&height.to_le_bytes());
                out.write_all(&head)?;
                out.write_all(pixels)?;
            }
            Reply::Failed { sequence, message } => {
                let message = clip(message).as_bytes();
                let mut payload = Vec::with_capacity(8 + message.len());
                payload.extend_from_slice(&sequence.to_le_bytes());
                payload.extend_from_slice(message);
                let announced = length(FAILED, payload.len(), u64::from(u32::MAX))?;
                out.write_all(&frame(FAILED, announced, &payload))?;
            }
        }
        out.flush()?;
        Ok(())
    }
}

/// Read one reply of the worker, which is not trusted: the length of the
/// frame is compared with the ceiling of its kind before anything is
/// allocated, and an image must have dimensions within bounds that give
/// exactly its length before its pixels are read.
pub fn read_reply(input: &mut impl Read) -> Result<Reply, ProtocolError> {
    let Some((tag, announced)) = read_header(input)? else {
        return Err(ProtocolError::Closed);
    };
    let text = MAX_TEXT as u64;
    let ceiling = match tag {
        READY => 5 + text,
        REFUSED => 4,
        OPENED => 8,
        BITMAP => u64::from(BITMAP_HEAD) + MAX_PIXELS * 4,
        FAILED => 8 + text,
        other => return Err(ProtocolError::UnknownFrame(other)),
    };
    if u64::from(announced) > ceiling {
        return Err(ProtocolError::TooLong {
            tag,
            length: u64::from(announced),
            ceiling,
        });
    }
    if tag == BITMAP {
        return read_bitmap(input, announced);
    }
    let payload = read_payload(input, announced)?;
    let mut fields = Fields(&payload);
    let reply = match tag {
        READY => Reply::Ready {
            version: fields.u32()?,
            available: match fields.u8()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::Malformed("disponibilité")),
            },
            detail: String::from_utf8_lossy(fields.rest()).into_owned(),
        },
        REFUSED => Reply::Refused {
            version: fields.u32()?,
        },
        OPENED => Reply::Opened {
            sequence: fields.u64()?,
        },
        _ => Reply::Failed {
            sequence: fields.u64()?,
            message: String::from_utf8_lossy(fields.rest()).into_owned(),
        },
    };
    fields.end()?;
    Ok(reply)
}

/// The image of a frame that announces `announced` bytes, already known to
/// be under the ceiling of an image.
fn read_bitmap(input: &mut impl Read, announced: u32) -> Result<Reply, ProtocolError> {
    if announced < BITMAP_HEAD {
        return Err(ProtocolError::Malformed("image sans dimensions"));
    }
    let mut head = [0_u8; BITMAP_HEAD as usize];
    input.read_exact(&mut head)?;
    let mut fields = Fields(&head);
    let sequence = fields.u64()?;
    let width = fields.u32()?;
    let height = fields.u32()?;
    let pixels = u64::from(width) * u64::from(height);
    let wrong = width == 0
        || height == 0
        || width > MAX_WIDTH
        || height > MAX_HEIGHT
        || pixels > MAX_PIXELS
        || u64::from(announced) != u64::from(BITMAP_HEAD) + pixels * 4;
    if wrong {
        return Err(ProtocolError::Dimensions {
            width,
            height,
            length: announced,
        });
    }
    Ok(Reply::Bitmap {
        sequence,
        width,
        height,
        pixels: read_payload(input, announced - BITMAP_HEAD)?,
    })
}

/// Read one request of the window, or `None` when the window closed its
/// end between two frames: the worker then stops.
pub fn read_request(input: &mut impl Read) -> Result<Option<Request>, ProtocolError> {
    let Some((tag, announced)) = read_header(input)? else {
        return Ok(None);
    };
    let ceiling = match tag {
        HELLO => u64::from(MAX_HELLO),
        DOCUMENT => u64::from(u32::MAX),
        DRAW => 24,
        other => return Err(ProtocolError::UnknownFrame(other)),
    };
    if u64::from(announced) > ceiling {
        return Err(ProtocolError::TooLong {
            tag,
            length: u64::from(announced),
            ceiling,
        });
    }
    let payload = read_payload(input, announced)?;
    let mut fields = Fields(&payload);
    let request = match tag {
        HELLO => {
            if fields.bytes(MAGIC.len())? != MAGIC {
                return Err(ProtocolError::Malformed("poignée de main"));
            }
            let version = fields.u32()?;
            let mut candidates = Vec::new();
            // A window of another version may lay out the rest otherwise:
            // the version alone is enough to refuse it.
            if version == VERSION {
                for _ in 0..fields.u32()? {
                    let size = fields.u32()? as usize;
                    candidates.push(path_from_bytes(fields.bytes(size)?)?);
                }
                fields.end()?;
            }
            Request::Hello {
                version,
                candidates,
            }
        }
        DOCUMENT => {
            let sequence = fields.u64()?;
            let id = fields.u64()?;
            let size = fields.u32()? as usize;
            let password = String::from_utf8(fields.bytes(size)?.to_vec())
                .map_err(|_| ProtocolError::Malformed("mot de passe"))?;
            let head = DOCUMENT_HEAD + size;
            let mut bytes = payload;
            bytes.drain(..head);
            return Ok(Some(Request::Document {
                sequence,
                id,
                password,
                bytes,
            }));
        }
        _ => {
            let request = Request::Draw {
                sequence: fields.u64()?,
                id: fields.u64()?,
                page: fields.u32()?,
                width: fields.u32()?,
            };
            fields.end()?;
            request
        }
    };
    Ok(Some(request))
}

/// A whole frame: its tag, the length it announces and its payload.
fn frame(tag: u8, announced: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER + payload.len());
    frame.push(tag);
    frame.extend_from_slice(&announced.to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// `size` as a length on four bytes, for a frame of kind `tag`.
fn length(tag: u8, size: usize, ceiling: u64) -> Result<u32, ProtocolError> {
    u32::try_from(size)
        .ok()
        .filter(|size| u64::from(*size) <= ceiling)
        .ok_or(ProtocolError::TooLong {
            tag,
            length: size as u64,
            ceiling,
        })
}

/// The tag and the length of the next frame, or `None` when the other side
/// closed its end before the first byte of one.
fn read_header(input: &mut impl Read) -> Result<Option<(u8, u32)>, ProtocolError> {
    let mut tag = [0_u8; 1];
    loop {
        match input.read(&mut tag) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let mut announced = [0_u8; 4];
    input.read_exact(&mut announced)?;
    Ok(Some((tag[0], u32::from_le_bytes(announced))))
}

/// The `announced` bytes of a payload, `announced` being under a ceiling
/// already. The buffer grows with what arrives, never ahead of it by more
/// than it holds: a frame that announces much and brings little costs
/// little.
fn read_payload(input: &mut impl Read, announced: u32) -> Result<Vec<u8>, ProtocolError> {
    let mut payload = Vec::new();
    let read = input
        .take(u64::from(announced))
        .read_to_end(&mut payload)
        .map_err(ProtocolError::from)?;
    if read as u64 != u64::from(announced) {
        return Err(ProtocolError::Truncated);
    }
    Ok(payload)
}

/// The fields of a payload, read in order; nothing here can panic on what
/// the payload holds.
struct Fields<'a>(&'a [u8]);

impl<'a> Fields<'a> {
    fn bytes(&mut self, size: usize) -> Result<&'a [u8], ProtocolError> {
        let (head, tail) = self
            .0
            .split_at_checked(size)
            .ok_or(ProtocolError::Malformed("trame trop courte"))?;
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(u8::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        self.bytes(N)?
            .try_into()
            .map_err(|_| ProtocolError::Malformed("trame trop courte"))
    }

    fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }

    fn end(&self) -> Result<(), ProtocolError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(ProtocolError::Malformed("trame trop longue"))
        }
    }
}

/// A path as the bytes of a frame: the 16-bit units of Windows, low byte
/// first, so that a name that is not Unicode travels as it is.
#[cfg(windows)]
fn path_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, ProtocolError> {
    use std::os::windows::ffi::OsStringExt;
    let (units, odd) = bytes.as_chunks::<2>();
    if !odd.is_empty() {
        return Err(ProtocolError::Malformed("chemin"));
    }
    let wide: Vec<u16> = units.iter().map(|unit| u16::from_le_bytes(*unit)).collect();
    Ok(std::ffi::OsString::from_wide(&wide).into())
}

/// A path as the bytes of a frame: its own bytes.
#[cfg(unix)]
fn path_bytes(path: &std::path::Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, ProtocolError> {
    use std::os::unix::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_vec(bytes.to_vec()).into())
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A reader that counts what is taken from it.
    struct Counted<'a> {
        bytes: &'a [u8],
        taken: usize,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.bytes.read(buffer)?;
            self.taken += read;
            Ok(read)
        }
    }

    fn written(reply: &Reply) -> Vec<u8> {
        let mut out = Vec::new();
        reply.write_to(&mut out).unwrap();
        out
    }

    fn reply_of(bytes: &[u8]) -> Result<Reply, ProtocolError> {
        read_reply(&mut &bytes[..])
    }

    /// A frame of kind `tag` that announces `announced` bytes and brings
    /// `payload`.
    fn raw(tag: u8, announced: u32, payload: &[u8]) -> Vec<u8> {
        frame(tag, announced, payload)
    }

    /// The payload of an image frame: sequence 7, the dimensions, `pixels`
    /// bytes.
    fn bitmap(width: u32, height: u32, pixels: usize) -> Vec<u8> {
        let mut payload = 7_u64.to_le_bytes().to_vec();
        payload.extend_from_slice(&width.to_le_bytes());
        payload.extend_from_slice(&height.to_le_bytes());
        payload.resize(payload.len() + pixels, 0x55);
        payload
    }

    #[test]
    fn every_request_reads_back_as_written() {
        let requests = [
            Request::Hello {
                version: VERSION,
                candidates: vec![
                    PathBuf::from("C:/Program Files/4YouPDF"),
                    PathBuf::from("é"),
                ],
            },
            Request::Hello {
                version: VERSION,
                candidates: Vec::new(),
            },
            Request::Document {
                sequence: 3,
                id: 12,
                password: "pässword".into(),
                bytes: b"%PDF-1.7 not really".to_vec(),
            },
            Request::Document {
                sequence: 4,
                id: 13,
                password: String::new(),
                bytes: Vec::new(),
            },
            Request::Draw {
                sequence: u64::MAX,
                id: 12,
                page: 41,
                width: 1400,
            },
        ];
        let mut stream = Vec::new();
        for request in &requests {
            request.write_to(&mut stream).unwrap();
        }
        let mut input = &stream[..];
        for request in &requests {
            assert_eq!(read_request(&mut input).unwrap().as_ref(), Some(request));
        }
        assert_eq!(read_request(&mut input).unwrap(), None);
    }

    #[test]
    fn every_reply_reads_back_as_written() {
        let replies = [
            Reply::Ready {
                version: VERSION,
                available: true,
                detail: "PDFium chargé depuis là".into(),
            },
            Reply::Ready {
                version: VERSION,
                available: false,
                detail: String::new(),
            },
            Reply::Refused { version: 9 },
            Reply::Opened { sequence: 5 },
            Reply::Bitmap {
                sequence: 6,
                width: 3,
                height: 2,
                pixels: (0..24).collect(),
            },
            Reply::Failed {
                sequence: 7,
                message: "page 10 hors de portée".into(),
            },
        ];
        let mut stream = Vec::new();
        for reply in &replies {
            reply.write_to(&mut stream).unwrap();
        }
        let mut input = &stream[..];
        for reply in &replies {
            assert_eq!(&read_reply(&mut input).unwrap(), reply);
        }
        assert_eq!(read_reply(&mut input), Err(ProtocolError::Closed));
    }

    /// The largest image the window accepts goes through, and an A4 page at
    /// the widest too.
    #[test]
    fn the_ceilings_admit_an_a4_page_at_the_widest() {
        const { assert!(MAX_WIDTH as u64 * 5793 <= MAX_PIXELS) };
        const { assert!(5793 <= MAX_HEIGHT) };
        let page = Reply::Bitmap {
            sequence: 1,
            width: MAX_WIDTH,
            height: 5793,
            pixels: vec![0; MAX_WIDTH as usize * 5793 * 4],
        };
        assert_eq!(reply_of(&written(&page)).unwrap(), page);
    }

    #[test]
    fn a_long_text_is_cut_between_two_characters() {
        let long = "é".repeat(MAX_TEXT);
        let cut = clip(&long);
        assert_eq!(cut.len(), MAX_TEXT);
        assert!(long.starts_with(cut));
        assert_eq!(clip(&format!("a{long}")).len(), MAX_TEXT - 1);
        let Reply::Failed { message, .. } = reply_of(&written(&Reply::Failed {
            sequence: 1,
            message: long.clone(),
        }))
        .unwrap() else {
            panic!("not a failure");
        };
        assert_eq!(message, cut);
    }

    /// A frame that announces more than its kind may hold is refused on its
    /// five first bytes: nothing more is read, so nothing is allocated for
    /// it.
    #[test]
    fn a_huge_length_is_refused_before_anything_else_is_read() {
        for (tag, announced) in [
            (BITMAP, u32::MAX),
            (BITMAP, (u64::from(BITMAP_HEAD) + MAX_PIXELS * 4 + 1) as u32),
            (FAILED, 8 + MAX_TEXT as u32 + 1),
            (READY, 1 << 30),
            (OPENED, 9),
            (REFUSED, 5),
        ] {
            let mut bytes = raw(tag, announced, &[]);
            bytes.resize(bytes.len() + 64, 0);
            let mut input = Counted {
                bytes: &bytes,
                taken: 0,
            };
            assert!(
                matches!(read_reply(&mut input), Err(ProtocolError::TooLong { tag: t, .. }) if t == tag),
                "{tag:#04x} {announced}"
            );
            assert_eq!(input.taken, HEADER, "{tag:#04x} {announced}");
        }
    }

    /// An image whose dimensions are wrong is refused on its sixteen bytes
    /// of dimensions, before its pixels are read.
    #[test]
    fn wrong_dimensions_are_refused_before_the_pixels_are_read() {
        let cases: [(u32, u32, usize, u32); 9] = [
            // Width, height, pixels brought, length announced.
            (0, 10, 0, 16),
            (10, 0, 0, 16),
            (MAX_WIDTH + 1, 1, 64, 16 + (MAX_WIDTH + 1) * 4),
            (1, MAX_HEIGHT + 1, 64, 16 + (MAX_HEIGHT + 1) * 4),
            // Each side within bounds, too many pixels together.
            (MAX_WIDTH, MAX_HEIGHT, 64, 16 + 64),
            // The length is not width × height × 4.
            (10, 10, 396, 16 + 396),
            (10, 10, 404, 16 + 404),
            // A product that wraps on 32 bits.
            (u32::MAX, u32::MAX, 4, 16 + 4),
            (65_536, 65_536, 0, 16),
        ];
        for (width, height, pixels, announced) in cases {
            let bytes = raw(BITMAP, announced, &bitmap(width, height, pixels));
            let mut input = Counted {
                bytes: &bytes,
                taken: 0,
            };
            assert_eq!(
                read_reply(&mut input),
                Err(ProtocolError::Dimensions {
                    width,
                    height,
                    length: announced
                }),
                "{width} × {height}"
            );
            assert_eq!(input.taken, HEADER + 16, "{width} × {height}");
        }
        assert_eq!(
            reply_of(&raw(BITMAP, 15, &[0; 15])),
            Err(ProtocolError::Malformed("image sans dimensions"))
        );
    }

    /// A frame cut anywhere is an error, and what was allocated for it is
    /// what it brought: an image that announces 128 MiB and stops after its
    /// dimensions costs nothing.
    #[test]
    fn a_truncated_frame_is_an_error_wherever_it_stops() {
        let whole = written(&Reply::Bitmap {
            sequence: 1,
            width: 4,
            height: 4,
            pixels: vec![9; 64],
        });
        for cut in 1..whole.len() {
            assert_eq!(
                reply_of(&whole[..cut]),
                Err(ProtocolError::Truncated),
                "cut at {cut}"
            );
        }
        let whole = written(&Reply::Failed {
            sequence: 1,
            message: "non".into(),
        });
        for cut in 1..whole.len() {
            assert_eq!(reply_of(&whole[..cut]), Err(ProtocolError::Truncated));
        }
        assert_eq!(reply_of(&[]), Err(ProtocolError::Closed));
        let largest = raw(
            BITMAP,
            (u64::from(BITMAP_HEAD) + MAX_PIXELS * 4) as u32,
            &bitmap(MAX_WIDTH, 8192, 0),
        );
        assert_eq!(reply_of(&largest), Err(ProtocolError::Truncated));
    }

    #[test]
    fn frames_of_the_wrong_kind_or_shape_are_errors() {
        // The window does not read requests, nor the worker replies.
        assert_eq!(
            reply_of(&draw_frame(1, 1, 0, 100)),
            Err(ProtocolError::UnknownFrame(DRAW))
        );
        assert_eq!(
            read_request(&mut &written(&Reply::Opened { sequence: 1 })[..]),
            Err(ProtocolError::UnknownFrame(OPENED))
        );
        assert_eq!(
            reply_of(&[0x00, 0, 0, 0, 0]),
            Err(ProtocolError::UnknownFrame(0))
        );
        // Too short, too long, a flag that is neither 0 nor 1.
        assert!(matches!(
            reply_of(&raw(OPENED, 4, &[0; 4])),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            reply_of(&raw(REFUSED, 2, &[0; 2])),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            reply_of(&raw(READY, 5, &[1, 0, 0, 0, 2])),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            reply_of(&raw(FAILED, 3, &[0; 3])),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            read_request(&mut &raw(DRAW, 20, &[0; 20])[..]),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            read_request(&mut &raw(DRAW, 25, &[0; 25])[..]),
            Err(ProtocolError::TooLong { .. })
        ));
        // A text that is not UTF-8 is read all the same, never trusted.
        let Reply::Failed { message, .. } =
            reply_of(&raw(FAILED, 10, &[0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xfe])).unwrap()
        else {
            panic!("not a failure");
        };
        assert_eq!(message, "\u{fffd}\u{fffd}");
    }

    /// The handshake: a hello that does not start as one is an error, and
    /// one of another version is read as far as its version, which is
    /// enough to refuse it.
    #[test]
    fn a_hello_of_another_version_is_read_as_far_as_its_version() {
        let mut payload = MAGIC.to_vec();
        payload.extend_from_slice(&(VERSION + 1).to_le_bytes());
        payload.extend_from_slice(b"whatever a later version puts here");
        let bytes = raw(HELLO, payload.len() as u32, &payload);
        assert_eq!(
            read_request(&mut &bytes[..]).unwrap(),
            Some(Request::Hello {
                version: VERSION + 1,
                candidates: Vec::new()
            })
        );
        let mut wrong = b"HTTP".to_vec();
        wrong.extend_from_slice(&VERSION.to_le_bytes());
        wrong.extend_from_slice(&0_u32.to_le_bytes());
        assert_eq!(
            read_request(&mut &raw(HELLO, 12, &wrong)[..]),
            Err(ProtocolError::Malformed("poignée de main"))
        );
        // A count of candidates that the frame does not hold.
        let mut lying = MAGIC.to_vec();
        lying.extend_from_slice(&VERSION.to_le_bytes());
        lying.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            read_request(&mut &raw(HELLO, 12, &lying)[..]),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            read_request(&mut &raw(HELLO, MAX_HELLO + 1, &[])[..]),
            Err(ProtocolError::TooLong { .. })
        ));
    }

    #[test]
    fn a_document_too_large_for_a_frame_is_refused_when_written() {
        assert!(document_head(1, 1, "", u32::MAX as usize - DOCUMENT_HEAD).is_ok());
        if usize::BITS > 32 {
            assert!(matches!(
                document_head(1, 1, "", u32::MAX as usize - DOCUMENT_HEAD + 1),
                Err(ProtocolError::TooLong { tag: DOCUMENT, .. })
            ));
            assert!(matches!(
                document_head(1, 1, "p", u32::MAX as usize - DOCUMENT_HEAD),
                Err(ProtocolError::TooLong { tag: DOCUMENT, .. })
            ));
        }
    }

    /// Random bytes, from a fixed seed: every stream ends in an error,
    /// without a panic, and no reply read on the way is an image out of
    /// bounds.
    #[test]
    fn random_bytes_end_in_an_error() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..4000 {
            let size = (next() % 96) as usize;
            let mut bytes: Vec<u8> = (0..size).map(|_| next() as u8).collect();
            // Half of the streams start as a frame the window reads.
            if round % 2 == 0 {
                if let Some(first) = bytes.first_mut() {
                    *first = [READY, REFUSED, OPENED, BITMAP, FAILED][(next() % 5) as usize];
                }
                // And a quarter announce a short length, to get past the
                // header.
                if round % 4 == 0 {
                    if let Some(announced) = bytes.get_mut(1..5) {
                        announced.copy_from_slice(&((next() % 40) as u32).to_le_bytes());
                    }
                }
            }
            let mut input = &bytes[..];
            let error = loop {
                match read_reply(&mut input) {
                    Ok(Reply::Bitmap {
                        width,
                        height,
                        pixels,
                        ..
                    }) => {
                        assert!(width <= MAX_WIDTH && height <= MAX_HEIGHT);
                        assert_eq!(
                            pixels.len() as u64,
                            u64::from(width) * u64::from(height) * 4
                        );
                    }
                    Ok(_) => {}
                    Err(error) => break error,
                }
            };
            // The stream is finite: it ends, at the latest, closed.
            let _ = error;
            let mut input = &bytes[..];
            while let Ok(Some(_)) = read_request(&mut input) {}
        }
    }
}
