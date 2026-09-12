//! Transparent decryption of an encrypted file (ISO 32000-2, 7.6): the
//! `/Encrypt` dictionary of the standard security handler is parsed into
//! [`fyp_crypto::Params`], the password is checked, and every object read
//! from the file is deciphered before it reaches the caller.
//!
//! What is and is not encrypted (7.6.3): strings and stream data of
//! indirect objects are; the `/Encrypt` dictionary itself, the trailer's
//! `/ID`, cross-reference streams and, when `/EncryptMetadata` is false,
//! the document's metadata stream are not. Objects inside an object stream
//! are not ciphered one by one: the object stream is, as a whole.
//!
//! A stream may name its own crypt filter (`/Filter /Crypt` with
//! `/DecodeParms << /Name /X >>`, 7.4.10). That filter is applied when the
//! object is read and removed from the dictionary, so the object model a
//! caller sees is the one of an unencrypted file.

use std::collections::BTreeMap;

use fyp_crypto::{Decryptor, Params};

use crate::object::{Dict, Name, ObjRef, Object};
use crate::{Error, Result};

pub use fyp_crypto::{Cipher, Revision};

/// How an opened file was encrypted, reported by
/// [`crate::document::Document::encryption`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encryption {
    /// Security handler revision (`/R`).
    pub revision: Revision,
    /// Length of the file key in bits.
    pub key_bits: u32,
    /// Cipher of streams that name no crypt filter of their own.
    pub streams: Cipher,
    /// Cipher of strings.
    pub strings: Cipher,
    /// `false` when the metadata stream is stored in the clear.
    pub encrypt_metadata: bool,
    /// Whether the password that opened the file was the owner password.
    /// With the empty password this is normally `false`.
    pub owner: bool,
}

/// The security handler of an open document.
#[derive(Debug, Clone)]
pub(crate) struct Crypt {
    decryptor: Decryptor,
    /// Named crypt filters of `/CF` (7.6.6), for streams that select one.
    filters: BTreeMap<Name, Cipher>,
    /// The `/Encrypt` dictionary's own object, whose strings are stored in
    /// the clear (7.6.3), when it is indirect.
    encrypt_ref: Option<ObjRef>,
}

impl Crypt {
    /// Read the trailer's `/Encrypt` entry through `resolve`, which must
    /// return objects as stored (no decryption yet), and open it with
    /// `password`. `Ok(None)` when the file is not encrypted.
    pub(crate) fn open<R>(trailer: &Dict, resolve: R, password: &[u8]) -> Result<Option<Crypt>>
    where
        R: Fn(&Object) -> Result<Object>,
    {
        let Some(entry) = trailer.get(&Name::new("Encrypt")) else {
            return Ok(None);
        };
        let encrypt_ref = match entry {
            Object::Reference(r) => Some(*r),
            _ => None,
        };
        let dict = match resolve(entry)? {
            Object::Dict(dict) => dict,
            Object::Null => return Ok(None),
            _ => return Err(bad("/Encrypt is not a dictionary")),
        };
        let file_id = match trailer.get(&Name::new("ID")).map(&resolve).transpose()? {
            Some(Object::Array(items)) => match items.first().map(&resolve).transpose()? {
                Some(Object::String(s)) => s,
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        let (params, filters) = parse(&dict, &resolve, file_id)?;
        let decryptor = Decryptor::open(&params, password).map_err(|e| match e {
            fyp_crypto::Error::WrongPassword => Error::WrongPassword,
            fyp_crypto::Error::BadParameters(message) => Error::BadEncryption { message },
        })?;
        Ok(Some(Crypt {
            decryptor,
            filters,
            encrypt_ref,
        }))
    }

    pub(crate) fn info(&self) -> Encryption {
        Encryption {
            revision: self.decryptor.revision(),
            key_bits: self.decryptor.key_bits(),
            streams: self.decryptor.stream_cipher(),
            strings: self.decryptor.string_cipher(),
            encrypt_metadata: self.decryptor.encrypt_metadata(),
            owner: self.decryptor.opened_as_owner(),
        }
    }

    /// Decipher every string and stream of the indirect object `r`
    /// (algorithm 1 keys them by object number and generation). Objects
    /// stored in the clear come back unchanged.
    pub(crate) fn decrypt_object(&self, r: ObjRef, obj: Object) -> Object {
        if Some(r) == self.encrypt_ref {
            return obj;
        }
        match obj {
            Object::Stream { dict, data } => self.decrypt_stream(r, dict, &data),
            other => self.decrypt_direct(r, other),
        }
    }

    /// Strings inside a direct object, at any depth. The depth is bounded
    /// by the parser ([`crate::parser::MAX_DEPTH`]).
    fn decrypt_direct(&self, r: ObjRef, obj: Object) -> Object {
        match obj {
            Object::String(s) => Object::String(self.decryptor.decrypt_string(r.num, r.gen, &s)),
            Object::Array(items) => Object::Array(
                items
                    .into_iter()
                    .map(|item| self.decrypt_direct(r, item))
                    .collect(),
            ),
            Object::Dict(dict) => Object::Dict(self.decrypt_dict(r, dict)),
            other => other,
        }
    }

    fn decrypt_dict(&self, r: ObjRef, dict: Dict) -> Dict {
        dict.into_iter()
            .map(|(k, v)| (k, self.decrypt_direct(r, v)))
            .collect()
    }

    fn decrypt_stream(&self, r: ObjRef, dict: Dict, data: &[u8]) -> Object {
        let type_is = |what: &[u8]| matches!(dict.get(&Name::new("Type")), Some(Object::Name(n)) if n.0 == what);
        // Cross-reference streams are never encrypted (7.5.8.2).
        if type_is(b"XRef") {
            return Object::Stream {
                dict,
                data: data.to_vec(),
            };
        }
        let (cipher, dict) = if type_is(b"Metadata") && !self.decryptor.encrypt_metadata() {
            (Cipher::Identity, dict)
        } else {
            match named_crypt_filter(&dict) {
                Some((name, stripped)) => (self.named_cipher(&name), stripped),
                None => (self.decryptor.stream_cipher(), dict),
            }
        };
        let data = self.decryptor.decrypt_with(cipher, r.num, r.gen, data);
        Object::Stream {
            dict: self.decrypt_dict(r, dict),
            data,
        }
    }

    /// Cipher of a crypt filter a stream names. `/Identity` is the
    /// identity; a name missing from `/CF` is a writer's slip, taken as
    /// the document's stream cipher.
    fn named_cipher(&self, name: &Name) -> Cipher {
        if name.0 == b"Identity" {
            return Cipher::Identity;
        }
        self.filters
            .get(name)
            .copied()
            .unwrap_or_else(|| self.decryptor.stream_cipher())
    }
}

/// The name of the crypt filter a stream selects through `/Filter /Crypt`
/// and `/DecodeParms /Name` (7.4.10), and the dictionary with that filter
/// stage removed. `None` when the stream has no `/Crypt` filter, or when
/// `/Filter` or `/DecodeParms` are indirect (then they cannot be inspected
/// here; the stream is treated as using the document's cipher).
fn named_crypt_filter(dict: &Dict) -> Option<(Name, Dict)> {
    let crypt = Name::new("Crypt");
    let filter = dict.get(&Name::new("Filter"))?;
    let parms = dict.get(&Name::new("DecodeParms"));
    let parms_name = |p: Option<&Object>| match p {
        Some(Object::Dict(d)) => match d.get(&Name::new("Name")) {
            Some(Object::Name(n)) => n.clone(),
            _ => Name::new("Identity"),
        },
        _ => Name::new("Identity"),
    };
    let mut stripped = dict.clone();
    let name = match filter {
        Object::Name(n) if *n == crypt => {
            stripped.remove(&Name::new("Filter"));
            stripped.remove(&Name::new("DecodeParms"));
            parms_name(parms)
        }
        Object::Array(filters) => {
            let index = filters
                .iter()
                .position(|f| matches!(f, Object::Name(n) if *n == crypt))?;
            let (name, remaining_parms) = match parms {
                Some(Object::Array(items)) => {
                    let name = parms_name(items.get(index));
                    let mut items = items.clone();
                    if index < items.len() {
                        items.remove(index);
                    }
                    (name, Some(items))
                }
                // A lone dictionary next to several filters belongs to
                // the one stage that takes parameters; a crypt filter's
                // `/Name` only makes sense if that stage is the crypt one.
                Some(Object::Dict(_)) if filters.len() == 1 => (parms_name(parms), None),
                _ => (Name::new("Identity"), parms.cloned().map(|p| vec![p])),
            };
            let mut filters = filters.clone();
            filters.remove(index);
            if filters.is_empty() {
                stripped.remove(&Name::new("Filter"));
                stripped.remove(&Name::new("DecodeParms"));
            } else {
                stripped.insert(Name::new("Filter"), Object::Array(filters));
                match remaining_parms {
                    Some(items) if items.iter().any(|p| !matches!(p, Object::Null)) => {
                        if items.len() == 1 {
                            if let Some(p) = items.into_iter().next() {
                                stripped.insert(Name::new("DecodeParms"), p);
                            }
                        } else {
                            stripped.insert(Name::new("DecodeParms"), Object::Array(items));
                        }
                    }
                    _ => {
                        stripped.remove(&Name::new("DecodeParms"));
                    }
                }
            }
            name
        }
        _ => return None,
    };
    Some((name, stripped))
}

/// Parse the `/Encrypt` dictionary (tables 20 and 21) into the handler's
/// parameters and the named crypt filters of `/CF`.
fn parse<R>(dict: &Dict, resolve: &R, file_id: Vec<u8>) -> Result<(Params, BTreeMap<Name, Cipher>)>
where
    R: Fn(&Object) -> Result<Object>,
{
    let get = |key: &str| -> Result<Option<Object>> {
        match dict.get(&Name::new(key)) {
            None | Some(Object::Null) => Ok(None),
            Some(obj) => resolve(obj).map(Some),
        }
    };
    let int = |key: &str| -> Result<Option<i64>> {
        match get(key)? {
            None => Ok(None),
            Some(Object::Integer(i)) => Ok(Some(i)),
            // Some writers store integers as reals.
            Some(Object::Real(f)) if f.fract() == 0.0 && f.abs() < 9.0e15 => Ok(Some(f as i64)),
            Some(_) => Err(bad(&format!("/{key} is not an integer"))),
        }
    };
    let string = |key: &str| -> Result<Option<Vec<u8>>> {
        match get(key)? {
            None => Ok(None),
            Some(Object::String(s)) => Ok(Some(s)),
            Some(_) => Err(bad(&format!("/{key} is not a string"))),
        }
    };

    match get("Filter")? {
        None => {}
        Some(Object::Name(n)) if n.0 == b"Standard" => {}
        Some(Object::Name(_)) => {
            return Err(Error::Unsupported {
                feature: "security handlers other than /Standard (ISO 32000-2, 7.6.5)",
            })
        }
        Some(_) => return Err(bad("/Filter is not a name")),
    }
    let r = int("R")?.ok_or_else(|| bad("no /R"))?;
    let revision = Revision::from_number(r)
        .ok_or_else(|| bad(&format!("security handler revision {r} is unknown")))?;
    let v = int("V")?.unwrap_or(0);
    let length = int("Length")?;
    let owner = string("O")?.ok_or_else(|| bad("no /O"))?;
    let user = string("U")?.ok_or_else(|| bad("no /U"))?;
    let p = int("P")?.ok_or_else(|| bad("no /P"))?;
    // `/P` is a signed 32-bit value; some writers store it unsigned.
    let permissions = u32::try_from(p)
        .or_else(|_| i32::try_from(p).map(|signed| signed as u32))
        .map_err(|_| bad(&format!("/P {p} does not fit in 32 bits")))?;
    let encrypt_metadata = match get("EncryptMetadata")? {
        None => true,
        Some(Object::Bool(b)) => b,
        Some(_) => return Err(bad("/EncryptMetadata is not a boolean")),
    };

    let mut filters = BTreeMap::new();
    let (streams, strings, key_bits) = if revision >= Revision::R4 || v >= 4 {
        // Crypt filters (7.6.6): `/CF` maps names to dictionaries, `/StmF`
        // and `/StrF` pick one for streams and strings, `/Identity` by default.
        let mut lengths = BTreeMap::new();
        if let Some(cf) = get("CF")? {
            let Object::Dict(cf) = cf else {
                return Err(bad("/CF is not a dictionary"));
            };
            for (name, entry) in &cf {
                let Object::Dict(entry) = resolve(entry)? else {
                    return Err(bad(&format!(
                        "crypt filter /{} is not a dictionary",
                        name.as_str_lossy()
                    )));
                };
                let cfm = match entry.get(&Name::new("CFM")).map(resolve).transpose()? {
                    None => Cipher::Identity,
                    Some(Object::Name(m)) => match m.0.as_slice() {
                        b"None" => Cipher::Identity,
                        b"V2" => Cipher::Rc4,
                        b"AESV2" => Cipher::Aes128,
                        b"AESV3" => Cipher::Aes256,
                        _ => {
                            return Err(bad(&format!(
                                "crypt filter /{} uses the unknown method /{}",
                                name.as_str_lossy(),
                                m.as_str_lossy()
                            )))
                        }
                    },
                    Some(_) => return Err(bad("/CFM is not a name")),
                };
                if let Some(Object::Integer(bits)) =
                    entry.get(&Name::new("Length")).map(resolve).transpose()?
                {
                    lengths.insert(name.clone(), bits);
                }
                filters.insert(name.clone(), cfm);
            }
        }
        let select = |key: &str| -> Result<(Cipher, Option<i64>)> {
            match get(key)? {
                None => Ok((Cipher::Identity, None)),
                Some(Object::Name(n)) if n.0 == b"Identity" => Ok((Cipher::Identity, None)),
                Some(Object::Name(n)) => match filters.get(&n) {
                    Some(&cipher) => Ok((cipher, lengths.get(&n).copied())),
                    None => Err(bad(&format!(
                        "/{key} names the crypt filter /{}, which /CF does not define",
                        n.as_str_lossy()
                    ))),
                },
                Some(_) => Err(bad(&format!("/{key} is not a name"))),
            }
        };
        let (streams, stream_length) = select("StmF")?;
        let (strings, string_length) = select("StrF")?;
        let key_bits = if revision.uses_aes256() {
            match length {
                None | Some(256) => 256,
                Some(other) => return Err(bad(&format!("/Length {other} with revision {r}"))),
            }
        } else if streams == Cipher::Aes128 || strings == Cipher::Aes128 {
            // AES-128 fixes the key length whatever the dictionary says.
            128
        } else {
            // A crypt filter's `/Length` is in bits, but a value that
            // small can only be bytes: Acrobat writes `/Length 16`.
            let in_bits = |n: i64| if n < 40 { n.saturating_mul(8) } else { n };
            let declared = stream_length.or(string_length).map(in_bits).or(length);
            bits_from(declared.unwrap_or(40))?
        };
        (streams, strings, key_bits)
    } else {
        // Revisions 2 and 3: RC4 everywhere. Revision 2 keys are 40-bit
        // whatever `/Length` says (7.6.4.3.2, algorithm 2 step i).
        let key_bits = match revision {
            Revision::R2 => 40,
            _ => bits_from(length.unwrap_or(40))?,
        };
        (Cipher::Rc4, Cipher::Rc4, key_bits)
    };

    let params = Params {
        revision,
        key_bits,
        owner,
        user,
        owner_key: string("OE")?.unwrap_or_default(),
        user_key: string("UE")?.unwrap_or_default(),
        permissions,
        encrypt_metadata,
        streams,
        strings,
        file_id,
    };
    Ok((params, filters))
}

/// `/Length` as a key size in bits: a multiple of 8 between 40 and 128.
fn bits_from(length: i64) -> Result<u32> {
    u32::try_from(length)
        .ok()
        .filter(|bits| bits % 8 == 0 && (40..=128).contains(bits))
        .ok_or_else(|| {
            bad(&format!(
                "/Length {length} is not a multiple of 8 between 40 and 128"
            ))
        })
}

fn bad(message: &str) -> Error {
    Error::BadEncryption {
        message: message.into(),
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn dict(src: &str) -> Dict {
        match Parser::new(src.as_bytes()).parse_object().expect("parse") {
            Object::Dict(d) => d,
            other => panic!("{other:?}"),
        }
    }

    fn direct(o: &Object) -> Result<Object> {
        Ok(o.clone())
    }

    #[test]
    fn named_crypt_filter_is_found_and_stripped() {
        // Lone /Crypt: the whole filter chain goes.
        let d = dict("<< /Filter /Crypt /DecodeParms << /Name /StdCF >> /Length 3 >>");
        let (name, stripped) = named_crypt_filter(&d).unwrap();
        assert_eq!(name, Name::new("StdCF"));
        assert_eq!(stripped, dict("<< /Length 3 >>"));
        // Without a name: Identity.
        let d = dict("<< /Filter /Crypt >>");
        assert_eq!(named_crypt_filter(&d).unwrap().0, Name::new("Identity"));
        // In an array with parallel parms.
        let d = dict(
            "<< /Filter [/Crypt /FlateDecode] /DecodeParms [<< /Name /X >> << /Predictor 12 >>] >>",
        );
        let (name, stripped) = named_crypt_filter(&d).unwrap();
        assert_eq!(name, Name::new("X"));
        assert_eq!(
            stripped,
            dict("<< /Filter [/FlateDecode] /DecodeParms << /Predictor 12 >> >>")
        );
        // Array with a null for the remaining stage: parms dropped.
        let d = dict("<< /Filter [/Crypt /FlateDecode] /DecodeParms [<< /Name /X >> null] >>");
        assert_eq!(
            named_crypt_filter(&d).unwrap().1,
            dict("<< /Filter [/FlateDecode] >>")
        );
        // Single-element array with a lone dict.
        let d = dict("<< /Filter [/Crypt] /DecodeParms << /Name /Y >> >>");
        let (name, stripped) = named_crypt_filter(&d).unwrap();
        assert_eq!(name, Name::new("Y"));
        assert_eq!(stripped, dict("<< >>"));
        // No crypt filter, or indirect filter.
        assert!(named_crypt_filter(&dict("<< /Filter /FlateDecode >>")).is_none());
        assert!(named_crypt_filter(&dict("<< /Filter 4 0 R >>")).is_none());
        assert!(named_crypt_filter(&dict("<< >>")).is_none());
    }

    #[test]
    fn parse_rejects_malformed_dictionaries() {
        let o = "(0123456789abcdef0123456789abcdef)";
        let base = format!("<< /Filter /Standard /V 2 /R 3 /Length 128 /O {o} /U {o} /P -1 >>");
        let (params, filters) = parse(&dict(&base), &direct, b"id".to_vec()).unwrap();
        assert_eq!(params.revision, Revision::R3);
        assert_eq!(params.key_bits, 128);
        assert_eq!(params.permissions, 0xffff_ffff);
        assert_eq!(params.streams, Cipher::Rc4);
        assert!(filters.is_empty());
        let cases = [
            ("no /R", format!("<< /V 2 /O {o} /U {o} /P -1 >>")),
            ("unknown revision", base.replace("/R 3", "/R 9")),
            ("no /O", base.replace(&format!("/O {o}"), "")),
            ("no /P", base.replace("/P -1", "")),
            ("/P huge", base.replace("/P -1", "/P 99999999999")),
            ("/Length 7", base.replace("/Length 128", "/Length 7")),
            ("/Length 256", base.replace("/Length 128", "/Length 256")),
            ("/Length 0", base.replace("/Length 128", "/Length 0")),
            ("/Length name", base.replace("/Length 128", "/Length /X")),
            ("/O integer", base.replace(&format!("/O {o}"), "/O 12")),
            (
                "/EncryptMetadata 1",
                base.replace("/P -1", "/P -1 /EncryptMetadata 1"),
            ),
            (
                "unknown StmF",
                base.replace("/R 3", "/R 4 /StmF /Nope /CF << >>"),
            ),
            (
                "unknown CFM",
                base.replace("/R 3", "/R 4 /StmF /A /CF << /A << /CFM /Blowfish >> >>"),
            ),
            ("/CF not a dict", base.replace("/R 3", "/R 4 /CF 5")),
            ("R6 with /Length 128", base.replace("/R 3", "/R 6")),
        ];
        for (what, src) in cases {
            let got = parse(&dict(&src), &direct, Vec::new());
            assert!(
                matches!(got, Err(Error::BadEncryption { .. })),
                "{what}: {got:?}"
            );
        }
        let pubsec = base.replace("/Standard", "/Adobe.PubSec");
        assert!(matches!(
            parse(&dict(&pubsec), &direct, Vec::new()),
            Err(Error::Unsupported { .. })
        ));
        // Unsigned /P and revision 2 ignoring /Length are tolerated.
        let unsigned = base.replace("/P -1", "/P 4294967292");
        assert_eq!(
            parse(&dict(&unsigned), &direct, Vec::new())
                .unwrap()
                .0
                .permissions,
            0xffff_fffc
        );
        let r2 = base.replace("/R 3", "/R 2");
        assert_eq!(
            parse(&dict(&r2), &direct, Vec::new()).unwrap().0.key_bits,
            40
        );
    }

    #[test]
    fn parse_resolves_crypt_filters() {
        let o = "(0123456789abcdef0123456789abcdef)";
        let src = format!(
            "<< /Filter /Standard /V 4 /R 4 /Length 128 /O {o} /U {o} /P -3904 \
             /CF << /StdCF << /CFM /AESV2 /Length 16 /AuthEvent /DocOpen >> /Plain << /CFM /None >> >> \
             /StmF /StdCF /StrF /Identity /EncryptMetadata false >>"
        );
        let (params, filters) = parse(&dict(&src), &direct, Vec::new()).unwrap();
        assert_eq!(params.streams, Cipher::Aes128);
        assert_eq!(params.strings, Cipher::Identity);
        assert_eq!(params.key_bits, 128);
        assert!(!params.encrypt_metadata);
        assert_eq!(params.permissions, 0xffff_f0c0);
        assert_eq!(filters.get(&Name::new("StdCF")), Some(&Cipher::Aes128));
        assert_eq!(filters.get(&Name::new("Plain")), Some(&Cipher::Identity));
        // RC4 crypt filter with /Length in bytes.
        let src = format!(
            "<< /Filter /Standard /V 4 /R 4 /Length 40 /O {o} /U {o} /P -1 \
             /CF << /StdCF << /CFM /V2 /Length 16 >> >> /StmF /StdCF /StrF /StdCF >>"
        );
        let (params, _) = parse(&dict(&src), &direct, Vec::new()).unwrap();
        assert_eq!(params.streams, Cipher::Rc4);
        assert_eq!(params.key_bits, 128);
        // Revision 6.
        let o48 = "(0123456789abcdef0123456789abcdef0123456789abcdef)";
        let src = format!(
            "<< /Filter /Standard /V 5 /R 6 /Length 256 /O {o48} /U {o48} /OE {o} /UE {o} /P -1 \
             /CF << /StdCF << /CFM /AESV3 /Length 32 >> >> /StmF /StdCF /StrF /StdCF >>"
        );
        let (params, _) = parse(&dict(&src), &direct, Vec::new()).unwrap();
        assert_eq!(params.revision, Revision::R6);
        assert_eq!(params.streams, Cipher::Aes256);
        assert_eq!(params.key_bits, 256);
        assert_eq!(params.user_key.len(), 32);
    }
}
