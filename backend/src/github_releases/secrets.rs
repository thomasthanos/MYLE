//! The GitHub token and the AI keys, each sealed with DPAPI in its own file
//! under MYLE's local data (`github-releases\*.bin`): only this Windows user
//! on this PC can open them. They are never sent to the page, written to a
//! settings file, logged, or put on a command line.

use std::path::PathBuf;

use zeroize::Zeroizing;

use super::ai::ProviderId;
use crate::account::vault;

const DIR: &str = "github-releases";
const TOKEN_FILE: &str = "github-token.bin";

fn dir() -> Result<PathBuf, String> {
    Ok(crate::storage::local_dir()?.join(DIR))
}

fn read(name: &str) -> Option<Zeroizing<String>> {
    let path = dir().ok()?.join(name);
    let bytes = Zeroizing::new(vault::load(&path)?);
    let text = std::str::from_utf8(&bytes).ok()?.trim().to_string();
    (!text.is_empty()).then(|| Zeroizing::new(text))
}

fn write(name: &str, secret: &str) -> Result<(), String> {
    vault::save(&dir()?.join(name), secret.trim().as_bytes())
}

fn remove(name: &str) {
    if let Ok(dir) = dir() {
        vault::remove(&dir.join(name));
    }
}

fn key_file(provider: ProviderId) -> String {
    format!("ai-{}.bin", provider.as_str())
}

pub(crate) fn token() -> Option<Zeroizing<String>> {
    read(TOKEN_FILE)
}

pub(crate) fn save_token(token: &str) -> Result<(), String> {
    write(TOKEN_FILE, token)
}

pub(crate) fn remove_token() {
    remove(TOKEN_FILE);
}

pub(crate) fn ai_key(provider: ProviderId) -> Option<Zeroizing<String>> {
    read(&key_file(provider))
}

pub(crate) fn save_ai_key(provider: ProviderId, key: &str) -> Result<(), String> {
    write(&key_file(provider), key)
}

pub(crate) fn remove_ai_key(provider: ProviderId) {
    remove(&key_file(provider));
}

/// The end of a key, for "…a1b2" next to the provider.
pub(crate) fn hint(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < 12 {
        return "…".into();
    }
    format!("…{}", chars[chars.len() - 4..].iter().collect::<String>())
}

/// A pasted key or token: one line of printable ASCII, of a sane length.
pub(crate) fn check_secret(secret: &str, what: &str) -> Result<String, String> {
    let secret = secret.trim();
    if secret.len() < 8 || secret.len() > 512 {
        return Err(format!("That doesn't look like a {what}."));
    }
    if !secret.chars().all(|c| c.is_ascii_graphic()) {
        return Err(format!(
            "That doesn't look like a {what}: it has spaces or other characters a key never has."
        ));
    }
    Ok(secret.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_show_only_the_end_and_pasted_keys_are_checked() {
        assert_eq!(hint("gsk_abcdefghijklmnop1234"), "…1234");
        assert_eq!(hint("short"), "…");
        assert_eq!(
            check_secret("  gsk_abcdefgh123  ", "key").unwrap(),
            "gsk_abcdefgh123"
        );
        assert!(check_secret("gsk abc defgh", "key").is_err());
        assert!(check_secret("abc", "key").is_err());
    }
}
