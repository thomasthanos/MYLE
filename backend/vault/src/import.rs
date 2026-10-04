//! Moving passwords in and out: the CSV files Chrome, Edge, Firefox,
//! Bitwarden and 1Password export, Bitwarden's JSON export, and the app's
//! own encrypted backup (`.myle-vault`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use super::crypto::{self, KdfParams};
use super::vault::Entry;

const BACKUP_FORMAT: &str = "myle-vault-backup";
const BACKUP_AAD: &str = "myle-backup";

/// Reads an export from another password manager or a browser. The format
/// is recognised from the content, not the file name.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim_start().starts_with('{') {
        let json: Value = serde_json::from_str(text)
            .map_err(|_| "That JSON file could not be read.".to_string())?;
        if json.get("encrypted").and_then(Value::as_bool) == Some(true) {
            return Err(
                "That Bitwarden export is encrypted. Export it again as unencrypted JSON or CSV."
                    .into(),
            );
        }
        return bitwarden_json(&json);
    }
    csv_entries(text)
}

fn bitwarden_json(json: &Value) -> Result<Vec<Entry>, String> {
    let items = json
        .get("items")
        .and_then(Value::as_array)
        .ok_or("That JSON file is not a Bitwarden export.")?;
    let folders: Vec<(String, String)> = json
        .get("folders")
        .and_then(Value::as_array)
        .map(|folders| {
            folders
                .iter()
                .filter_map(|f| Some((text_of(f, "id")?, text_of(f, "name")?)))
                .collect()
        })
        .unwrap_or_default();
    let mut entries = Vec::new();
    for item in items {
        // Type 1 is a login; cards, identities and notes are left out.
        if item.get("type").and_then(Value::as_u64) != Some(1) {
            continue;
        }
        let login = item.get("login").cloned().unwrap_or(Value::Null);
        let urls = login
            .get("uris")
            .and_then(Value::as_array)
            .map(|uris| uris.iter().filter_map(|u| text_of(u, "uri")).collect())
            .unwrap_or_default();
        let folder = text_of(item, "folderId")
            .and_then(|id| {
                folders
                    .iter()
                    .find(|(f, _)| *f == id)
                    .map(|(_, n)| n.clone())
            })
            .unwrap_or_default();
        entries.push(Entry {
            title: text_of(item, "name").unwrap_or_default(),
            username: text_of(&login, "username").unwrap_or_default(),
            password: text_of(&login, "password").unwrap_or_default(),
            urls,
            notes: text_of(item, "notes").unwrap_or_default(),
            favorite: item
                .get("favorite")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            folder,
            ..Entry::default()
        });
    }
    Ok(finish(entries))
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// The column that holds each field, found by its header in any of the
/// exporters' spellings.
fn column(headers: &[String], names: &[&str]) -> Option<usize> {
    headers
        .iter()
        .position(|h| names.iter().any(|n| h.eq_ignore_ascii_case(n)))
}

fn csv_entries(text: &str) -> Result<Vec<Entry>, String> {
    let mut rows = parse_csv(text).into_iter();
    let headers: Vec<String> = rows
        .next()
        .ok_or("The file is empty.")?
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    let password = column(&headers, &["password", "login_password"])
        .ok_or("No password column was found. Is this a password export?")?;
    let title = column(&headers, &["name", "title"]);
    let username = column(
        &headers,
        &["username", "login_username", "user name", "email"],
    );
    let url = column(&headers, &["url", "login_uri", "website", "web site"]);
    let notes = column(&headers, &["note", "notes", "extra"]);
    let folder = column(&headers, &["folder", "grouping"]);
    let favorite = column(&headers, &["favorite", "fav"]);

    let cell = |row: &[String], index: Option<usize>| {
        index
            .and_then(|i| row.get(i))
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    let mut entries = Vec::new();
    for row in rows {
        if row.iter().all(|v| v.trim().is_empty()) {
            continue;
        }
        let url_value = cell(&row, url);
        let mut title_value = cell(&row, title);
        if title_value.is_empty() {
            // Firefox has no name column: use the site's host.
            title_value = host_of(&url_value).unwrap_or_default();
        }
        entries.push(Entry {
            title: title_value,
            username: cell(&row, username),
            password: cell(&row, Some(password)),
            urls: if url_value.is_empty() {
                Vec::new()
            } else {
                vec![url_value]
            },
            notes: cell(&row, notes),
            folder: cell(&row, folder),
            favorite: matches!(
                cell(&row, favorite).to_lowercase().as_str(),
                "1" | "true" | "yes"
            ),
            ..Entry::default()
        });
    }
    Ok(finish(entries))
}

/// Drops rows with nothing to keep and names the nameless.
fn finish(entries: Vec<Entry>) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|e| !e.password.is_empty() || !e.username.is_empty())
        .map(|mut e| {
            if e.title.trim().is_empty() {
                e.title = e
                    .urls
                    .first()
                    .and_then(|u| host_of(u))
                    .unwrap_or_else(|| "Imported login".into());
            }
            e
        })
        .collect()
}

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?.trim_start_matches("www.");
    (!host.is_empty()).then(|| host.to_lowercase())
}

/// RFC 4180: quoted fields with doubled quotes and line breaks inside.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Backup {
    format: String,
    version: u32,
    kdf: KdfParams,
    data: String,
}

/// The app's own backup: every entry, sealed under a password of its own.
pub fn backup(entries: &[Entry], password: &str, kdf: KdfParams) -> Result<Vec<u8>, String> {
    let key = crypto::derive(password, &kdf)?;
    let plain = Zeroizing::new(serde_json::to_vec(entries).map_err(|e| e.to_string())?);
    let backup = Backup {
        format: BACKUP_FORMAT.into(),
        version: 1,
        kdf,
        data: crypto::seal(&key, BACKUP_AAD, &plain)?,
    };
    serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())
}

pub fn is_backup(text: &str) -> bool {
    serde_json::from_str::<Backup>(text).is_ok_and(|b| b.format == BACKUP_FORMAT)
}

pub fn restore(text: &str, password: &str) -> Result<Vec<Entry>, String> {
    let backup: Backup = serde_json::from_str(text)
        .map_err(|_| "That is not a MYLE backup.".to_string())?;
    let key = crypto::derive(password, &backup.kdf)?;
    let plain = crypto::open(&key, BACKUP_AAD, &backup.data)
        .map_err(|_| "Wrong backup password, or the file was changed.".to_string())?;
    serde_json::from_slice(&plain).map_err(|_| "The backup is damaged.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_and_edge_csv() {
        let text = "name,url,username,password,note\r\n\
                    GitHub,https://github.com/login,me,\"p,ass\"\"word\",\"line one\nline two\"\r\n";
        let entries = parse(text).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "GitHub");
        assert_eq!(entries[0].password, "p,ass\"word");
        assert_eq!(entries[0].notes, "line one\nline two");
        assert_eq!(entries[0].urls, vec!["https://github.com/login"]);
    }

    #[test]
    fn firefox_csv_takes_its_title_from_the_site() {
        let text = "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\"\n\
                    \"https://www.reddit.com\",\"me\",\"secret\",,\"https://www.reddit.com\",\"{1}\"\n";
        let entries = parse(text).unwrap();
        assert_eq!(entries[0].title, "reddit.com");
        assert_eq!(entries[0].username, "me");
    }

    #[test]
    fn bitwarden_and_1password_csv() {
        let bitwarden = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
                         Games,1,login,Steam,,,,https://store.steampowered.com,me,pw,\n";
        let entry = &parse(bitwarden).unwrap()[0];
        assert_eq!(
            (entry.title.as_str(), entry.folder.as_str(), entry.favorite),
            ("Steam", "Games", true)
        );
        let one_password =
            "Title,Website,Username,Password,Notes\nDiscord,https://discord.com,me,pw,hi\n";
        assert_eq!(
            parse(one_password).unwrap()[0].urls,
            vec!["https://discord.com"]
        );
    }

    #[test]
    fn bitwarden_json_keeps_logins_and_their_folders() {
        let text = r#"{"encrypted":false,"folders":[{"id":"f1","name":"Work"}],
            "items":[{"type":1,"name":"Mail","folderId":"f1","favorite":true,
                      "login":{"username":"me","password":"pw","uris":[{"uri":"https://mail.example"}]}},
                     {"type":3,"name":"Card"}]}"#;
        let entries = parse(text).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].folder.as_str(), entries[0].favorite),
            ("Work", true)
        );
        assert!(parse(r#"{"encrypted":true,"data":"x"}"#).is_err());
    }

    #[test]
    fn a_backup_opens_only_with_its_password() {
        let entries = vec![Entry {
            title: "A".into(),
            password: "pw".into(),
            ..Entry::default()
        }];
        let bytes = backup(&entries, "backup pass", KdfParams::cheap_for_tests()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(is_backup(&text) && !text.contains("\"pw\""));
        assert_eq!(restore(&text, "backup pass").unwrap()[0].title, "A");
        assert!(restore(&text, "wrong").is_err());
    }

    #[test]
    fn hosts_are_read_from_any_address() {
        assert_eq!(
            host_of("https://user@www.Example.com:8443/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of("example.org/login").as_deref(), Some("example.org"));
        assert_eq!(host_of(""), None);
    }
}
