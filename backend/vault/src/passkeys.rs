//! Passkeys (WebAuthn) kept in the vault and used through the browser
//! extension, as other password managers do: the extension takes the page's
//! `navigator.credentials` calls to MYLE, which answers as the authenticator.
//!
//! Each passkey is a P-256 key (ES256) inside the entry of its site, sealed
//! and synced with it; its signature counter stays 0, as for any passkey
//! that lives on more than one device.
//!
//! What a page cannot do:
//! - name its origin: it comes from the browser (the address of the page),
//!   and `clientDataJSON` is made here from it;
//! - use a site's passkey elsewhere: the relying party must be the page's
//!   own host or a domain above it, never a public suffix (`github.io`);
//! - use one without the user: every use follows a click on MYLE's own
//!   prompt, with Windows Hello when the site asks for user verification.

use base64::Engine;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use p256::ecdsa::{DerSignature, SigningKey, signature::Signer};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

/// Tells relying parties which kind of authenticator made a passkey: MYLE.
const AAGUID: [u8; 16] = [26, 252, 48, 205, 184, 136, 70, 238, 141, 23, 239, 15, 78, 94, 80, 137];
/// ES256: ECDSA on P-256 with SHA-256, the one algorithm every site takes.
pub const ES256: i64 = -7;
const CHALLENGE_MAX: usize = 1024;
const USER_HANDLE_MAX: usize = 64;

// authenticatorData flags.
const USER_PRESENT: u8 = 0x01;
const USER_VERIFIED: u8 = 0x04;
const BACKUP_ELIGIBLE: u8 = 0x08;
const BACKED_UP: u8 = 0x10;
const ATTESTED: u8 = 0x40;

/// A passkey as the vault keeps it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Zeroize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Passkey {
    /// base64url, as sites see it.
    pub credential_id: String,
    pub rp_id: String,
    pub rp_name: String,
    /// The site's id for the account (base64url).
    pub user_handle: String,
    pub user_name: String,
    pub user_display_name: String,
    /// The private key: PKCS#8, base64url.
    pub key: String,
    pub created_at: u64,
}

/// What the page and the extension see of a passkey: never its key.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyInfo {
    pub credential_id: String,
    pub rp_id: String,
    pub user_name: String,
    pub user_display_name: String,
    pub created_at: u64,
}

impl Passkey {
    pub fn info(&self) -> PasskeyInfo {
        PasskeyInfo {
            credential_id: self.credential_id.clone(),
            rp_id: self.rp_id.clone(),
            user_name: self.user_name.clone(),
            user_display_name: self.user_display_name.clone(),
            created_at: self.created_at,
        }
    }
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// base64url, with or without padding.
pub fn unb64(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).or_else(|_| URL_SAFE.decode(text)).ok()
}

/// The relying party a page on `host` may use: `asked` (the page's
/// `rp.id`/`rpId`) or the host itself. It must be the host or a domain
/// above it, never a public suffix or an IP address; `localhost` is allowed
/// for local development.
pub fn rp_id_for(host: &str, asked: Option<&str>) -> Result<String, String> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let rp_id = asked
        .map(|asked| asked.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|asked| !asked.is_empty())
        .unwrap_or_else(|| host.clone());
    if host != rp_id && !host.ends_with(&format!(".{rp_id}")) {
        return Err("rpMismatch".into());
    }
    if rp_id == "localhost" {
        return Ok(rp_id);
    }
    if rp_id.parse::<std::net::IpAddr>().is_ok() || rp_id.starts_with('[') || psl::domain_str(&rp_id).is_none() {
        return Err("rpMismatch".into());
    }
    Ok(rp_id)
}

/// The page's origin (`https://example.com:8443`), from its address.
pub fn origin_of(url: &str) -> Option<String> {
    let origin = reqwest::Url::parse(url).ok()?.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

/// `clientDataJSON`, in the order the WebAuthn spec writes it.
fn client_data(kind: &str, challenge: &str, origin: &str) -> String {
    format!(
        r#"{{"type":{},"challenge":{},"origin":{},"crossOrigin":false}}"#,
        Value::from(kind),
        Value::from(challenge),
        Value::from(origin)
    )
}

/// A challenge as the page sent it (base64url), checked.
fn challenge(text: &str) -> Result<String, String> {
    let bytes = unb64(text).ok_or("badRequest")?;
    if bytes.is_empty() || bytes.len() > CHALLENGE_MAX {
        return Err("badRequest".into());
    }
    // Written back as the browser would: base64url without padding.
    Ok(b64(&bytes))
}

fn auth_data(rp_id: &str, flags: u8, credential: Option<(&[u8], &[u8])>) -> Vec<u8> {
    let mut data = Sha256::digest(rp_id.as_bytes()).to_vec();
    data.push(flags);
    data.extend(0u32.to_be_bytes());
    if let Some((id, public_key)) = credential {
        data.extend(AAGUID);
        data.extend((id.len() as u16).to_be_bytes());
        data.extend(id);
        data.extend(public_key);
    }
    data
}

fn flags(user_verified: bool) -> u8 {
    USER_PRESENT | BACKUP_ELIGIBLE | BACKED_UP | if user_verified { USER_VERIFIED } else { 0 }
}

/// The few CBOR forms WebAuthn needs, in canonical form.
mod cbor {
    fn head(out: &mut Vec<u8>, major: u8, value: u64) {
        let major = major << 5;
        match value {
            0..=23 => out.push(major | value as u8),
            24..=0xff => out.extend([major | 24, value as u8]),
            0x100..=0xffff => {
                out.push(major | 25);
                out.extend((value as u16).to_be_bytes());
            }
            _ => {
                out.push(major | 26);
                out.extend((value as u32).to_be_bytes());
            }
        }
    }

    pub fn int(out: &mut Vec<u8>, value: i64) {
        if value >= 0 {
            head(out, 0, value as u64);
        } else {
            head(out, 1, (-1 - value) as u64);
        }
    }

    pub fn bytes(out: &mut Vec<u8>, bytes: &[u8]) {
        head(out, 2, bytes.len() as u64);
        out.extend(bytes);
    }

    pub fn text(out: &mut Vec<u8>, text: &str) {
        head(out, 3, text.len() as u64);
        out.extend(text.as_bytes());
    }

    pub fn map(out: &mut Vec<u8>, entries: u64) {
        head(out, 5, entries);
    }
}

/// The public key as COSE_Key (EC2, P-256, ES256).
fn cose_key(key: &SigningKey) -> Vec<u8> {
    let point = key.verifying_key().to_sec1_point(false);
    let (x, y) = (point.x().expect("an uncompressed point"), point.y().expect("an uncompressed point"));
    let mut out = Vec::with_capacity(77);
    cbor::map(&mut out, 5);
    cbor::int(&mut out, 1); // kty: EC2
    cbor::int(&mut out, 2);
    cbor::int(&mut out, 3); // alg: ES256
    cbor::int(&mut out, ES256);
    cbor::int(&mut out, -1); // crv: P-256
    cbor::int(&mut out, 1);
    cbor::int(&mut out, -2);
    cbor::bytes(&mut out, x);
    cbor::int(&mut out, -3);
    cbor::bytes(&mut out, y);
    out
}

fn new_key() -> SigningKey {
    loop {
        let mut secret = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(secret.as_mut());
        // Out of range only with a chance of about 2^-32: draw again.
        if let Ok(key) = SigningKey::from_slice(secret.as_ref()) {
            return key;
        }
    }
}

/// The account a new passkey is for, as the site names it.
pub struct User {
    /// base64url.
    pub handle: String,
    pub name: String,
    pub display_name: String,
}

/// A new passkey, and the browser's answer to `navigator.credentials.create()`.
pub fn create(
    rp_id: &str,
    rp_name: &str,
    user: &User,
    challenge_b64: &str,
    origin: &str,
    user_verified: bool,
    now: u64,
) -> Result<(Passkey, Value), String> {
    let handle = unb64(&user.handle).ok_or("badRequest")?;
    if handle.is_empty() || handle.len() > USER_HANDLE_MAX {
        return Err("badRequest".into());
    }
    let challenge = challenge(challenge_b64)?;
    let key = new_key();
    let mut credential_id = [0u8; 16];
    OsRng.fill_bytes(&mut credential_id);
    let client_data = client_data("webauthn.create", &challenge, origin);
    let auth_data = auth_data(rp_id, flags(user_verified) | ATTESTED, Some((&credential_id, &cose_key(&key))));
    let mut attestation = Vec::with_capacity(auth_data.len() + 32);
    cbor::map(&mut attestation, 3);
    cbor::text(&mut attestation, "fmt");
    cbor::text(&mut attestation, "none");
    cbor::text(&mut attestation, "attStmt");
    cbor::map(&mut attestation, 0);
    cbor::text(&mut attestation, "authData");
    cbor::bytes(&mut attestation, &auth_data);
    let public_key = key.verifying_key().to_public_key_der().map_err(|e| e.to_string())?;
    let private_key = key.to_pkcs8_der().map_err(|e| e.to_string())?;
    let id = b64(&credential_id);
    let passkey = Passkey {
        credential_id: id.clone(),
        rp_id: rp_id.to_string(),
        rp_name: rp_name.trim().chars().take(200).collect(),
        user_handle: b64(&handle),
        user_name: user.name.trim().chars().take(200).collect(),
        user_display_name: user.display_name.trim().chars().take(200).collect(),
        key: b64(private_key.as_bytes()),
        created_at: now,
    };
    let response = json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "authenticatorAttachment": "platform",
        "response": {
            "clientDataJSON": b64(client_data.as_bytes()),
            "attestationObject": b64(&attestation),
            "authenticatorData": b64(&auth_data),
            "publicKey": b64(public_key.as_bytes()),
            "publicKeyAlgorithm": ES256,
            "transports": ["hybrid", "internal"],
        },
    });
    Ok((passkey, response))
}

/// The browser's answer to `navigator.credentials.get()` with `passkey`.
pub fn sign_in(passkey: &Passkey, challenge_b64: &str, origin: &str, user_verified: bool) -> Result<Value, String> {
    let challenge = challenge(challenge_b64)?;
    let der = Zeroizing::new(unb64(&passkey.key).ok_or("This passkey is damaged.")?);
    let key = SigningKey::from_pkcs8_der(&der).map_err(|_| "This passkey is damaged.")?;
    let client_data = client_data("webauthn.get", &challenge, origin);
    let auth_data = auth_data(&passkey.rp_id, flags(user_verified), None);
    let mut signed = auth_data.clone();
    signed.extend(Sha256::digest(client_data.as_bytes()));
    let signature: DerSignature = key.sign(&signed);
    Ok(json!({
        "id": passkey.credential_id,
        "rawId": passkey.credential_id,
        "type": "public-key",
        "authenticatorAttachment": "platform",
        "response": {
            "clientDataJSON": b64(client_data.as_bytes()),
            "authenticatorData": b64(&auth_data),
            "signature": b64(signature.as_bytes()),
            "userHandle": passkey.user_handle,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;
    use p256::pkcs8::DecodePublicKey;

    fn user() -> User {
        User { handle: b64(b"user-1234"), name: "thomas@example.com".into(), display_name: "Thomas".into() }
    }

    fn field(value: &Value, path: &str) -> Vec<u8> {
        unb64(value.pointer(path).and_then(Value::as_str).unwrap()).unwrap()
    }

    #[test]
    fn a_page_uses_only_its_own_site_and_never_a_public_suffix() {
        assert_eq!(rp_id_for("github.com", None).unwrap(), "github.com");
        assert_eq!(rp_id_for("login.example.com", Some("example.com")).unwrap(), "example.com");
        assert_eq!(rp_id_for("Login.Example.com.", Some("EXAMPLE.com")).unwrap(), "example.com");
        assert_eq!(rp_id_for("localhost", None).unwrap(), "localhost");
        assert!(rp_id_for("example.com", Some("login.example.com")).is_err(), "not below the page");
        assert!(rp_id_for("evil.com", Some("example.com")).is_err());
        assert!(rp_id_for("notexample.com", Some("example.com")).is_err());
        assert!(rp_id_for("attacker.github.io", Some("github.io")).is_err(), "a public suffix");
        assert!(rp_id_for("shop.example.co.uk", Some("co.uk")).is_err());
        assert!(rp_id_for("127.0.0.1", None).is_err(), "no IP addresses");
        assert_eq!(origin_of("https://example.com:8443/login?x=1").as_deref(), Some("https://example.com:8443"));
        assert_eq!(origin_of("https://example.com/login").as_deref(), Some("https://example.com"));
    }

    #[test]
    fn a_new_passkey_is_an_es256_key_its_site_can_verify() {
        let challenge = b64(&[7u8; 32]);
        let (passkey, made) = create("example.com", "Example", &user(), &challenge, "https://example.com", true, 1).unwrap();
        assert_eq!(passkey.rp_id, "example.com");
        assert_eq!(made["id"], passkey.credential_id);

        // clientDataJSON: what the site checks first.
        let client: Value = serde_json::from_slice(&field(&made, "/response/clientDataJSON")).unwrap();
        assert_eq!(client["type"], "webauthn.create");
        assert_eq!(client["challenge"], challenge);
        assert_eq!(client["origin"], "https://example.com");

        // authenticatorData: the site's hash, the flags, counter 0, then the key.
        let data = field(&made, "/response/authenticatorData");
        assert_eq!(&data[..32], Sha256::digest(b"example.com").as_slice());
        assert_eq!(data[32], USER_PRESENT | USER_VERIFIED | BACKUP_ELIGIBLE | BACKED_UP | ATTESTED);
        assert_eq!(&data[33..37], &[0, 0, 0, 0]);
        assert_eq!(&data[37..53], &AAGUID);
        assert_eq!(u16::from_be_bytes([data[53], data[54]]), 16);
        assert_eq!(b64(&data[55..71]), passkey.credential_id);
        // The COSE key: a map of five, EC2 / ES256 / P-256, then x and y.
        assert_eq!(&data[71..78], &[0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01]);
        assert_eq!(data.len(), 71 + 77);

        // The attestation object holds the same authenticatorData, "none".
        let attestation = field(&made, "/response/attestationObject");
        assert_eq!(&attestation[..1], &[0xa3]);
        assert!(attestation.windows(4).any(|w| w == b"none"));
        assert!(attestation.ends_with(&data));

        let public = VerifyingKey::from_public_key_der(&field(&made, "/response/publicKey")).unwrap();
        assert_eq!(made["response"]["publicKeyAlgorithm"], ES256);

        // Signing in: the site verifies with the key it was given.
        let challenge = b64(b"a fresh challenge from the site");
        let signed = sign_in(&passkey, &challenge, "https://example.com", false).unwrap();
        let data = field(&signed, "/response/authenticatorData");
        assert_eq!(data.len(), 37, "no key this time");
        assert_eq!(data[32], USER_PRESENT | BACKUP_ELIGIBLE | BACKED_UP);
        let client = field(&signed, "/response/clientDataJSON");
        let mut message = data.clone();
        message.extend(Sha256::digest(&client));
        let signature = DerSignature::from_bytes(&field(&signed, "/response/signature")).unwrap();
        public.verify(&message, &signature).expect("a valid ES256 signature");
        assert_eq!(field(&signed, "/response/userHandle"), b"user-1234");
        let client: Value = serde_json::from_slice(&client).unwrap();
        assert_eq!(client["type"], "webauthn.get");
        // Anything else signed does not verify.
        assert!(public.verify(b"something else", &signature).is_err());
    }

    #[test]
    fn what_a_page_sends_is_checked() {
        let challenge = b64(&[1u8; 32]);
        let mut bad = user();
        bad.handle = b64(&[0u8; 65]);
        assert!(create("example.com", "", &bad, &challenge, "https://example.com", false, 0).is_err(), "handle too long");
        assert!(create("example.com", "", &user(), "not base64!", "https://example.com", false, 0).is_err());
        assert!(create("example.com", "", &user(), &b64(&[0u8; 2000]), "https://example.com", false, 0).is_err());
        let damaged = Passkey { key: b64(b"not a key"), ..Passkey::default() };
        assert!(sign_in(&damaged, &challenge, "https://example.com", false).is_err());
    }

    #[test]
    fn cbor_is_written_canonically() {
        let mut out = Vec::new();
        cbor::int(&mut out, 23);
        cbor::int(&mut out, 24);
        cbor::int(&mut out, -1);
        cbor::int(&mut out, -25);
        cbor::int(&mut out, 1000);
        cbor::text(&mut out, "fmt");
        cbor::map(&mut out, 0);
        assert_eq!(out, [0x17, 0x18, 0x18, 0x20, 0x38, 0x18, 0x19, 0x03, 0xe8, 0x63, b'f', b'm', b't', 0xa0]);
    }
}
