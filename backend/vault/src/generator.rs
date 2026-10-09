//! Strong passwords and passphrases from the operating system's random
//! source, and a rough strength rating for the ones already saved.
//!
//! Passphrases use the EFF's large wordlist (7,776 words, CC BY 3.0 US,
//! https://www.eff.org/dice): its four hyphenated words are written without
//! the hyphen, and "yoyo" appears once, so 7,775 words of about 12.9 bits
//! each.

use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/~";
/// Easy to mistake for one another when read or typed by hand.
const AMBIGUOUS: &str = "Il1O0o|`'\";:,.";
const WORDLIST: &str = include_str!("eff_large_wordlist.txt");

/// What kind of secret to make.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Random characters.
    #[default]
    Password,
    /// Random words, easier to read out and type.
    Passphrase,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    #[serde(default)]
    pub kind: Kind,
    pub length: usize,
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
    #[serde(default)]
    pub avoid_ambiguous: bool,
    /// Passphrases: how many words (3 to 12).
    #[serde(default = "default_words")]
    pub words: usize,
    /// Passphrases: what goes between the words (up to 3 characters).
    #[serde(default = "default_separator")]
    pub separator: String,
    /// Passphrases: every word starts with a capital.
    #[serde(default)]
    pub capitalize: bool,
    /// Passphrases: one word gets a digit after it.
    #[serde(default)]
    pub number: bool,
}

fn default_words() -> usize {
    5
}

fn default_separator() -> String {
    "-".into()
}

impl Default for Options {
    fn default() -> Self {
        Self {
            kind: Kind::Password,
            length: 20,
            lower: true,
            upper: true,
            digits: true,
            symbols: true,
            avoid_ambiguous: false,
            words: default_words(),
            separator: default_separator(),
            capitalize: false,
            number: false,
        }
    }
}

fn words() -> impl Iterator<Item = &'static str> {
    WORDLIST.lines().map(str::trim).filter(|word| !word.is_empty())
}

/// A uniformly random number below `bound`, without modulo bias.
fn below(bound: usize) -> usize {
    let bound = bound as u64;
    let zone = u64::MAX - (u64::MAX % bound);
    loop {
        let value = OsRng.next_u64();
        if value < zone {
            return (value % bound) as usize;
        }
    }
}

pub fn generate(options: &Options) -> Result<Zeroizing<String>, String> {
    match options.kind {
        Kind::Password => password(options),
        Kind::Passphrase => passphrase(options),
    }
}

/// Words from the list, joined by the separator.
fn passphrase(options: &Options) -> Result<Zeroizing<String>, String> {
    let list: Vec<&str> = words().collect();
    let count = options.words.clamp(3, 12);
    // Up to 3 characters, and none that would be hard to type or see
    // (spaces, which some sites trim, included).
    let separator: String = options
        .separator
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace() && !c.is_alphanumeric())
        .take(3)
        .collect();
    let with_number = options.number.then(|| below(count));
    let mut phrase = Zeroizing::new(String::new());
    for i in 0..count {
        if i > 0 {
            phrase.push_str(&separator);
        }
        let word = list[below(list.len())];
        if options.capitalize {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                phrase.extend(first.to_uppercase());
                phrase.push_str(chars.as_str());
            }
        } else {
            phrase.push_str(word);
        }
        if with_number == Some(i) {
            phrase.push(char::from(b'0' + below(10) as u8));
        }
    }
    Ok(phrase)
}

fn password(options: &Options) -> Result<Zeroizing<String>, String> {
    let length = options.length.clamp(8, 128);
    let keep = |set: &str| -> Vec<char> {
        set.chars()
            .filter(|c| !options.avoid_ambiguous || !AMBIGUOUS.contains(*c))
            .collect()
    };
    let sets: Vec<Vec<char>> = [
        (options.lower, LOWER),
        (options.upper, UPPER),
        (options.digits, DIGITS),
        (options.symbols, SYMBOLS),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, set)| keep(set))
    .filter(|set| !set.is_empty())
    .collect();
    if sets.is_empty() {
        return Err("Choose at least one kind of character.".into());
    }
    let all: Vec<char> = sets.iter().flatten().copied().collect();

    // One from every chosen kind, the rest from all of them, then shuffled.
    let mut chars: Zeroizing<Vec<char>> = Zeroizing::new(Vec::with_capacity(length));
    for set in &sets {
        chars.push(set[below(set.len())]);
    }
    while chars.len() < length {
        chars.push(all[below(all.len())]);
    }
    for i in (1..chars.len()).rev() {
        chars.swap(i, below(i + 1));
    }
    Ok(Zeroizing::new(chars.iter().collect()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Strength {
    None,
    Weak,
    Fair,
    Strong,
}

/// The words most leaked passwords are built on. A password that is one of
/// them with digits or symbols around it ("Password123!", "Summer2024")
/// falls within the first guesses, whatever its length.
const COMMON: &[&str] = &[
    "password", "passwort", "passwd", "pass", "secret", "letmein", "welcome", "admin", "administrator", "login",
    "user", "guest", "root", "test", "testing", "default", "changeme", "master", "access", "trustno", "iloveyou",
    "love", "lovely", "loveme", "hello", "freedom", "whatever", "sunshine", "princess", "shadow", "monkey",
    "dragon", "superman", "batman", "spiderman", "pokemon", "starwars", "matrix", "killer", "ninja", "football",
    "baseball", "basketball", "soccer", "hockey", "summer", "winter", "spring", "autumn", "flower", "angel",
    "baby", "family", "friend", "friends", "happy", "lucky", "cookie", "chocolate", "banana", "orange", "apple",
    "google", "samsung", "computer", "internet", "michael", "jennifer", "jordan", "hunter", "ranger", "buster",
    "harley", "charlie", "daniel", "andrew", "thomas", "jessica", "ashley", "nicole", "michelle", "tigger",
    "maria", "george", "mustang", "ferrari", "liverpool", "chelsea", "arsenal", "barcelona", "juventus",
    "greece", "hellas", "athens", "abc", "qwe", "zaq", "xsw",
];

/// Keyboard rows and the alphabet: "qwerty", "asdf", "abcd" and backwards.
const WALKS: &[&str] = &["qwertyuiop", "asdfghjkl", "zxcvbnm", "qwertzuiop", "azertyuiop", "abcdefghijklmnopqrstuvwxyz"];

/// The letter a character stands for in a password: itself, or what it
/// replaces the way people write "P@ssw0rd".
fn letter(c: char) -> Option<char> {
    match c {
        c if c.is_alphabetic() => c.to_lowercase().next(),
        '4' | '@' => Some('a'),
        '3' => Some('e'),
        '1' | '!' => Some('i'),
        '0' => Some('o'),
        '5' | '$' => Some('s'),
        '7' => Some('t'),
        _ => None,
    }
}

/// Bits of guessing for a well-known word or keyboard run with only digits
/// and symbols around it; `None` for any other password.
fn common_word_bits(password: &str) -> Option<f64> {
    let chars: Vec<char> = password.chars().collect();
    let start = chars.iter().position(|c| c.is_alphabetic())?;
    let end = chars.iter().rposition(|c| c.is_alphabetic())? + 1;
    let core = &chars[start..end];
    let word: String = core.iter().map(|&c| letter(c)).collect::<Option<_>>()?;
    let length = word.chars().count();
    let walk = length >= 4 && WALKS.iter().any(|row| row.contains(&word) || row.chars().rev().collect::<String>().contains(&word));
    if !COMMON.contains(&word.as_str()) && !walk {
        return None;
    }
    // A capital first letter is the usual one; capitals elsewhere are not.
    let capitals = core.iter().filter(|c| c.is_uppercase()).count();
    if capitals > 1 || capitals == 1 && !core[0].is_uppercase() {
        return None;
    }
    let around: f64 = chars[..start]
        .iter()
        .chain(&chars[end..])
        .map(|c| if c.is_ascii_digit() { 10f64.log2() } else { 6.0 })
        .sum();
    Some(10.0 + capitals as f64 + around)
}

/// Bits of guessing from the length and the kinds of characters used,
/// halved for passwords made of one repeated or sequential run, and far
/// lower for a well-known word with digits or symbols around it.
pub fn strength(password: &str) -> Strength {
    if password.is_empty() {
        return Strength::None;
    }
    let mut pool = 0u32;
    if password.chars().any(|c| c.is_ascii_lowercase()) {
        pool += 26;
    }
    if password.chars().any(|c| c.is_ascii_uppercase()) {
        pool += 26;
    }
    if password.chars().any(|c| c.is_ascii_digit()) {
        pool += 10;
    }
    if password.chars().any(|c| !c.is_ascii_alphanumeric()) {
        pool += 33;
    }
    let length = password.chars().count() as f64;
    let mut bits = length * f64::from(pool.max(1)).log2();
    let chars: Vec<char> = password.chars().collect();
    let repetitive = chars.windows(2).all(|w| w[0] == w[1])
        || chars
            .windows(2)
            .all(|w| (w[1] as i64 - w[0] as i64).abs() == 1);
    if repetitive {
        bits /= 2.0;
    }
    if let Some(common) = common_word_bits(password) {
        bits = bits.min(common);
    }
    match bits {
        b if b < 45.0 => Strength::Weak,
        b if b < 75.0 => Strength::Fair,
        _ => Strength::Strong,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(length: usize) -> Options {
        Options {
            length,
            ..Options::default()
        }
    }

    fn phrase(words: usize) -> Options {
        Options {
            kind: Kind::Passphrase,
            words,
            ..Options::default()
        }
    }

    #[test]
    fn the_wordlist_has_7775_distinct_plain_words() {
        let list: Vec<&str> = words().collect();
        assert_eq!(list.len(), 7775);
        assert!(list.iter().all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase())));
        let distinct: std::collections::HashSet<&&str> = list.iter().collect();
        assert_eq!(distinct.len(), list.len());
    }

    #[test]
    fn passphrases_have_the_asked_words_separator_and_options() {
        let list: std::collections::HashSet<&str> = words().collect();
        for _ in 0..100 {
            let plain = generate(&phrase(5)).unwrap();
            let parts: Vec<&str> = plain.split('-').collect();
            assert_eq!(parts.len(), 5, "{}", plain.as_str());
            assert!(parts.iter().all(|w| list.contains(w)));
        }
        assert_eq!(generate(&phrase(1)).unwrap().split('-').count(), 3, "at least 3 words");
        assert_eq!(generate(&phrase(40)).unwrap().split('-').count(), 12, "at most 12 words");

        let fancy = Options {
            separator: " . ".into(),
            capitalize: true,
            number: true,
            ..phrase(4)
        };
        for _ in 0..100 {
            let value = generate(&fancy).unwrap();
            // Spaces and letters are dropped from the separator: only "." stays.
            let parts: Vec<&str> = value.split('.').collect();
            assert_eq!(parts.len(), 4, "{}", value.as_str());
            assert!(parts.iter().all(|w| w.chars().next().unwrap().is_ascii_uppercase()));
            assert_eq!(parts.iter().filter(|w| w.ends_with(|c: char| c.is_ascii_digit())).count(), 1);
            assert_ne!(strength(&value), Strength::Weak);
        }
        let joined = Options {
            separator: String::new(),
            ..phrase(3)
        };
        assert!(generate(&joined).unwrap().chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn options_from_older_pages_still_make_passwords() {
        let old: Options = serde_json::from_str(
            r#"{"length":16,"lower":true,"upper":true,"digits":true,"symbols":false,"avoidAmbiguous":true}"#,
        )
        .unwrap();
        assert_eq!(old.kind, Kind::Password);
        assert_eq!(generate(&old).unwrap().chars().count(), 16);
        let new: Options = serde_json::from_str(
            r#"{"kind":"passphrase","length":16,"lower":true,"upper":true,"digits":true,"symbols":false,"words":6,"separator":"_","capitalize":false,"number":false}"#,
        )
        .unwrap();
        assert_eq!(generate(&new).unwrap().split('_').count(), 6);
    }

    #[test]
    fn generated_passwords_have_every_chosen_kind_and_the_asked_length() {
        for _ in 0..200 {
            let password = generate(&options(12)).unwrap();
            assert_eq!(password.chars().count(), 12);
            assert!(password.chars().any(|c| c.is_ascii_lowercase()));
            assert!(password.chars().any(|c| c.is_ascii_uppercase()));
            assert!(password.chars().any(|c| c.is_ascii_digit()));
            assert!(password.chars().any(|c| SYMBOLS.contains(c)));
        }
        let mut plain = options(200);
        plain.symbols = false;
        plain.avoid_ambiguous = true;
        let password = generate(&plain).unwrap();
        assert_eq!(password.chars().count(), 128, "clamped");
        assert!(
            !password
                .chars()
                .any(|c| AMBIGUOUS.contains(c) || SYMBOLS.contains(c))
        );
        let none = Options {
            lower: false,
            upper: false,
            digits: false,
            symbols: false,
            ..options(16)
        };
        assert!(generate(&none).is_err());
    }

    #[test]
    fn strength_reflects_length_variety_and_patterns() {
        assert_eq!(strength(""), Strength::None);
        assert_eq!(strength("password"), Strength::Weak);
        assert_eq!(strength("abcdefghijklmnop"), Strength::Weak);
        assert_eq!(strength("Tr0ub4dor&3"), Strength::Fair);
        assert_eq!(strength(&generate(&options(20)).unwrap()), Strength::Strong);
    }

    #[test]
    fn well_known_words_with_digits_around_them_are_weak() {
        for weak in ["password123", "Password123!", "P@ssw0rd!", "Summer2024!", "qwerty123", "iloveyou2", "1234asdf", "Welcome1"] {
            assert_eq!(strength(weak), Strength::Weak, "{weak}");
        }
        // Words people do not share, or not just one word: the length counts.
        assert_eq!(strength("river-copper-lantern-orbit-1234"), Strength::Strong);
        assert_eq!(strength("correcthorsebatterystaple"), Strength::Strong);
        assert_eq!(strength("pAssword123"), Strength::Fair, "a capital inside is not the usual pattern");
        for _ in 0..200 {
            assert_ne!(strength(&generate(&options(16)).unwrap()), Strength::Weak);
        }
    }
}
