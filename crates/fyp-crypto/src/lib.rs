//! `fyp-crypto` — the standard security handler (ISO 32000-2, clause 7.6).
//!
//! This crate turns the values of an `/Encrypt` dictionary and a password
//! into a [`Decryptor`] that deciphers strings and streams object by
//! object. It knows nothing about PDF syntax: the caller (`fyp-core`)
//! parses the dictionary into [`Params`] and hands over raw bytes. Nothing
//! here depends on `fyp-core`, so the dependency arrow goes core → crypto.
//!
//! Supported: revisions 2 and 3 (RC4, 40 to 128 bits, 7.6.4), revision 4
//! (RC4 or AES-128 through crypt filters, 7.6.6), revisions 5 and 6
//! (AES-256; revision 6 is the PDF 2.0 one, 7.6.4.3.3). Public-key
//! handlers (7.6.5) are outside this crate.
//!
//! Primitives come from audited RustCrypto crates (`aes`, `cbc`, `md-5`,
//! `rc4`, `sha2`); this crate only wires the PDF key derivation
//! (algorithms 1 to 13 of 7.6.4). Nothing here panics on input: malformed
//! ciphertext is decrypted as far as it goes, and every inconsistency in
//! the parameters is an [`Error`].
//!
//! Passwords for revisions 5 and 6 are taken as UTF-8 bytes truncated to
//! 127 bytes; the SASLprep normalisation of 7.6.4.3.3 is not applied, so
//! only passwords whose normalisation changes them are affected.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fmt;

use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use aes::{Aes128, Aes256};
use md5::{Digest as _, Md5};
use rc4::consts::{U10, U11, U12, U13, U14, U15, U16, U5, U6, U7, U8, U9};
use rc4::{KeyInit, Rc4, StreamCipher};
use sha2::{Sha256, Sha384, Sha512};

/// Security handler revision, the `/R` entry of the `/Encrypt` dictionary
/// (ISO 32000-2, table 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Revision {
    /// RC4 40-bit (PDF 1.1).
    R2,
    /// RC4 40 to 128-bit (PDF 1.4).
    R3,
    /// RC4 or AES-128 via crypt filters (PDF 1.5).
    R4,
    /// AES-256 as published in Adobe extension level 3, deprecated.
    R5,
    /// AES-256, ISO 32000-2 (PDF 2.0). The only revision new files should use.
    R6,
}

impl Revision {
    /// The revision for the integer value of `/R`, if it is one this
    /// crate knows.
    pub fn from_number(r: i64) -> Option<Revision> {
        match r {
            2 => Some(Revision::R2),
            3 => Some(Revision::R3),
            4 => Some(Revision::R4),
            5 => Some(Revision::R5),
            6 => Some(Revision::R6),
            _ => None,
        }
    }

    /// The integer value of `/R`.
    pub fn number(self) -> u8 {
        match self {
            Revision::R2 => 2,
            Revision::R3 => 3,
            Revision::R4 => 4,
            Revision::R5 => 5,
            Revision::R6 => 6,
        }
    }

    /// Revisions 5 and 6 derive the file key from SHA-2 and use AES-256;
    /// the earlier ones use MD5 and RC4 or AES-128.
    pub fn uses_aes256(self) -> bool {
        matches!(self, Revision::R5 | Revision::R6)
    }
}

/// How one class of data (streams, strings, or a named crypt filter) is
/// ciphered: the `/CFM` of a crypt filter dictionary (ISO 32000-2,
/// table 25), or the whole file's algorithm for revisions 2 and 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cipher {
    /// `/Identity` crypt filter or `/CFM /None`: data is stored in the clear.
    Identity,
    /// RC4 with a per-object key (`/CFM /V2`, and every revision 2 or 3 file).
    Rc4,
    /// AES-128 in CBC mode with a per-object key (`/CFM /AESV2`).
    Aes128,
    /// AES-256 in CBC mode with the file key itself (`/CFM /AESV3`).
    Aes256,
}

/// Values of an `/Encrypt` dictionary of the standard security handler,
/// already parsed and resolved by the caller. Sizes are checked by
/// [`Decryptor::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Params {
    /// `/R`.
    pub revision: Revision,
    /// Key length in bits: 40 for revision 2, `/Length` (40 to 128) for
    /// revisions 3 and 4, 256 for revisions 5 and 6.
    pub key_bits: u32,
    /// `/O`: 32 bytes up to revision 4, 48 from revision 5.
    pub owner: Vec<u8>,
    /// `/U`: 32 bytes up to revision 4, 48 from revision 5.
    pub user: Vec<u8>,
    /// `/OE` (revisions 5 and 6): 32 bytes.
    pub owner_key: Vec<u8>,
    /// `/UE` (revisions 5 and 6): 32 bytes.
    pub user_key: Vec<u8>,
    /// `/P` as an unsigned 32-bit value (the low 32 bits of the integer).
    pub permissions: u32,
    /// `/EncryptMetadata`, `true` when absent.
    pub encrypt_metadata: bool,
    /// Cipher of streams: `/StmF` resolved through `/CF` for revision 4
    /// and later, RC4 before.
    pub streams: Cipher,
    /// Cipher of strings: `/StrF` resolved through `/CF` for revision 4
    /// and later, RC4 before.
    pub strings: Cipher,
    /// First string of the trailer's `/ID` array; empty when absent.
    pub file_id: Vec<u8>,
}

/// Why a [`Decryptor`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Neither the user nor the owner password matches.
    WrongPassword,
    /// The parameters contradict each other or the standard: wrong key
    /// length, `/O` or `/U` too short, a cipher the revision cannot use.
    BadParameters(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WrongPassword => write!(f, "the password does not open this file"),
            Error::BadParameters(message) => write!(f, "invalid encryption parameters: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// Padding string of algorithm 2 (ISO 32000-2, 7.6.4.3.2).
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Salt appended to the object key material for AES-128 (algorithm 1, step b).
const AES_SALT: [u8; 4] = [0x73, 0x41, 0x6C, 0x54];

/// Longest password of revisions 5 and 6, in bytes (7.6.4.3.3).
const MAX_UTF8_PASSWORD: usize = 127;

/// The file key of an opened document, ready to decipher objects.
#[derive(Clone, PartialEq, Eq)]
pub struct Decryptor {
    key: Vec<u8>,
    revision: Revision,
    streams: Cipher,
    strings: Cipher,
    encrypt_metadata: bool,
    owner: bool,
}

impl fmt::Debug for Decryptor {
    /// The key is never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decryptor")
            .field("revision", &self.revision)
            .field("key_bits", &(self.key.len() * 8))
            .field("streams", &self.streams)
            .field("strings", &self.strings)
            .field("encrypt_metadata", &self.encrypt_metadata)
            .field("owner", &self.owner)
            .finish()
    }
}

impl Decryptor {
    /// Check the parameters, try `password` as the user password then as
    /// the owner password (algorithms 6, 7, 11 and 12), and derive the
    /// file key. The empty password is the usual one: most encrypted files
    /// only restrict what readers may do, not who may read.
    pub fn open(params: &Params, password: &[u8]) -> Result<Decryptor, Error> {
        check(params)?;
        let params = &padded(params);
        let (key, owner) = if params.revision.uses_aes256() {
            aes256_file_key(params, password)?
        } else {
            legacy_file_key(params, password)?
        };
        Ok(Decryptor {
            key,
            revision: params.revision,
            streams: params.streams,
            strings: params.strings,
            encrypt_metadata: params.encrypt_metadata,
            owner,
        })
    }

    /// Security handler revision of the file.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Length of the file key in bits.
    pub fn key_bits(&self) -> u32 {
        u32::try_from(self.key.len() * 8).unwrap_or(u32::MAX)
    }

    /// Cipher applied to streams that name no crypt filter of their own.
    pub fn stream_cipher(&self) -> Cipher {
        self.streams
    }

    /// Cipher applied to strings.
    pub fn string_cipher(&self) -> Cipher {
        self.strings
    }

    /// `false` when the document's metadata stream is stored in the clear
    /// (`/EncryptMetadata false`, 7.6.4.2).
    pub fn encrypt_metadata(&self) -> bool {
        self.encrypt_metadata
    }

    /// Whether the password that opened the file was the owner password.
    pub fn opened_as_owner(&self) -> bool {
        self.owner
    }

    /// Decipher the data of a stream that belongs to object `num gen`
    /// (algorithm 1), with the document's stream cipher.
    pub fn decrypt_stream(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        self.decrypt_with(self.streams, num, gen, data)
    }

    /// Decipher a string that belongs to object `num gen` (algorithm 1),
    /// with the document's string cipher.
    pub fn decrypt_string(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        self.decrypt_with(self.strings, num, gen, data)
    }

    /// Decipher data of object `num gen` with an explicit cipher: the one
    /// of a crypt filter the stream names itself (7.4.10).
    ///
    /// Tolerant with broken ciphertext, as readers are: AES data shorter
    /// than its initialisation vector gives nothing, a trailing partial
    /// block is dropped, and padding that is not PKCS#5 is kept.
    pub fn decrypt_with(&self, cipher: Cipher, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        match cipher {
            Cipher::Identity => data.to_vec(),
            Cipher::Rc4 => rc4_apply(&self.object_key(num, gen, false), data),
            Cipher::Aes128 => aes_cbc_decrypt(&self.object_key(num, gen, true), data),
            Cipher::Aes256 => aes_cbc_decrypt(&self.key, data),
        }
    }

    /// Cipher data of object `num gen`, the inverse of
    /// [`Decryptor::decrypt_with`]. `iv` is the initialisation vector
    /// written in front of AES data; it is ignored by RC4. Building block
    /// for fixtures and for the future encrypting writer.
    pub fn encrypt_with(
        &self,
        cipher: Cipher,
        num: u32,
        gen: u16,
        iv: &[u8; 16],
        data: &[u8],
    ) -> Vec<u8> {
        match cipher {
            Cipher::Identity => data.to_vec(),
            Cipher::Rc4 => rc4_apply(&self.object_key(num, gen, false), data),
            Cipher::Aes128 => aes_cbc_encrypt(&self.object_key(num, gen, true), iv, data),
            Cipher::Aes256 => aes_cbc_encrypt(&self.key, iv, data),
        }
    }

    /// Algorithm 1: the object key is the MD5 of the file key, the low
    /// three bytes of the object number, the low two bytes of the
    /// generation, and `sAlT` for AES-128, truncated to `n + 5` bytes,
    /// 16 at most.
    fn object_key(&self, num: u32, gen: u16, aes: bool) -> Vec<u8> {
        let mut hasher = Md5::new();
        hasher.update(&self.key);
        hasher.update(&num.to_le_bytes()[..3]);
        hasher.update(gen.to_le_bytes());
        if aes {
            hasher.update(AES_SALT);
        }
        let digest = hasher.finalize();
        let n = (self.key.len() + 5).min(16);
        digest.get(..n).unwrap_or(&digest).to_vec()
    }
}

/// Sizes and coherence of the parameters (tables 20 and 21).
fn check(params: &Params) -> Result<(), Error> {
    let bad = |m: &str| Err(Error::BadParameters(m.into()));
    let bits = params.key_bits;
    match params.revision {
        Revision::R2 if bits != 40 => return bad("revision 2 uses a 40-bit key"),
        Revision::R3 | Revision::R4 if bits % 8 != 0 || !(40..=128).contains(&bits) => {
            return bad(&format!(
                "key length {bits} is not a multiple of 8 between 40 and 128"
            ))
        }
        Revision::R5 | Revision::R6 if bits != 256 => {
            return bad(&format!(
                "revision {} uses a 256-bit key, not {bits}",
                params.revision.number()
            ))
        }
        _ => {}
    }
    if params.owner.is_empty() {
        return bad("/O is empty");
    }
    if params.user.is_empty() {
        return bad("/U is empty");
    }
    if params.revision.uses_aes256() {
        if params.owner_key.is_empty() {
            return bad("/OE is missing or empty");
        }
        if params.user_key.is_empty() {
            return bad("/UE is missing or empty");
        }
    }
    for cipher in [params.streams, params.strings] {
        let allowed = match cipher {
            Cipher::Identity => true,
            Cipher::Rc4 => !params.revision.uses_aes256(),
            Cipher::Aes128 => params.revision == Revision::R4 && bits == 128,
            Cipher::Aes256 => params.revision.uses_aes256(),
        };
        if !allowed {
            return bad(&format!(
                "cipher {cipher:?} cannot be used with revision {}",
                params.revision.number()
            ));
        }
    }
    Ok(())
}

/// `/O`, `/U`, `/OE` and `/UE` brought to their nominal size: longer
/// values are used as they are (the algorithms read a prefix), shorter
/// ones are padded with zero bytes, as qpdf does for writers that drop
/// trailing bytes. Only the first 16 bytes of `/U` matter for revisions 3
/// and 4 (algorithm 5), so such files still open.
fn padded(params: &Params) -> Params {
    let pad = |v: &[u8], n: usize| {
        let mut v = v.to_vec();
        if v.len() < n {
            v.resize(n, 0);
        }
        v
    };
    let nominal = if params.revision.uses_aes256() {
        48
    } else {
        32
    };
    Params {
        owner: pad(&params.owner, nominal),
        user: pad(&params.user, nominal),
        owner_key: pad(&params.owner_key, 32),
        user_key: pad(&params.user_key, 32),
        ..params.clone()
    }
}

// ---------------------------------------------------------------------------
// Revisions 2 to 4: MD5 and RC4 (7.6.4.3.2, algorithms 2 to 7)
// ---------------------------------------------------------------------------

/// Password padded or truncated to 32 bytes (algorithm 2, step a).
fn pad_password(password: &[u8]) -> [u8; 32] {
    let mut padded = [0u8; 32];
    let n = password.len().min(32);
    padded[..n].copy_from_slice(&password[..n]);
    padded[n..].copy_from_slice(&PAD[..32 - n]);
    padded
}

/// Key length in bytes for revisions 2 to 4.
fn legacy_key_len(params: &Params) -> usize {
    match params.revision {
        Revision::R2 => 5,
        _ => usize::try_from(params.key_bits / 8)
            .unwrap_or(16)
            .clamp(5, 16),
    }
}

/// Algorithm 2: the file key computed from a padded user password.
fn compute_legacy_key(params: &Params, padded: &[u8; 32]) -> Vec<u8> {
    let n = legacy_key_len(params);
    let mut hasher = Md5::new();
    hasher.update(padded);
    hasher.update(params.owner.get(..32).unwrap_or(&params.owner));
    hasher.update(params.permissions.to_le_bytes());
    hasher.update(&params.file_id);
    if params.revision >= Revision::R4 && !params.encrypt_metadata {
        hasher.update([0xff, 0xff, 0xff, 0xff]);
    }
    let mut digest = hasher.finalize().to_vec();
    if params.revision >= Revision::R3 {
        for _ in 0..50 {
            digest = Md5::digest(digest.get(..n).unwrap_or(&digest)).to_vec();
        }
    }
    digest.truncate(n);
    digest
}

/// Algorithms 4 and 5: the `/U` value for a file key. Revision 2 gives
/// 32 significant bytes; revisions 3 and 4 give 16, padded to 32.
fn compute_user_value(params: &Params, key: &[u8]) -> Vec<u8> {
    match params.revision {
        Revision::R2 => rc4_apply(key, &PAD),
        _ => {
            let mut hasher = Md5::new();
            hasher.update(PAD);
            hasher.update(&params.file_id);
            let mut value = rc4_apply(key, &hasher.finalize());
            for i in 1..=19u8 {
                let xored: Vec<u8> = key.iter().map(|b| b ^ i).collect();
                value = rc4_apply(&xored, &value);
            }
            value.extend_from_slice(&PAD[..16]);
            value
        }
    }
}

/// Algorithm 6: does `padded` (a user password) open the file? Returns
/// the file key when it does.
fn try_user_password(params: &Params, padded: &[u8; 32]) -> Option<Vec<u8>> {
    let key = compute_legacy_key(params, padded);
    let computed = compute_user_value(params, &key);
    let significant = match params.revision {
        Revision::R2 => 32,
        _ => 16,
    };
    (computed.get(..significant) == params.user.get(..significant)).then_some(key)
}

/// Algorithm 3, steps a to d: the RC4 key derived from the owner password.
fn owner_rc4_key(params: &Params, owner_password: &[u8]) -> Vec<u8> {
    let n = legacy_key_len(params);
    let mut digest = Md5::digest(pad_password(owner_password)).to_vec();
    if params.revision >= Revision::R3 {
        for _ in 0..50 {
            digest = Md5::digest(digest.get(..n).unwrap_or(&digest)).to_vec();
        }
    }
    digest.truncate(n);
    digest
}

/// Algorithm 7: the padded user password recovered from `/O` with the
/// owner password.
fn user_password_from_owner(params: &Params, owner_password: &[u8]) -> [u8; 32] {
    let key = owner_rc4_key(params, owner_password);
    let owner = params.owner.get(..32).unwrap_or(&params.owner);
    let recovered = match params.revision {
        Revision::R2 => rc4_apply(&key, owner),
        _ => {
            let mut value = owner.to_vec();
            for i in (0..=19u8).rev() {
                let xored: Vec<u8> = key.iter().map(|b| b ^ i).collect();
                value = rc4_apply(&xored, &value);
            }
            value
        }
    };
    let mut padded = [0u8; 32];
    let n = recovered.len().min(32);
    padded[..n].copy_from_slice(&recovered[..n]);
    padded
}

/// The file key of a revision 2 to 4 file, and whether the password was
/// the owner one.
fn legacy_file_key(params: &Params, password: &[u8]) -> Result<(Vec<u8>, bool), Error> {
    if let Some(key) = try_user_password(params, &pad_password(password)) {
        return Ok((key, false));
    }
    let padded = user_password_from_owner(params, password);
    match try_user_password(params, &padded) {
        Some(key) => Ok((key, true)),
        None => Err(Error::WrongPassword),
    }
}

/// Algorithm 3: the `/O` value for a pair of passwords. An empty owner
/// password is replaced by the user password.
pub fn legacy_owner_value(params: &Params, owner_password: &[u8], user_password: &[u8]) -> Vec<u8> {
    let owner_password = if owner_password.is_empty() {
        user_password
    } else {
        owner_password
    };
    let key = owner_rc4_key(params, owner_password);
    let mut value = rc4_apply(&key, &pad_password(user_password));
    if params.revision >= Revision::R3 {
        for i in 1..=19u8 {
            let xored: Vec<u8> = key.iter().map(|b| b ^ i).collect();
            value = rc4_apply(&xored, &value);
        }
    }
    value
}

/// The `/O` and `/U` values of a revision 2 to 4 file for a pair of
/// passwords (algorithms 3 to 5). `params.owner` and `params.user` are
/// ignored; the other fields describe the file. Building block for
/// fixtures and for the future encrypting writer.
pub fn legacy_owner_user(
    params: &Params,
    owner_password: &[u8],
    user_password: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    if params.revision.uses_aes256() {
        return Err(Error::BadParameters(
            "revisions 5 and 6 use `aes256_entries`".into(),
        ));
    }
    let owner = legacy_owner_value(params, owner_password, user_password);
    let with_owner = Params {
        owner: owner.clone(),
        ..params.clone()
    };
    let key = compute_legacy_key(&with_owner, &pad_password(user_password));
    let user = compute_user_value(&with_owner, &key);
    Ok((owner, user))
}

// ---------------------------------------------------------------------------
// Revisions 5 and 6: SHA-2 and AES-256 (7.6.4.3.3, algorithms 2.A, 2.B, 8 to 13)
// ---------------------------------------------------------------------------

/// Password as used by revisions 5 and 6: at most 127 bytes.
fn utf8_password(password: &[u8]) -> &[u8] {
    password.get(..MAX_UTF8_PASSWORD).unwrap_or(password)
}

/// Algorithm 2.B (revision 6), or a plain SHA-256 for revision 5: the
/// 32-byte hash of a password, a salt, and, for the owner password, the
/// 48 bytes of `/U`.
fn hash_2b(revision: Revision, password: &[u8], salt: &[u8], udata: &[u8]) -> [u8; 32] {
    let mut k: Vec<u8> = {
        let mut h = Sha256::new();
        h.update(password);
        h.update(salt);
        h.update(udata);
        h.finalize().to_vec()
    };
    if revision == Revision::R5 {
        return first_32(&k);
    }
    let mut round: usize = 0;
    loop {
        // K1 is (password + K + udata) repeated 64 times.
        let unit_len = password.len() + k.len() + udata.len();
        let mut k1 = Vec::with_capacity(unit_len * 64);
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(udata);
        }
        let (key, iv) = (k.get(..16).unwrap_or(&k), k.get(16..32).unwrap_or(&k));
        let e = aes128_cbc_encrypt_raw(key, iv, &k1);
        let modulo = e.iter().take(16).map(|&b| u32::from(b)).sum::<u32>() % 3;
        k = match modulo {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        let last = e.last().copied().map_or(0, usize::from);
        if round >= 64 && last <= round - 32 {
            break;
        }
    }
    first_32(&k)
}

fn first_32(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

/// The file key of a revision 5 or 6 file (algorithms 11, 12 and 2.A),
/// and whether the password was the owner one.
fn aes256_file_key(params: &Params, password: &[u8]) -> Result<(Vec<u8>, bool), Error> {
    let password = utf8_password(password);
    let user = params.user.get(..48).unwrap_or(&params.user);
    let owner = params.owner.get(..48).unwrap_or(&params.owner);
    let (u_hash, u_vsalt, u_ksalt) = (&user[..32], &user[32..40], &user[40..48]);
    let (o_hash, o_vsalt, o_ksalt) = (&owner[..32], &owner[32..40], &owner[40..48]);
    // Algorithm 11: user password.
    if hash_2b(params.revision, password, u_vsalt, &[]) == u_hash {
        let intermediate = hash_2b(params.revision, password, u_ksalt, &[]);
        let key = aes256_cbc_no_iv_decrypt(&intermediate, &params.user_key);
        return Ok((key, false));
    }
    // Algorithm 12: owner password.
    if hash_2b(params.revision, password, o_vsalt, user) == o_hash {
        let intermediate = hash_2b(params.revision, password, o_ksalt, user);
        let key = aes256_cbc_no_iv_decrypt(&intermediate, &params.owner_key);
        return Ok((key, true));
    }
    Err(Error::WrongPassword)
}

/// The `/U`, `/UE`, `/O`, `/OE` and `/Perms` values of a revision 5 or 6
/// file (algorithms 8, 9 and 10). Building block for fixtures and for the
/// future encrypting writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aes256Entries {
    /// `/U`, 48 bytes.
    pub user: Vec<u8>,
    /// `/UE`, 32 bytes.
    pub user_key: Vec<u8>,
    /// `/O`, 48 bytes.
    pub owner: Vec<u8>,
    /// `/OE`, 32 bytes.
    pub owner_key: Vec<u8>,
    /// `/Perms`, 16 bytes.
    pub perms: Vec<u8>,
}

/// Compute [`Aes256Entries`] for a file key and a pair of passwords. The
/// four salts (user validation, user key, owner validation, owner key)
/// are the caller's: random in a writer, fixed in a fixture.
pub fn aes256_entries(
    revision: Revision,
    file_key: &[u8; 32],
    user_password: &[u8],
    owner_password: &[u8],
    salts: &[[u8; 8]; 4],
    permissions: u32,
    encrypt_metadata: bool,
) -> Aes256Entries {
    let user_password = utf8_password(user_password);
    let owner_password = utf8_password(owner_password);
    // Algorithm 8.
    let mut user = hash_2b(revision, user_password, &salts[0], &[]).to_vec();
    user.extend_from_slice(&salts[0]);
    user.extend_from_slice(&salts[1]);
    let intermediate = hash_2b(revision, user_password, &salts[1], &[]);
    let user_key = aes256_cbc_no_iv_encrypt(&intermediate, file_key);
    // Algorithm 9.
    let mut owner = hash_2b(revision, owner_password, &salts[2], &user).to_vec();
    owner.extend_from_slice(&salts[2]);
    owner.extend_from_slice(&salts[3]);
    let intermediate = hash_2b(revision, owner_password, &salts[3], &user);
    let owner_key = aes256_cbc_no_iv_encrypt(&intermediate, file_key);
    // Algorithm 10.
    let mut perms = [0u8; 16];
    perms[..4].copy_from_slice(&permissions.to_le_bytes());
    perms[4..8].copy_from_slice(&[0xff; 4]);
    perms[8] = if encrypt_metadata { b'T' } else { b'F' };
    perms[9..12].copy_from_slice(b"adb");
    let perms = aes256_ecb_encrypt_block(file_key, &perms);
    Aes256Entries {
        user,
        user_key,
        owner,
        owner_key,
        perms,
    }
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// RC4 keystream applied to `data`. The `rc4` crate types the key length,
/// so each of the 5 to 16 byte sizes the standard allows gets its own
/// instantiation; any other length (impossible after `check`) leaves the
/// data unchanged rather than guessing.
fn rc4_apply(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    macro_rules! with_size {
        ($size:ty) => {
            if let Some(key) = rc4::Key::<$size>::from_exact_iter(key.iter().copied()) {
                Rc4::<$size>::new(&key).apply_keystream(&mut out);
            }
        };
    }
    match key.len() {
        5 => with_size!(U5),
        6 => with_size!(U6),
        7 => with_size!(U7),
        8 => with_size!(U8),
        9 => with_size!(U9),
        10 => with_size!(U10),
        11 => with_size!(U11),
        12 => with_size!(U12),
        13 => with_size!(U13),
        14 => with_size!(U14),
        15 => with_size!(U15),
        16 => with_size!(U16),
        _ => {}
    }
    out
}

/// One 16-byte block from a slice of that exact length.
fn block(chunk: &[u8]) -> Option<aes::Block> {
    <[u8; 16]>::try_from(chunk).ok().map(aes::Block::from)
}

/// AES-CBC decryption as PDF stores it (7.6.3.1): a 16-byte initialisation
/// vector in front, PKCS#5 padding at the end. The key selects AES-128 or
/// AES-256. Tolerant: no IV gives nothing, a trailing partial block is
/// dropped, invalid padding is kept.
fn aes_cbc_decrypt(key: &[u8], data: &[u8]) -> Vec<u8> {
    let Some((iv, body)) = data.split_at_checked(16) else {
        return Vec::new();
    };
    let mut out = match key.len() {
        16 => cbc_decrypt_blocks::<Aes128>(key, iv, body),
        32 => cbc_decrypt_blocks::<Aes256>(key, iv, body),
        _ => return Vec::new(),
    };
    strip_pkcs5(&mut out);
    out
}

fn cbc_decrypt_blocks<C>(key: &[u8], iv: &[u8], body: &[u8]) -> Vec<u8>
where
    C: aes::cipher::BlockCipher
        + aes::cipher::BlockDecrypt
        + aes::cipher::KeyInit
        + aes::cipher::BlockSizeUser<BlockSize = aes::cipher::consts::U16>,
{
    let Ok(mut dec) = cbc::Decryptor::<C>::new_from_slices(key, iv) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(body.len());
    for chunk in body.chunks_exact(16) {
        let Some(mut b) = block(chunk) else {
            break;
        };
        dec.decrypt_block_mut(&mut b);
        out.extend_from_slice(&b);
    }
    out
}

/// Remove PKCS#5 padding when it is well formed (last byte 1 to 16, that
/// many bytes available). Anything else is left as is.
fn strip_pkcs5(data: &mut Vec<u8>) {
    if let Some(&last) = data.last() {
        let n = usize::from(last);
        if (1..=16).contains(&n) && n <= data.len() {
            data.truncate(data.len() - n);
        }
    }
}

/// AES-CBC encryption as PDF stores it: `iv` written first, PKCS#5
/// padding added. The key selects AES-128 or AES-256.
fn aes_cbc_encrypt(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let pad = 16 - data.len() % 16;
    let mut padded = data.to_vec();
    padded.resize(data.len() + pad, u8::try_from(pad).unwrap_or(16));
    let mut out = iv.to_vec();
    out.extend_from_slice(&match key.len() {
        16 => cbc_encrypt_blocks::<Aes128>(key, iv, &padded),
        32 => cbc_encrypt_blocks::<Aes256>(key, iv, &padded),
        _ => Vec::new(),
    });
    out
}

fn cbc_encrypt_blocks<C>(key: &[u8], iv: &[u8], body: &[u8]) -> Vec<u8>
where
    C: aes::cipher::BlockCipher
        + aes::cipher::BlockEncrypt
        + aes::cipher::KeyInit
        + aes::cipher::BlockSizeUser<BlockSize = aes::cipher::consts::U16>,
{
    let Ok(mut enc) = cbc::Encryptor::<C>::new_from_slices(key, iv) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(body.len());
    for chunk in body.chunks_exact(16) {
        let Some(mut b) = block(chunk) else {
            break;
        };
        enc.encrypt_block_mut(&mut b);
        out.extend_from_slice(&b);
    }
    out
}

/// AES-128 CBC without padding, for algorithm 2.B.
fn aes128_cbc_encrypt_raw(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    cbc_encrypt_blocks::<Aes128>(key, iv, data)
}

/// AES-256 CBC with a zero IV and no padding: the wrapping of the file
/// key in `/UE` and `/OE` (algorithms 8 and 9), and its unwrapping.
fn aes256_cbc_no_iv_decrypt(key: &[u8; 32], wrapped: &[u8]) -> Vec<u8> {
    cbc_decrypt_blocks::<Aes256>(key, &[0u8; 16], wrapped.get(..32).unwrap_or(wrapped))
}

fn aes256_cbc_no_iv_encrypt(key: &[u8; 32], file_key: &[u8; 32]) -> Vec<u8> {
    cbc_encrypt_blocks::<Aes256>(key, &[0u8; 16], file_key)
}

/// AES-256 ECB of one block: `/Perms` (algorithm 10). ECB on a single
/// block is CBC with a zero IV.
fn aes256_ecb_encrypt_block(key: &[u8; 32], block16: &[u8; 16]) -> Vec<u8> {
    cbc_encrypt_blocks::<Aes256>(key, &[0u8; 16], block16)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn legacy(revision: Revision, key_bits: u32, streams: Cipher) -> Params {
        Params {
            revision,
            key_bits,
            owner: Vec::new(),
            user: Vec::new(),
            owner_key: Vec::new(),
            user_key: Vec::new(),
            permissions: 0xffff_fffc,
            encrypt_metadata: true,
            streams,
            strings: streams,
            file_id: b"\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10".to_vec(),
        }
    }

    fn with_passwords(params: &Params, owner: &[u8], user: &[u8]) -> Params {
        let (o, u) = legacy_owner_user(params, owner, user).expect("legacy");
        Params {
            owner: o,
            user: u,
            ..params.clone()
        }
    }

    #[test]
    fn rc4_known_answer() {
        // Test vector from the `rc4` crate documentation.
        let out = rc4_apply(b"Secret", b"Attack at dawn");
        assert_eq!(
            out,
            [0x45, 0xA0, 0x1F, 0x64, 0x5F, 0xC3, 0x5B, 0x38, 0x35, 0x52, 0x54, 0x4B, 0x9B, 0xF5]
        );
        // Unsupported key length: data left alone, no panic.
        assert_eq!(rc4_apply(b"abc", b"xyz"), b"xyz");
    }

    #[test]
    fn pad_password_is_32_bytes() {
        assert_eq!(pad_password(b""), PAD);
        let p = pad_password(b"abc");
        assert_eq!(&p[..3], b"abc");
        assert_eq!(&p[3..], &PAD[..29]);
        let long = [b'x'; 40];
        assert_eq!(pad_password(&long), [b'x'; 32]);
    }

    #[test]
    fn legacy_user_and_owner_passwords_open_the_file() {
        for (revision, bits) in [
            (Revision::R2, 40),
            (Revision::R3, 40),
            (Revision::R3, 128),
            (Revision::R4, 128),
        ] {
            let cipher = if revision == Revision::R4 {
                Cipher::Aes128
            } else {
                Cipher::Rc4
            };
            let base = legacy(revision, bits, cipher);
            let params = with_passwords(&base, b"owner", b"user");
            assert_eq!(params.owner.len(), 32);
            assert_eq!(params.user.len(), 32);
            let as_user = Decryptor::open(&params, b"user").expect("user password");
            assert!(!as_user.opened_as_owner());
            assert_eq!(as_user.key_bits(), bits);
            let as_owner = Decryptor::open(&params, b"owner").expect("owner password");
            assert!(as_owner.opened_as_owner());
            assert_eq!(as_user.key, as_owner.key);
            assert_eq!(Decryptor::open(&params, b"nope"), Err(Error::WrongPassword));
            assert_eq!(Decryptor::open(&params, b""), Err(Error::WrongPassword));
            // Empty user password: the common case.
            let open = with_passwords(&base, b"owner", b"");
            let d = Decryptor::open(&open, b"").expect("empty password");
            let secret = b"hello, world - a string longer than one block";
            let iv = [7u8; 16];
            let boxed = d.encrypt_with(cipher, 12, 3, &iv, secret);
            assert_ne!(&boxed[..], &secret[..]);
            assert_eq!(d.decrypt_with(cipher, 12, 3, &boxed), secret);
            // Another object number gives another ciphertext.
            assert_ne!(d.encrypt_with(cipher, 13, 3, &iv, secret), boxed);
        }
    }

    #[test]
    fn empty_owner_password_falls_back_to_user_password() {
        let params = with_passwords(&legacy(Revision::R3, 128, Cipher::Rc4), b"", b"user");
        assert!(Decryptor::open(&params, b"user").is_ok());
    }

    #[test]
    fn encrypt_metadata_false_changes_the_key() {
        let base = legacy(Revision::R4, 128, Cipher::Aes128);
        let a = with_passwords(&base, b"o", b"");
        let b = with_passwords(
            &Params {
                encrypt_metadata: false,
                ..base
            },
            b"o",
            b"",
        );
        assert_ne!(a.user, b.user);
        assert!(Decryptor::open(&a, b"").is_ok());
        assert!(Decryptor::open(&b, b"").is_ok());
        assert!(!Decryptor::open(&b, b"").unwrap().encrypt_metadata());
    }

    #[test]
    fn aes256_revisions_5_and_6() {
        let file_key = [0xabu8; 32];
        let salts = [[1u8; 8], [2u8; 8], [3u8; 8], [4u8; 8]];
        for revision in [Revision::R5, Revision::R6] {
            let e = aes256_entries(
                revision,
                &file_key,
                b"",
                b"owner",
                &salts,
                0xffff_fffc,
                true,
            );
            assert_eq!(e.user.len(), 48);
            assert_eq!(e.owner.len(), 48);
            assert_eq!(e.user_key.len(), 32);
            assert_eq!(e.owner_key.len(), 32);
            assert_eq!(e.perms.len(), 16);
            let params = Params {
                revision,
                key_bits: 256,
                owner: e.owner.clone(),
                user: e.user.clone(),
                owner_key: e.owner_key.clone(),
                user_key: e.user_key.clone(),
                permissions: 0xffff_fffc,
                encrypt_metadata: true,
                streams: Cipher::Aes256,
                strings: Cipher::Aes256,
                file_id: Vec::new(),
            };
            let d = Decryptor::open(&params, b"").expect("user");
            assert_eq!(d.key, file_key);
            assert!(!d.opened_as_owner());
            let o = Decryptor::open(&params, b"owner").expect("owner");
            assert_eq!(o.key, file_key);
            assert!(o.opened_as_owner());
            assert_eq!(Decryptor::open(&params, b"x"), Err(Error::WrongPassword));
            let iv = [9u8; 16];
            let data = b"0123456789abcdef"; // exactly one block: a full padding block follows
            let boxed = d.encrypt_with(Cipher::Aes256, 1, 0, &iv, data);
            assert_eq!(boxed.len(), 48);
            assert_eq!(d.decrypt_with(Cipher::Aes256, 1, 0, &boxed), data);
            assert_eq!(d.decrypt_with(Cipher::Aes256, 1, 0, b""), b"");
            assert_eq!(d.decrypt_with(Cipher::Aes256, 1, 0, &boxed[..20]), b"");
        }
    }

    #[test]
    fn hash_2b_revision_6_is_deterministic() {
        // Real-file validation of algorithm 2.B happens on the corpus
        // (pdf.js and qpdf ship revision 6 files); here: determinism and
        // sensitivity to every input.
        let h = hash_2b(Revision::R6, b"user", &[0, 1, 2, 3, 4, 5, 6, 7], &[]);
        assert_eq!(h.len(), 32);
        // Deterministic and sensitive to every input.
        assert_eq!(
            h,
            hash_2b(Revision::R6, b"user", &[0, 1, 2, 3, 4, 5, 6, 7], &[])
        );
        assert_ne!(
            h,
            hash_2b(Revision::R6, b"usex", &[0, 1, 2, 3, 4, 5, 6, 7], &[])
        );
        assert_ne!(
            h,
            hash_2b(Revision::R6, b"user", &[0, 1, 2, 3, 4, 5, 6, 8], &[])
        );
        assert_ne!(
            h,
            hash_2b(Revision::R6, b"user", &[0, 1, 2, 3, 4, 5, 6, 7], b"u")
        );
        assert_ne!(
            h,
            hash_2b(Revision::R5, b"user", &[0, 1, 2, 3, 4, 5, 6, 7], &[])
        );
    }

    #[test]
    fn bad_parameters_are_refused() {
        let ok = with_passwords(&legacy(Revision::R3, 128, Cipher::Rc4), b"o", b"");
        assert!(Decryptor::open(&ok, b"").is_ok());
        let cases: Vec<(&str, Params)> = vec![
            (
                "length 7",
                Params {
                    key_bits: 7,
                    ..ok.clone()
                },
            ),
            (
                "length 256 with R3",
                Params {
                    key_bits: 256,
                    ..ok.clone()
                },
            ),
            (
                "length 0",
                Params {
                    key_bits: 0,
                    ..ok.clone()
                },
            ),
            (
                "R2 with 128",
                Params {
                    revision: Revision::R2,
                    key_bits: 128,
                    ..ok.clone()
                },
            ),
            (
                "empty /O",
                Params {
                    owner: Vec::new(),
                    ..ok.clone()
                },
            ),
            (
                "empty /U",
                Params {
                    user: Vec::new(),
                    ..ok.clone()
                },
            ),
            (
                "AES-128 with R3",
                Params {
                    streams: Cipher::Aes128,
                    ..ok.clone()
                },
            ),
            (
                "AES-256 with R4",
                Params {
                    revision: Revision::R4,
                    strings: Cipher::Aes256,
                    ..ok.clone()
                },
            ),
            (
                "R6 without /UE",
                Params {
                    revision: Revision::R6,
                    key_bits: 256,
                    owner: vec![0; 48],
                    user: vec![0; 48],
                    streams: Cipher::Aes256,
                    strings: Cipher::Aes256,
                    ..ok.clone()
                },
            ),
        ];
        for (what, params) in cases {
            assert!(
                matches!(Decryptor::open(&params, b""), Err(Error::BadParameters(_))),
                "{what}"
            );
        }
        // A wrong /U of the right size is a wrong password, not bad parameters.
        let wrong = Params {
            user: vec![0; 32],
            ..ok.clone()
        };
        assert_eq!(Decryptor::open(&wrong, b""), Err(Error::WrongPassword));
        // A /U cut to its 16 significant bytes still opens (padded with
        // zeros); a /U cut to 15 bytes does not.
        let short = Params {
            user: ok.user[..16].to_vec(),
            ..ok.clone()
        };
        assert!(Decryptor::open(&short, b"").is_ok());
        let shorter = Params {
            user: ok.user[..15].to_vec(),
            ..ok
        };
        assert_eq!(Decryptor::open(&shorter, b""), Err(Error::WrongPassword));
    }

    #[test]
    fn pkcs5_padding_tolerance() {
        let mut v = vec![1, 2, 3, 4, 4, 4, 4];
        strip_pkcs5(&mut v);
        assert_eq!(v, [1, 2, 3]);
        let mut v = vec![1, 2, 3, 0];
        strip_pkcs5(&mut v);
        assert_eq!(v, [1, 2, 3, 0]);
        let mut v = vec![1, 2, 9];
        strip_pkcs5(&mut v);
        assert_eq!(v, [1, 2, 9]);
        let mut v = Vec::new();
        strip_pkcs5(&mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn debug_hides_the_key() {
        let params = with_passwords(&legacy(Revision::R3, 128, Cipher::Rc4), b"o", b"");
        let d = Decryptor::open(&params, b"").unwrap();
        let text = format!("{d:?}");
        assert!(text.contains("R3"));
        assert!(!text.contains("key: ["));
    }
}
