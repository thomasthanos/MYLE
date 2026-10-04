//! Two-factor codes (TOTP, RFC 6238): the six-digit code an authenticator
//! app shows for a login, new every 30 seconds.
//!
//! An entry keeps the secret like its password, sealed, as an
//! `otpauth://totp/…` link. The page only ever gets the codes; the secret
//! reaches it only while the user types or scans it into the editor.

use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Shorter secrets than this are no real 2FA key (80 bits is the oldest
/// size in use; most are 160).
const MIN_SECRET: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }
}

/// A login's 2FA key and how its codes are made.
#[derive(Clone, Debug, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Totp {
    secret: Vec<u8>,
    #[zeroize(skip)]
    pub digits: u32,
    #[zeroize(skip)]
    pub period: u64,
    #[zeroize(skip)]
    pub algorithm: Algorithm,
    #[zeroize(skip)]
    pub issuer: String,
    #[zeroize(skip)]
    pub account: String,
}

/// A code, and how long it still holds.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Code {
    pub code: String,
    pub period: u64,
    /// Seconds until the next code.
    pub remaining: u64,
}

/// What the editor shows about a key it was given: never the key.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub issuer: String,
    pub account: String,
    pub digits: u32,
    pub period: u64,
    pub algorithm: &'static str,
}

impl Totp {
    /// A key as a site gives it: the `otpauth://totp/…` link of its QR
    /// code, or the key alone ("JBSW Y3DP EHPK 3PXP"), with the usual
    /// settings (6 digits, 30 seconds, SHA-1).
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.to_ascii_lowercase().starts_with("otpauth://") {
            return Self::parse_link(text);
        }
        let secret = base32_decode(text).ok_or(
            "That is not a 2FA key: use the key the site shows (letters A–Z and digits 2–7) or its otpauth:// link.",
        )?;
        Self::new(secret, 6, 30, Algorithm::Sha1, String::new(), String::new())
    }

    fn parse_link(text: &str) -> Result<Self, String> {
        let url = reqwest::Url::parse(text).map_err(|_| "That otpauth:// link is not complete.")?;
        match url.host_str().map(str::to_ascii_lowercase).as_deref() {
            Some("totp") => {}
            Some("hotp") => return Err("Only time-based codes (TOTP) are supported, not counter-based ones (HOTP).".into()),
            _ => return Err("That otpauth:// link is not for 2FA codes.".into()),
        }
        let label = percent_decode(url.path().trim_start_matches('/'));
        let (mut issuer, account) = match label.split_once(':') {
            Some((issuer, account)) => (issuer.trim().to_string(), account.trim().to_string()),
            None => (String::new(), label.trim().to_string()),
        };
        let mut secret = None;
        let (mut digits, mut period, mut algorithm) = (6, 30, Algorithm::Sha1);
        for (key, value) in url.query_pairs() {
            match key.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(Zeroizing::new(value.into_owned())),
                "issuer" if !value.trim().is_empty() => issuer = value.trim().to_string(),
                "digits" => digits = value.parse().map_err(|_| "The link's number of digits is not a number.")?,
                "period" => period = value.parse().map_err(|_| "The link's period is not a number.")?,
                "algorithm" => {
                    algorithm = match value.to_ascii_uppercase().as_str() {
                        "SHA1" => Algorithm::Sha1,
                        "SHA256" => Algorithm::Sha256,
                        "SHA512" => Algorithm::Sha512,
                        _ => return Err(format!("MYLE cannot make codes with {value}.")),
                    }
                }
                _ => {}
            }
        }
        let secret = secret.ok_or("That otpauth:// link has no key in it.")?;
        let secret = base32_decode(&secret).ok_or("The key in that otpauth:// link is not valid.")?;
        Self::new(secret, digits, period, algorithm, issuer, account)
    }

    fn new(secret: Vec<u8>, digits: u32, period: u64, algorithm: Algorithm, issuer: String, account: String) -> Result<Self, String> {
        let totp = Self { secret, digits, period, algorithm, issuer, account };
        if totp.secret.len() < MIN_SECRET {
            return Err("That 2FA key is too short. Copy the whole key the site shows.".into());
        }
        if !(6..=8).contains(&totp.digits) {
            return Err("Codes have 6 to 8 digits.".into());
        }
        if !(10..=300).contains(&totp.period) {
            return Err("Codes change every 10 seconds to 5 minutes.".into());
        }
        Ok(totp)
    }

    /// Fills in whose key it is when the site gave only the key.
    pub fn name_if_unnamed(&mut self, issuer: &str, account: &str) {
        if self.issuer.is_empty() && self.account.is_empty() {
            self.issuer = issuer.trim().to_string();
            self.account = account.trim().to_string();
        }
    }

    /// The `otpauth://` link this key is kept as.
    pub fn to_link(&self) -> Zeroizing<String> {
        let label = match (self.issuer.is_empty(), self.account.is_empty()) {
            (false, false) => format!("{}:{}", self.issuer, self.account),
            (false, true) => self.issuer.clone(),
            _ => self.account.clone(),
        };
        let mut url = reqwest::Url::parse("otpauth://totp/").expect("a fixed link");
        url.set_path(&format!("/{label}"));
        {
            let secret = Zeroizing::new(base32_encode(&self.secret));
            let mut query = url.query_pairs_mut();
            query.append_pair("secret", &secret);
            if !self.issuer.is_empty() {
                query.append_pair("issuer", &self.issuer);
            }
            query
                .append_pair("algorithm", self.algorithm.name())
                .append_pair("digits", &self.digits.to_string())
                .append_pair("period", &self.period.to_string());
        }
        Zeroizing::new(url.to_string())
    }

    /// The same key, whatever the label or settings it came with.
    pub fn same_key(&self, other: &Totp) -> bool {
        self.secret == other.secret
    }

    pub fn info(&self) -> Info {
        Info {
            issuer: self.issuer.clone(),
            account: self.account.clone(),
            digits: self.digits,
            period: self.period,
            algorithm: self.algorithm.name(),
        }
    }

    /// The code for `unix` (seconds since 1970).
    pub fn code_at(&self, unix: u64) -> String {
        let counter = (unix / self.period).to_be_bytes();
        let hash = match self.algorithm {
            Algorithm::Sha1 => sign::<Hmac<sha1::Sha1>>(&self.secret, &counter),
            Algorithm::Sha256 => sign::<Hmac<sha2::Sha256>>(&self.secret, &counter),
            Algorithm::Sha512 => sign::<Hmac<sha2::Sha512>>(&self.secret, &counter),
        };
        // RFC 4226's dynamic truncation: four bytes from where the last
        // byte points, without the sign bit.
        let at = usize::from(hash[hash.len() - 1] & 0x0f);
        let number = u32::from_be_bytes([hash[at] & 0x7f, hash[at + 1], hash[at + 2], hash[at + 3]]);
        format!("{:0width$}", number % 10u32.pow(self.digits), width = self.digits as usize)
    }

    pub fn now(&self) -> Code {
        let unix = super::vault::now();
        Code {
            code: self.code_at(unix),
            period: self.period,
            remaining: self.period - unix % self.period,
        }
    }
}

fn sign<M: Mac + KeyInit>(key: &[u8], message: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut mac = <M as KeyInit>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(message);
    Zeroizing::new(mac.finalize().into_bytes().to_vec())
}

/// RFC 4648 base32, as 2FA keys are written: any case, spaces, dashes and
/// padding allowed.
fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut bits, mut count) = (0u32, 0u32);
    let mut any = false;
    for c in text.chars().filter(|c| !matches!(c, ' ' | '-' | '=' | '\t')) {
        let value = match c.to_ascii_uppercase() {
            letter @ 'A'..='Z' => letter as u32 - 'A' as u32,
            digit @ '2'..='7' => digit as u32 - '2' as u32 + 26,
            _ => return None,
        };
        any = true;
        bits = (bits << 5) | value;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    any.then_some(out)
}

fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut bits, mut count) = (0u32, 0u32);
    for &byte in bytes {
        bits = (bits << 8) | u32::from(byte);
        count += 8;
        while count >= 5 {
            count -= 5;
            out.push(ALPHABET[((bits >> count) & 31) as usize] as char);
        }
        bits &= (1 << count) - 1;
    }
    if count > 0 {
        out.push(ALPHABET[((bits << (5 - count)) & 31) as usize] as char);
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238, appendix B: the same 8-digit codes for the same times.
    #[test]
    fn the_rfc_test_vectors_give_the_same_codes() {
        let key = |seed: &[u8], length: usize| seed.iter().copied().cycle().take(length).collect::<Vec<u8>>();
        let make = |secret: Vec<u8>, algorithm| Totp::new(secret, 8, 30, algorithm, String::new(), String::new()).unwrap();
        let sha1 = make(key(b"12345678901234567890", 20), Algorithm::Sha1);
        let sha256 = make(key(b"12345678901234567890", 32), Algorithm::Sha256);
        let sha512 = make(key(b"12345678901234567890", 64), Algorithm::Sha512);
        let vectors: [(u64, &str, &str, &str); 6] = [
            (59, "94287082", "46119246", "90693936"),
            (1_111_111_109, "07081804", "68084774", "25091201"),
            (1_111_111_111, "14050471", "67062674", "99943326"),
            (1_234_567_890, "89005924", "91819424", "93441116"),
            (2_000_000_000, "69279037", "90698825", "38618901"),
            (20_000_000_000, "65353130", "77737706", "47863826"),
        ];
        for (time, one, two, five) in vectors {
            assert_eq!(sha1.code_at(time), one, "SHA1 at {time}");
            assert_eq!(sha256.code_at(time), two, "SHA256 at {time}");
            assert_eq!(sha512.code_at(time), five, "SHA512 at {time}");
        }
    }

    #[test]
    fn a_key_alone_or_a_link_is_read_and_kept_as_a_link() {
        // The key a site shows, as people paste it.
        let plain = Totp::parse(" jbsw y3dp ehpk 3pxp jbsw y3dp ehpk 3pxp ").unwrap();
        assert_eq!(plain.secret, b"Hello!\xde\xad\xbe\xefHello!\xde\xad\xbe\xef");
        assert_eq!((plain.digits, plain.period, plain.algorithm), (6, 30, Algorithm::Sha1));
        assert_eq!(plain.code_at(59).len(), 6);

        let link = "otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA1&digits=6&period=30";
        let totp = Totp::parse(link).unwrap();
        assert_eq!((totp.issuer.as_str(), totp.account.as_str()), ("ACME Co", "john.doe@email.com"));
        // Kept as a link, and read back the same.
        let kept = totp.to_link();
        assert!(kept.starts_with("otpauth://totp/"), "{}", kept.as_str());
        assert_eq!(Totp::parse(&kept).unwrap(), totp);

        let mut named = Totp::parse("HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ").unwrap();
        named.name_if_unnamed("GitHub", "thomas");
        assert_eq!(Totp::parse(&named.to_link()).unwrap().issuer, "GitHub");
    }

    #[test]
    fn what_is_not_a_2fa_key_is_refused() {
        assert!(Totp::parse("").is_err());
        assert!(Totp::parse("hunter2").is_err(), "1, 8, 9 and 0 are not base32");
        assert!(Totp::parse("ABCDEFGH").is_err(), "too short");
        assert!(Totp::parse("otpauth://hotp/x?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&counter=1").unwrap_err().contains("HOTP"));
        assert!(Totp::parse("otpauth://totp/x?issuer=y").is_err(), "no key");
        assert!(Totp::parse("otpauth://totp/x?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&digits=4").is_err());
        assert!(Totp::parse("otpauth://totp/x?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&algorithm=MD5").is_err());
    }

    #[test]
    fn the_same_key_is_known_however_it_is_written() {
        let link = Totp::parse("otpauth://totp/Site:me?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=Site").unwrap();
        let plain = Totp::parse("hxdm vjec jjws rb3h wizr 4ifu gftm xboz").unwrap();
        assert!(link.same_key(&plain));
        assert!(!link.same_key(&Totp::parse("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP").unwrap()));
    }

    #[test]
    fn base32_round_trips() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(base32_decode(&base32_encode(bytes)).unwrap_or_default(), bytes);
        }
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn the_code_says_how_long_it_holds() {
        let totp = Totp::parse("HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ").unwrap();
        let code = totp.now();
        assert_eq!(code.code.len(), 6);
        assert!((1..=30).contains(&code.remaining));
    }
}
