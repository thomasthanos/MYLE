//! The Password Manager's cryptography.
//!
//! - The master password becomes a 256-bit master key through Argon2id.
//! - A random vault key encrypts every entry; it is stored only wrapped
//!   (encrypted) by the master key, and again by the recovery key. Changing
//!   the master password re-wraps it: no entry is encrypted again.
//! - Every seal is XChaCha20-Poly1305 with a random 192-bit nonce and
//!   associated data naming what it is (an entry's id and revision), so a
//!   ciphertext cannot be moved to another entry or another revision.
//!
//! Key material lives in `Key`, which wipes itself when dropped.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Marks the format, so a later change can still read what this one wrote.
const SEAL_VERSION: &str = "v1";

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; 32]);

impl Key {
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    fn from_slice(bytes: &[u8]) -> Result<Self, String> {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "A vault key has the wrong length.".to_string())?;
        Ok(Self(bytes))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.0).into())
    }
}

/// How the master key was derived, stored next to the wrapped vault key.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub algorithm: String,
    /// Memory in KiB.
    pub memory: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt: String,
}

impl KdfParams {
    /// 64 MiB and 3 passes: about half a second on a typical PC, and costly
    /// to guess at scale.
    #[allow(clippy::new_without_default, reason = "each call makes a new random salt")]
    pub fn new() -> Self {
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        Self {
            algorithm: "argon2id".into(),
            memory: 64 * 1024,
            iterations: 3,
            parallelism: 1,
            salt: B64.encode(salt),
        }
    }

    #[cfg(test)]
    pub fn cheap_for_tests() -> Self {
        Self {
            memory: 256,
            iterations: 1,
            ..Self::new()
        }
    }
}

/// The master key for `password`.
pub fn derive(password: &str, params: &KdfParams) -> Result<Key, String> {
    if params.algorithm != "argon2id" {
        return Err("This vault uses a key derivation this version does not know.".into());
    }
    let salt = B64
        .decode(&params.salt)
        .map_err(|_| "The vault's salt is damaged.".to_string())?;
    let argon = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(
            params.memory,
            params.iterations,
            params.parallelism,
            Some(32),
        )
        .map_err(|e| e.to_string())?,
    );
    let mut out = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), &salt, &mut out)
        .map_err(|e| e.to_string())?;
    let key = Key(out);
    out.zeroize();
    Ok(key)
}

/// The key a Windows Hello signature stands for (`hello.rs`): the same
/// signature always gives the same key.
pub fn key_from_signature(signature: &[u8]) -> Key {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"myle-hello-v1\0");
    hash.update(signature);
    let mut bytes: [u8; 32] = hash.finalize().into();
    let key = Key(bytes);
    bytes.zeroize();
    key
}

/// `v1.<nonce>.<ciphertext>`, both base64.
pub fn seal(key: &Key, aad: &str, plain: &[u8]) -> Result<String, String> {
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let sealed = key
        .cipher()
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "Encryption failed.".to_string())?;
    Ok(format!(
        "{SEAL_VERSION}.{}.{}",
        B64.encode(nonce),
        B64.encode(sealed)
    ))
}

/// The plaintext, or an error when the key is wrong, the data was changed,
/// or it belongs to something else (`aad`).
pub fn open(key: &Key, aad: &str, sealed: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut parts = sealed.splitn(3, '.');
    let (Some(SEAL_VERSION), Some(nonce), Some(data)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err("The encrypted data is in an unknown format.".into());
    };
    let nonce = B64
        .decode(nonce)
        .map_err(|_| "The encrypted data is damaged.".to_string())?;
    let data = B64
        .decode(data)
        .map_err(|_| "The encrypted data is damaged.".to_string())?;
    if nonce.len() != 24 {
        return Err("The encrypted data is damaged.".into());
    }
    key.cipher()
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: aad.as_bytes(),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| "Wrong password, or the data was changed.".to_string())
}

/// `key` encrypted under `with`.
pub fn wrap(with: &Key, aad: &str, key: &Key) -> Result<String, String> {
    seal(with, aad, &key.0)
}

pub fn unwrap(with: &Key, aad: &str, wrapped: &str) -> Result<Key, String> {
    Key::from_slice(&open(with, aad, wrapped)?)
}

/// Crockford base32 without padding: no I, L, O or U to misread.
const RECOVERY_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A random recovery code, 32 bytes as 52 characters in groups of four
/// (`ABCD-EFGH-...`), and the key it stands for.
pub fn new_recovery_code() -> (Zeroizing<String>, Key) {
    let key = Key::random();
    (Zeroizing::new(format_recovery(&key.0)), key)
}

fn format_recovery(bytes: &[u8; 32]) -> String {
    let mut bits = 0u32;
    let mut count = 0;
    let mut chars = Vec::with_capacity(52);
    for &byte in bytes {
        bits = (bits << 8) | u32::from(byte);
        count += 8;
        while count >= 5 {
            count -= 5;
            chars.push(RECOVERY_ALPHABET[((bits >> count) & 31) as usize]);
        }
    }
    if count > 0 {
        chars.push(RECOVERY_ALPHABET[((bits << (5 - count)) & 31) as usize]);
    }
    chars
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// The key a recovery code stands for. Case, spaces and dashes do not
/// matter, and O/I/L read as 0/1/1.
pub fn parse_recovery_code(code: &str) -> Result<Key, String> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Zeroizing::new(Vec::with_capacity(33));
    let mut symbols = 0;
    for c in code.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let value = RECOVERY_ALPHABET
            .iter()
            .position(|&a| a as char == c)
            .ok_or("The recovery code has a character it cannot contain.")?;
        symbols += 1;
        bits = (bits << 5) | value as u32;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    if symbols != 52 {
        return Err("The recovery code should have 52 characters.".into());
    }
    Key::from_slice(&out[..32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seal_opens_only_with_its_key_and_its_associated_data() {
        let key = Key::random();
        let sealed = seal(&key, "item|a|1", b"secret").unwrap();
        assert_eq!(
            open(&key, "item|a|1", &sealed).unwrap().as_slice(),
            b"secret"
        );
        // Moved to another entry, or another revision of it.
        assert!(open(&key, "item|b|1", &sealed).is_err());
        assert!(open(&key, "item|a|2", &sealed).is_err());
        // Another key.
        assert!(open(&Key::random(), "item|a|1", &sealed).is_err());
        // One changed byte.
        let mut changed = sealed.clone().into_bytes();
        let last = changed.len() - 3;
        changed[last] = if changed[last] == b'A' { b'B' } else { b'A' };
        assert!(open(&key, "item|a|1", &String::from_utf8(changed).unwrap()).is_err());
    }

    #[test]
    fn the_master_password_unwraps_the_vault_key_and_nothing_else_does() {
        let params = KdfParams::cheap_for_tests();
        let vault = Key::random();
        let master = derive("correct horse", &params).unwrap();
        let wrapped = wrap(&master, "vault", &vault).unwrap();
        let again = derive("correct horse", &params).unwrap();
        assert_eq!(unwrap(&again, "vault", &wrapped).unwrap().0, vault.0);
        let wrong = derive("correct horse!", &params).unwrap();
        assert!(unwrap(&wrong, "vault", &wrapped).is_err());
    }

    #[test]
    fn a_windows_hello_signature_always_gives_the_same_key() {
        let a = key_from_signature(b"signature bytes");
        let b = key_from_signature(b"signature bytes");
        let c = key_from_signature(b"other signature");
        assert_eq!(a.0, b.0);
        assert_ne!(a.0, c.0);
        let wrapped = wrap(&a, "hello", &Key::random()).unwrap();
        assert!(unwrap(&b, "hello", &wrapped).is_ok());
        assert!(unwrap(&c, "hello", &wrapped).is_err());
    }

    #[test]
    fn recovery_codes_round_trip_and_forgive_how_they_are_typed() {
        let (code, key) = new_recovery_code();
        assert_eq!(code.len(), 52 + 12);
        assert_eq!(parse_recovery_code(&code).unwrap().0, key.0);
        let sloppy = code.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(parse_recovery_code(&sloppy).unwrap().0, key.0);
        assert!(parse_recovery_code("ABCD-EFGH").is_err());
        assert!(parse_recovery_code(&code.replace(|c: char| c.is_ascii_digit(), "U")).is_err());
    }
}
