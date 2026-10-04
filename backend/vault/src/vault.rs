//! The vault: its file on disk, its entries, and locking.
//!
//! The file holds only what the sync may see: the Argon2id settings, the
//! vault key wrapped twice (master password, recovery code) and each entry
//! sealed on its own. Titles, addresses and user names are sealed too.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::crypto::{self, KdfParams, Key};

const FORMAT_VERSION: u32 = 1;
const MASTER_AAD: &str = "myle-vault|master";
const RECOVERY_AAD: &str = "myle-vault|recovery";
/// Old passwords kept per entry.
const HISTORY: usize = 10;

fn entry_aad(id: &str, revision: u64) -> String {
    format!("myle-entry|{id}|{revision}")
}

/// A deletion is sealed too, so only someone with the vault key can delete
/// an entry: the account (or anyone who reaches it) cannot.
const TOMBSTONE: &[u8] = b"deleted";

fn tombstone_aad(id: &str, revision: u64) -> String {
    format!("myle-tombstone|{id}|{revision}")
}

/// Wrong master passwords in a row before each try has to wait, and the
/// longest wait.
const FREE_TRIES: u32 = 3;
const LONGEST_WAIT: Duration = Duration::from_secs(60);

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Settings that are not secret, kept with the vault.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// Minutes without use before the vault locks itself; 0 never.
    pub auto_lock_minutes: u32,
    /// The browser extension may fill in and save logins (on by default).
    #[serde(default = "shown")]
    pub browser_filling: bool,
    /// Browser filling was turned on once for a vault made before it was on
    /// by default; from then on the user's choice stands.
    #[serde(default)]
    pub filling_default_applied: bool,
    /// The list shows each website's icon, fetched from the website.
    #[serde(default = "shown")]
    pub website_icons: bool,
}

fn shown() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            auto_lock_minutes: 5,
            browser_filling: true,
            filling_default_applied: true,
            website_icons: true,
        }
    }
}

/// One sealed entry. A deleted entry keeps its id and revision (with no
/// ciphertext), so the deletion reaches other PCs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub revision: u64,
    #[serde(default)]
    pub deleted: bool,
    pub updated_at: u64,
    #[serde(default)]
    pub ciphertext: String,
    /// Changed here since the last sync.
    #[serde(default)]
    pub dirty: bool,
    /// The revision the account had when this PC last synced this entry;
    /// `None` when it has never been synced. A push only replaces that one.
    #[serde(default)]
    pub base_revision: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFile {
    pub version: u32,
    /// Tells vaults apart: two PCs sync only the same vault.
    #[serde(default)]
    pub vault_id: String,
    pub kdf: KdfParams,
    pub wrapped_key: String,
    pub recovery_wrapped_key: String,
    #[serde(default)]
    pub records: Vec<Record>,
    #[serde(default)]
    pub prefs: Prefs,
    /// The master password changed here since the last sync.
    #[serde(default)]
    pub header_dirty: bool,
}

/// A Windows program an entry belongs to, for filling in from that program.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Zeroize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppLink {
    /// Full executable path, or a legacy file-name-only link.
    pub exe: String,
    /// What to call it (`Riot Client`).
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Zeroize)]
#[serde(rename_all = "camelCase")]
pub struct OldPassword {
    pub password: String,
    pub changed_at: u64,
}

/// Kept in memory as `Zeroizing<Entry>`, which wipes it when dropped.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Zeroize)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub title: String,
    pub username: String,
    pub password: String,
    pub urls: Vec<String>,
    pub apps: Vec<AppLink>,
    pub notes: String,
    pub favorite: bool,
    pub folder: String,
    pub history: Vec<OldPassword>,
    /// The 2FA key as an `otpauth://totp/…` link; empty without one.
    pub totp: String,
    /// The site's passkeys for this login.
    pub passkeys: Vec<super::passkeys::Passkey>,
    pub created_at: u64,
    pub updated_at: u64,
    /// What a newer MYLE keeps in an entry: saved back as it was, so an
    /// edit here never drops it.
    #[serde(flatten)]
    #[zeroize(skip)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// What the page lists: everything but the password and its history.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub username: String,
    pub urls: Vec<String>,
    pub apps: Vec<AppLink>,
    pub notes: String,
    pub favorite: bool,
    pub folder: String,
    pub updated_at: u64,
    pub has_password: bool,
    pub strength: super::generator::Strength,
    /// Other entries use the same password.
    pub reused: bool,
    pub history_count: usize,
    /// It has a 2FA key: the page can ask for its codes.
    pub has_totp: bool,
    /// Its passkeys (never their keys).
    pub passkeys: Vec<super::passkeys::PasskeyInfo>,
}

/// A new or changed entry from the page. `password: None` keeps it.
#[derive(Debug, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub apps: Vec<AppLink>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub folder: String,
    /// The 2FA key or its `otpauth://` link: `None` keeps the saved one,
    /// empty removes it.
    #[serde(default)]
    pub totp: Option<String>,
}

struct Unlocked {
    key: Key,
    entries: BTreeMap<String, Zeroizing<Entry>>,
    /// Entries in the file that did not open with the key (damaged, or not
    /// made with it). They stay in the file untouched and are not shown.
    damaged: usize,
    /// The websites' icons, read from disk the first time they are asked for.
    icons: Option<IconCache>,
}

/// The websites' icons for the list (`icons.rs` fetches them). Which sites
/// the vault holds is private, so the cache is kept only while the vault is
/// open, and on disk sealed with the vault key, next to the vault.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct IconCache {
    pub icons: BTreeMap<String, CachedIcon>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CachedIcon {
    /// A `data:` URL, or none when the website had no usable icon.
    pub data: Option<String>,
    /// When it was fetched (seconds since 1970).
    pub at: u64,
}

const ICONS_AAD: &str = "myle-vault|icons";

pub struct Vault {
    path: PathBuf,
    file: Option<VaultFile>,
    unlocked: Option<Unlocked>,
    /// The last time the user did something with the vault (not the browser
    /// or the sync), for locking it after a while.
    pub last_used: Instant,
    failed_tries: u32,
    retry_at: Option<Instant>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// No vault on this PC yet.
    New,
    Locked,
    Unlocked,
}

impl Vault {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let file = match std::fs::read(&path) {
            Ok(bytes) => Some(
                serde_json::from_slice::<VaultFile>(&bytes)
                    .map_err(|_| "The password vault file is damaged.".to_string())?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        if file.as_ref().is_some_and(|f| f.version > FORMAT_VERSION) {
            return Err("This vault was made by a newer version of the app.".into());
        }
        let mut vault = Self {
            path,
            file,
            unlocked: None,
            last_used: Instant::now(),
            failed_tries: 0,
            retry_at: None,
        };
        if let Some(file) = vault.file.as_mut()
            && file.vault_id.is_empty()
        {
            file.vault_id = uuid::Uuid::new_v4().to_string();
            vault.write()?;
        }
        Ok(vault)
    }

    pub fn status(&self) -> Status {
        match (&self.file, &self.unlocked) {
            (None, _) => Status::New,
            (Some(_), None) => Status::Locked,
            (Some(_), Some(_)) => Status::Unlocked,
        }
    }

    pub fn prefs(&self) -> Prefs {
        self.file
            .as_ref()
            .map(|f| f.prefs.clone())
            .unwrap_or_default()
    }

    pub fn set_prefs(&mut self, prefs: Prefs) -> Result<(), String> {
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        file.prefs = prefs;
        self.write()
    }

    /// Browser filling is on by default: a vault made before that has it
    /// turned on once. True when it was turned on now.
    pub fn apply_filling_default(&mut self) -> Result<bool, String> {
        let Some(file) = self.file.as_mut() else {
            return Ok(false);
        };
        if file.prefs.filling_default_applied {
            return Ok(false);
        }
        file.prefs.filling_default_applied = true;
        file.prefs.browser_filling = true;
        self.write()?;
        Ok(true)
    }

    /// A new, empty vault under `master`. Returns the recovery code, which
    /// is shown once and never stored.
    pub fn create(&mut self, master: &str, kdf: KdfParams) -> Result<Zeroizing<String>, String> {
        if self.file.is_some() {
            return Err("There is already a vault on this device.".into());
        }
        let vault_key = Key::random();
        let master_key = crypto::derive(master, &kdf)?;
        let (code, recovery_key) = crypto::new_recovery_code();
        self.file = Some(VaultFile {
            version: FORMAT_VERSION,
            vault_id: uuid::Uuid::new_v4().to_string(),
            kdf,
            wrapped_key: crypto::wrap(&master_key, MASTER_AAD, &vault_key)?,
            recovery_wrapped_key: crypto::wrap(&recovery_key, RECOVERY_AAD, &vault_key)?,
            records: Vec::new(),
            prefs: Prefs::default(),
            header_dirty: true,
        });
        self.write()?;
        self.unlocked = Some(Unlocked {
            key: vault_key,
            entries: BTreeMap::new(),
            damaged: 0,
            icons: None,
        });
        Ok(code)
    }

    pub fn unlock(&mut self, master: &str) -> Result<(), String> {
        let key = self.master_key_check(master, "Wrong master password.")?;
        self.finish_unlock(key)
    }

    /// The vault key for `master`, counting wrong tries: after a few, each
    /// try has to wait longer (up to a minute), so guessing through the app
    /// is slow even for a short password.
    fn master_key_check(&mut self, master: &str, wrong: &str) -> Result<Key, String> {
        if let Some(at) = self.retry_at
            && let Some(left) = at.checked_duration_since(Instant::now())
        {
            return Err(format!(
                "Too many wrong tries. Try again in {} seconds.",
                left.as_secs() + 1
            ));
        }
        let file = self.file.as_ref().ok_or("There is no vault yet.")?;
        let master_key = crypto::derive(master, &file.kdf)?;
        match crypto::unwrap(&master_key, MASTER_AAD, &file.wrapped_key) {
            Ok(key) => {
                self.failed_tries = 0;
                self.retry_at = None;
                Ok(key)
            }
            Err(_) => {
                self.failed_tries += 1;
                if self.failed_tries >= FREE_TRIES {
                    let wait = Duration::from_secs(1 << (self.failed_tries - FREE_TRIES).min(6));
                    self.retry_at = Some(Instant::now() + wait.min(LONGEST_WAIT));
                }
                Err(wrong.to_string())
            }
        }
    }

    /// Asks for the master password again before something sensitive (an
    /// export of every password), even while the vault is open.
    pub fn verify_master(&mut self, master: &str) -> Result<(), String> {
        self.master_key_check(master, "Wrong master password.")
            .map(|_| ())
    }

    /// For a forgotten master password: the recovery code opens the vault
    /// and `new_master` replaces the old one.
    pub fn recover(&mut self, code: &str, new_master: &str, kdf: KdfParams) -> Result<(), String> {
        if self.retry_at.is_some_and(|at| at > Instant::now()) {
            return Err("Too many wrong tries. Wait a moment and try again.".into());
        }
        let file = self.file.as_ref().ok_or("There is no vault yet.")?;
        let recovery_key = crypto::parse_recovery_code(code)?;
        let key = match crypto::unwrap(&recovery_key, RECOVERY_AAD, &file.recovery_wrapped_key) {
            Ok(key) => key,
            Err(_) => {
                self.failed_tries += 1;
                if self.failed_tries >= FREE_TRIES {
                    self.retry_at = Some(Instant::now() + LONGEST_WAIT.min(Duration::from_secs(10)));
                }
                return Err("That recovery code does not open this vault.".into());
            }
        };
        self.failed_tries = 0;
        self.retry_at = None;
        self.finish_unlock(key)?;
        self.rewrap(new_master, kdf)
    }

    fn finish_unlock(&mut self, key: Key) -> Result<(), String> {
        let file = self.file.as_ref().ok_or("There is no vault yet.")?;
        let mut entries = BTreeMap::new();
        let mut damaged = 0;
        // One entry that does not open (damaged, or slipped in by someone
        // without the key) must not keep the rest of the vault shut.
        for record in file.records.iter().filter(|r| !r.deleted) {
            match open_entry(&key, &record.id, record.revision, &record.ciphertext) {
                Some(entry) => {
                    entries.insert(record.id.clone(), Zeroizing::new(entry));
                }
                None => damaged += 1,
            }
        }
        self.unlocked = Some(Unlocked {
            key,
            entries,
            damaged,
            icons: None,
        });
        self.last_used = Instant::now();
        Ok(())
    }

    /// Entries in the file that did not open with the key.
    pub fn damaged(&self) -> usize {
        self.unlocked.as_ref().map_or(0, |u| u.damaged)
    }

    /// The user did something with the vault: it stays open longer.
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    /// Which vault this is, once there is one.
    pub fn vault_id(&self) -> Option<String> {
        self.file.as_ref().map(|file| file.vault_id.clone())
    }

    /// A copy of the open vault's key, to wrap for Windows Hello.
    pub fn key_copy(&mut self) -> Result<Key, String> {
        Ok(self.unlocked()?.key.clone())
    }

    /// Opens the vault with its key (from Windows Hello).
    pub fn unlock_with_key(&mut self, key: Key) -> Result<(), String> {
        if self.file.is_none() {
            return Err("There is no vault yet.".into());
        }
        self.finish_unlock(key)
    }

    /// Forgets the key and every decrypted entry (wiped as they drop).
    pub fn lock(&mut self) {
        self.unlocked = None;
    }

    /// `passwords-icons.bin` next to `passwords.vault`.
    fn icons_path(&self) -> PathBuf {
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        self.path.with_file_name(format!("{stem}-icons.bin"))
    }

    /// The icon cache, read (and opened with the vault key) on first use. A
    /// cache that does not open, from another vault say, starts over empty.
    pub fn icons(&mut self) -> Result<&mut IconCache, String> {
        let path = self.icons_path();
        let unlocked = self.unlocked.as_mut().ok_or("The vault is locked.")?;
        if unlocked.icons.is_none() {
            let cache = std::fs::read_to_string(&path)
                .ok()
                .and_then(|sealed| crypto::open(&unlocked.key, ICONS_AAD, sealed.trim()).ok())
                .and_then(|plain| serde_json::from_slice(&plain).ok())
                .unwrap_or_default();
            unlocked.icons = Some(cache);
        }
        Ok(unlocked.icons.as_mut().expect("read above"))
    }

    pub fn save_icons(&self) -> Result<(), String> {
        let unlocked = self.unlocked.as_ref().ok_or("The vault is locked.")?;
        let Some(cache) = &unlocked.icons else { return Ok(()) };
        let plain = Zeroizing::new(serde_json::to_vec(cache).map_err(|e| e.to_string())?);
        let sealed = crypto::seal(&unlocked.key, ICONS_AAD, &plain)?;
        write_atomic(&self.icons_path(), sealed.as_bytes())
    }

    /// Deletes the icon cache (website icons were turned off).
    pub fn forget_icons(&mut self) {
        if let Some(unlocked) = self.unlocked.as_mut() {
            unlocked.icons = None;
        }
        let _ = std::fs::remove_file(self.icons_path());
    }

    pub fn change_master(
        &mut self,
        current: &str,
        new_master: &str,
        kdf: KdfParams,
    ) -> Result<(), String> {
        self.master_key_check(current, "The current master password is wrong.")?;
        self.rewrap(new_master, kdf)
    }

    /// The vault key under a new master password; the entries stay as they are.
    fn rewrap(&mut self, new_master: &str, kdf: KdfParams) -> Result<(), String> {
        let key = &self.unlocked.as_ref().ok_or("The vault is locked.")?.key;
        let master_key = crypto::derive(new_master, &kdf)?;
        let wrapped = crypto::wrap(&master_key, MASTER_AAD, key)?;
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        file.kdf = kdf;
        file.wrapped_key = wrapped;
        file.header_dirty = true;
        self.write()
    }

    fn unlocked(&mut self) -> Result<&mut Unlocked, String> {
        self.unlocked
            .as_mut()
            .ok_or_else(|| "The vault is locked.".to_string())
    }

    pub fn summaries(&mut self) -> Result<Vec<Summary>, String> {
        let unlocked = self.unlocked()?;
        let mut uses: HashMap<&str, usize> = HashMap::new();
        for entry in unlocked.entries.values().filter(|e| !e.password.is_empty()) {
            *uses.entry(entry.password.as_str()).or_default() += 1;
        }
        Ok(unlocked
            .entries
            .iter()
            .map(|(id, e)| Summary {
                id: id.clone(),
                title: e.title.clone(),
                username: e.username.clone(),
                urls: e.urls.clone(),
                apps: e.apps.clone(),
                notes: e.notes.clone(),
                favorite: e.favorite,
                folder: e.folder.clone(),
                updated_at: e.updated_at,
                has_password: !e.password.is_empty(),
                strength: super::generator::strength(&e.password),
                reused: uses.get(e.password.as_str()).is_some_and(|n| *n > 1),
                history_count: e.history.len(),
                has_totp: !e.totp.is_empty(),
                passkeys: e.passkeys.iter().map(super::passkeys::Passkey::info).collect(),
            })
            .collect())
    }

    pub fn password(&mut self, id: &str) -> Result<Zeroizing<String>, String> {
        let unlocked = self.unlocked()?;
        let entry = unlocked
            .entries
            .get(id)
            .ok_or("That entry no longer exists.")?;
        Ok(Zeroizing::new(entry.password.clone()))
    }

    pub fn username(&mut self, id: &str) -> Result<String, String> {
        let unlocked = self.unlocked()?;
        let entry = unlocked
            .entries
            .get(id)
            .ok_or("That entry no longer exists.")?;
        Ok(entry.username.clone())
    }

    /// The passkeys for the relying party `rp_id`, only those in `allow`
    /// (credential ids) when it names any: (entry id, entry title, passkey).
    pub fn passkeys_for(
        &mut self,
        rp_id: &str,
        allow: &[String],
    ) -> Result<Vec<(String, String, super::passkeys::Passkey)>, String> {
        let unlocked = self.unlocked()?;
        let mut found: Vec<_> = unlocked
            .entries
            .iter()
            .flat_map(|(id, entry)| {
                entry
                    .passkeys
                    .iter()
                    .filter(|key| key.rp_id == rp_id && (allow.is_empty() || allow.contains(&key.credential_id)))
                    .map(|key| (id.clone(), entry.title.clone(), key.clone()))
            })
            .collect();
        found.sort_by(|a, b| a.2.user_name.cmp(&b.2.user_name));
        Ok(found)
    }

    /// Keeps a new passkey in the login `target`, or in a new login named
    /// `title` for `url`. A passkey of the same account on the same site goes
    /// (a site that makes a new one replaces the old). Returns the entry's id.
    pub fn add_passkey(
        &mut self,
        target: Option<&str>,
        passkey: super::passkeys::Passkey,
        title: &str,
        url: &str,
    ) -> Result<String, String> {
        let now = now();
        let unlocked = self.unlocked()?;
        let replaced: Vec<(String, Entry)> = unlocked
            .entries
            .iter()
            .filter(|(id, entry)| {
                Some(id.as_str()) != target
                    && entry.passkeys.iter().any(|k| k.rp_id == passkey.rp_id && k.user_handle == passkey.user_handle)
            })
            .map(|(id, entry)| {
                let mut entry = (**entry).clone();
                entry.passkeys.retain(|k| !(k.rp_id == passkey.rp_id && k.user_handle == passkey.user_handle));
                entry.updated_at = now;
                (id.clone(), entry)
            })
            .collect();
        let existing = target.and_then(|id| unlocked.entries.get(id).map(|e| (id.to_string(), (**e).clone())));
        let (id, mut entry) = existing.unwrap_or_else(|| {
            (
                uuid::Uuid::new_v4().to_string(),
                Entry {
                    title: title.trim().to_string(),
                    username: passkey.user_name.clone(),
                    urls: vec![url.to_string()],
                    created_at: now,
                    ..Entry::default()
                },
            )
        });
        entry.passkeys.retain(|k| !(k.rp_id == passkey.rp_id && k.user_handle == passkey.user_handle));
        entry.passkeys.push(passkey);
        entry.updated_at = now;
        for (other, changed) in replaced {
            self.put_unwritten(&other, changed)?;
        }
        self.put(&id, entry)?;
        Ok(id)
    }

    pub fn delete_passkey(&mut self, id: &str, credential_id: &str) -> Result<(), String> {
        let unlocked = self.unlocked()?;
        let mut entry = unlocked
            .entries
            .get(id)
            .map(|e| (**e).clone())
            .ok_or("That entry no longer exists.")?;
        let before = entry.passkeys.len();
        entry.passkeys.retain(|k| k.credential_id != credential_id);
        if entry.passkeys.len() == before {
            return Err("That passkey is no longer in the vault.".into());
        }
        entry.updated_at = now();
        self.put(id, entry)
    }

    /// Gives login `id` the 2FA key `key`, in place of any it has.
    pub fn set_totp(&mut self, id: &str, key: &super::totp::Totp) -> Result<(), String> {
        let unlocked = self.unlocked()?;
        let mut entry = unlocked
            .entries
            .get(id)
            .map(|e| (**e).clone())
            .ok_or("That entry no longer exists.")?;
        let mut key = key.clone();
        key.name_if_unnamed(&entry.title, &entry.username);
        entry.totp = key.to_link().to_string();
        entry.updated_at = now();
        self.put(id, entry)
    }

    /// A new login that holds only a site's 2FA key.
    pub fn add_totp_login(&mut self, title: &str, url: &str, key: &super::totp::Totp) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now();
        let mut key = key.clone();
        key.name_if_unnamed(title, "");
        let entry = Entry {
            title: title.to_string(),
            username: key.account.clone(),
            urls: vec![url.to_string()],
            totp: key.to_link().to_string(),
            created_at: now,
            updated_at: now,
            ..Entry::default()
        };
        self.unlocked()?;
        self.put(&id, entry)?;
        Ok(id)
    }

    /// Links login `id` to the program at `exe` (a full path), in place of
    /// links to the same file name (by name only, or another copy).
    pub fn link_program(&mut self, id: &str, exe: &str, name: &str) -> Result<(), String> {
        let unlocked = self.unlocked()?;
        let mut entry = unlocked
            .entries
            .get(id)
            .map(|e| (**e).clone())
            .ok_or("That entry no longer exists.")?;
        let file = |path: &str| path.rsplit(['\\', '/']).next().unwrap_or(path).trim_matches('"').to_lowercase();
        let wanted = file(exe);
        entry.apps.retain(|app| file(&app.exe) != wanted);
        entry.apps.push(AppLink { exe: exe.to_string(), name: name.to_string() });
        entry.updated_at = now();
        self.put(id, entry)
    }

    /// The entry's 2FA key.
    pub fn totp(&mut self, id: &str) -> Result<super::totp::Totp, String> {
        let unlocked = self.unlocked()?;
        let entry = unlocked
            .entries
            .get(id)
            .ok_or("That entry no longer exists.")?;
        if entry.totp.is_empty() {
            return Err("This login has no 2FA key.".into());
        }
        super::totp::Totp::parse(&entry.totp)
    }

    pub fn history(&mut self, id: &str) -> Result<Vec<OldPassword>, String> {
        let unlocked = self.unlocked()?;
        let entry = unlocked
            .entries
            .get(id)
            .ok_or("That entry no longer exists.")?;
        Ok(entry.history.clone())
    }

    /// Adds or changes an entry and returns its id.
    pub fn save(&mut self, input: &EntryInput) -> Result<String, String> {
        let title = input.title.trim();
        if title.is_empty() {
            return Err("Give the entry a name.".into());
        }
        let now = now();
        let unlocked = self.unlocked()?;
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut entry = unlocked
            .entries
            .get(&id)
            .map(|e| (**e).clone())
            .unwrap_or_else(|| Entry {
                created_at: now,
                ..Entry::default()
            });
        if let Some(password) = &input.password
            && *password != entry.password
        {
            if !entry.password.is_empty() {
                entry.history.insert(
                    0,
                    OldPassword {
                        password: std::mem::take(&mut entry.password),
                        changed_at: now,
                    },
                );
                entry.history.truncate(HISTORY);
            }
            entry.password = password.clone();
        }
        entry.title = title.to_string();
        entry.username = input.username.trim().to_string();
        if let Some(totp) = &input.totp {
            entry.totp = if totp.trim().is_empty() {
                String::new()
            } else {
                let mut key = super::totp::Totp::parse(totp)?;
                key.name_if_unnamed(&entry.title, &entry.username);
                key.to_link().to_string()
            };
        }
        entry.urls = clean_list(&input.urls);
        entry.apps = input.apps.clone();
        entry.notes = input.notes.clone();
        entry.favorite = input.favorite;
        entry.folder = input.folder.trim().to_string();
        entry.updated_at = now;
        self.put(&id, entry)?;
        Ok(id)
    }

    /// Seals `entry` as the next revision of `id` and writes the vault.
    fn put(&mut self, id: &str, entry: Entry) -> Result<(), String> {
        self.put_unwritten(id, entry)?;
        self.write()
    }

    /// The same, leaving the file to be written once after a batch.
    fn put_unwritten(&mut self, id: &str, entry: Entry) -> Result<(), String> {
        let unlocked = self.unlocked.as_mut().ok_or("The vault is locked.")?;
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        let existing = file.records.iter().find(|r| r.id == id);
        let revision = existing.map_or(1, |r| r.revision + 1);
        let base_revision = existing.and_then(|r| r.base_revision);
        let plain = Zeroizing::new(serde_json::to_vec(&entry).map_err(|e| e.to_string())?);
        let ciphertext = crypto::seal(&unlocked.key, &entry_aad(id, revision), &plain)?;
        let record = Record {
            id: id.to_string(),
            revision,
            deleted: false,
            updated_at: entry.updated_at,
            ciphertext,
            dirty: true,
            base_revision,
        };
        match file.records.iter_mut().find(|r| r.id == id) {
            Some(existing) => *existing = record,
            None => file.records.push(record),
        }
        unlocked
            .entries
            .insert(id.to_string(), Zeroizing::new(entry));
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let unlocked = self.unlocked.as_mut().ok_or("The vault is locked.")?;
        unlocked.entries.remove(id);
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        if let Some(record) = file.records.iter_mut().find(|r| r.id == id) {
            record.revision += 1;
            record.deleted = true;
            // Sealed: proof for the other PCs that the deletion is real.
            record.ciphertext =
                crypto::seal(&unlocked.key, &tombstone_aad(id, record.revision), TOMBSTONE)?;
            record.updated_at = now();
            record.dirty = true;
        }
        self.write()
    }

    /// Adds entries (an import). Returns how many.
    /// Adds imported entries and returns how many. One the vault already has
    /// (the same name, user name, password and sites) is left out, so a file
    /// imported twice, or a backup put back into its own vault, makes no
    /// copies.
    pub fn add_all(&mut self, entries: Vec<Entry>) -> Result<usize, String> {
        let unlocked = self.unlocked()?;
        let same = |a: &Entry, b: &Entry| {
            a.title.trim() == b.title.trim()
                && a.username.trim().eq_ignore_ascii_case(b.username.trim())
                && a.password == b.password
                && a.urls == b.urls
        };
        let mut new: Vec<Entry> = Vec::new();
        for entry in entries {
            if !unlocked.entries.values().any(|had| same(had, &entry)) && !new.iter().any(|had| same(had, &entry)) {
                new.push(entry);
            }
        }
        let count = new.len();
        if count == 0 {
            return Ok(0);
        }
        for mut entry in new {
            let now = now();
            entry.created_at = if entry.created_at == 0 {
                now
            } else {
                entry.created_at
            };
            entry.updated_at = now;
            self.put_unwritten(&uuid::Uuid::new_v4().to_string(), entry)?;
        }
        // One write for the whole import, not one per entry.
        self.write()?;
        Ok(count)
    }

    /// Every entry, decrypted, for an export.
    pub fn all_entries(&mut self) -> Result<Vec<Entry>, String> {
        Ok(self
            .unlocked()?
            .entries
            .values()
            .map(|e| (**e).clone())
            .collect())
    }

    /// Entries whose linked programs include `exe`, or whose name matches
    /// it; linked ones first. For filling in from Windows programs, which
    /// comes in a later step.
    #[allow(dead_code)]
    pub fn for_app(&mut self, exe: &str, name: &str) -> Result<Vec<(String, bool)>, String> {
        let unlocked = self.unlocked()?;
        let exe = exe.to_lowercase();
        let name = name.to_lowercase();
        let mut linked = Vec::new();
        let mut guessed = Vec::new();
        for (id, entry) in &unlocked.entries {
            if entry.apps.iter().any(|app| app.exe == exe) {
                linked.push((id.clone(), true));
            } else if !name.is_empty() && matches_name(&entry.title.to_lowercase(), &name) {
                guessed.push((id.clone(), false));
            }
        }
        linked.extend(guessed);
        Ok(linked)
    }

    // -------------------------------------------------------------------
    // Sync (see sync.rs): the account holds the same header and records.

    /// This vault's header as the account stores it, and whether it changed
    /// here since the last sync. `None` before there is a vault.
    pub fn sync_header(&self) -> Option<(RemoteHeader, bool)> {
        let file = self.file.as_ref()?;
        Some((
            RemoteHeader {
                vault_id: file.vault_id.clone(),
                kdf: file.kdf.clone(),
                wrapped_key: file.wrapped_key.clone(),
                recovery_wrapped_key: file.recovery_wrapped_key.clone(),
            },
            file.header_dirty,
        ))
    }

    pub fn header_synced(&mut self) -> Result<(), String> {
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        file.header_dirty = false;
        self.write()
    }

    /// The master password was changed on another PC: its wrapped key
    /// replaces this one. The vault key itself is the same, so an unlocked
    /// vault stays unlocked.
    pub fn take_header(&mut self, header: RemoteHeader) -> Result<(), String> {
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        file.kdf = header.kdf;
        file.wrapped_key = header.wrapped_key;
        file.recovery_wrapped_key = header.recovery_wrapped_key;
        file.header_dirty = false;
        self.write()
    }

    /// Entries that are not deleted.
    pub fn live_count(&self) -> usize {
        self.file
            .as_ref()
            .map_or(0, |f| f.records.iter().filter(|r| !r.deleted).count())
    }

    /// This PC takes the account's vault as it is (a second PC, or the
    /// user chose the account's vault over this one). Locked afterwards:
    /// it opens with the master password set where it was made.
    pub fn adopt(&mut self, header: RemoteHeader, items: Vec<RemoteItem>) -> Result<(), String> {
        let prefs = self.prefs();
        self.unlocked = None;
        self.file = Some(VaultFile {
            version: FORMAT_VERSION,
            vault_id: header.vault_id,
            kdf: header.kdf,
            wrapped_key: header.wrapped_key,
            recovery_wrapped_key: header.recovery_wrapped_key,
            records: items.into_iter().map(RemoteItem::into_record).collect(),
            prefs,
            header_dirty: false,
        });
        self.write()
    }

    /// Takes in the account's entries and returns the ones to send.
    ///
    /// Nothing from the account is trusted: an entry or a deletion is taken
    /// in only if it opens with the vault key, so only while the vault is
    /// open (locked, this PC just sends its own changes). One that does not
    /// open (damaged, or made without the key) is left out and counted, and
    /// this PC's own version is sent over it. An entry changed both here and
    /// elsewhere since the last sync is merged: the newer change wins, and a
    /// password it replaced goes into the entry's history.
    pub fn merge(&mut self, remote: Vec<RemoteItem>) -> Result<Merged, String> {
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        let Some(unlocked) = self.unlocked.as_mut() else {
            return Ok(Merged {
                outgoing: file.records.iter().filter(|r| r.dirty).cloned().collect(),
                rejected: 0,
            });
        };
        let mut changed = false;
        let mut rejected = 0;
        for item in remote {
            let theirs = open_remote(&unlocked.key, &item);
            let Some(index) = file.records.iter().position(|r| r.id == item.id) else {
                // New from elsewhere.
                match theirs {
                    Ok(entry) => {
                        changed = true;
                        apply(unlocked, &item.id, entry);
                        file.records.push(item.into_record());
                    }
                    Err(()) => rejected += 1,
                }
                continue;
            };
            let local = &file.records[index];
            let their_revision = item.revision;
            if !local.dirty {
                if their_revision > local.revision {
                    changed = true;
                    match theirs {
                        Ok(entry) => {
                            apply(unlocked, &item.id, entry);
                            file.records[index] = item.into_record();
                        }
                        // Not a real change: put this PC's version back over it.
                        Err(()) => {
                            rejected += 1;
                            let ours = unlocked.entries.get(&item.id).map(|e| (**e).clone());
                            // Damaged here as well: nothing to send; leave it be.
                            if ours.is_none() && !local.deleted {
                                continue;
                            }
                            let updated = local.updated_at;
                            file.records[index] =
                                sealed(&unlocked.key, &item.id, ours.as_ref(), updated, their_revision)?;
                        }
                    }
                }
                continue;
            }
            // Changed here, and not there since the last sync: ours goes up.
            if Some(their_revision) == local.base_revision {
                continue;
            }
            // Changed in both places.
            changed = true;
            let ours = unlocked.entries.get(&item.id).map(|e| (**e).clone());
            let local_updated = local.updated_at;
            let winner = match theirs {
                Ok(theirs) => resolve(ours, local_updated, theirs, item.updated_at),
                Err(()) => {
                    rejected += 1;
                    ours
                }
            };
            let updated = winner
                .as_ref()
                .map_or_else(now, |entry| entry.updated_at.max(item.updated_at));
            file.records[index] =
                sealed(&unlocked.key, &item.id, winner.as_ref(), updated, their_revision)?;
            apply(unlocked, &item.id, winner);
        }
        let outgoing = file.records.iter().filter(|r| r.dirty).cloned().collect();
        if changed {
            self.write()?;
        }
        Ok(Merged { outgoing, rejected })
    }

    /// The account now has `revision` of `id`.
    pub fn mark_pushed(&mut self, id: &str, revision: u64) -> Result<(), String> {
        let file = self.file.as_mut().ok_or("There is no vault yet.")?;
        if let Some(record) = file.records.iter_mut().find(|r| r.id == id) {
            record.base_revision = Some(revision);
            // An edit made while it was sending stays to be sent.
            if record.revision == revision {
                record.dirty = false;
            }
        }
        self.write()
    }

    fn write(&self) -> Result<(), String> {
        let file = self.file.as_ref().ok_or("There is no vault yet.")?;
        write_atomic(
            &self.path,
            &serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?,
        )
    }
}

/// The vault's header as the account's `password_vault` row holds it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RemoteHeader {
    pub vault_id: String,
    pub kdf: KdfParams,
    pub wrapped_key: String,
    pub recovery_wrapped_key: String,
}

/// One entry as the account's `password_items` row holds it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RemoteItem {
    pub id: String,
    pub revision: u64,
    pub deleted: bool,
    pub ciphertext: String,
    pub updated_at: u64,
}

impl RemoteItem {
    fn into_record(self) -> Record {
        Record {
            id: self.id,
            revision: self.revision,
            deleted: self.deleted,
            updated_at: self.updated_at,
            ciphertext: self.ciphertext,
            dirty: false,
            base_revision: Some(self.revision),
        }
    }

    pub fn from_record(record: &Record) -> Self {
        Self {
            id: record.id.clone(),
            revision: record.revision,
            deleted: record.deleted,
            ciphertext: record.ciphertext.clone(),
            updated_at: record.updated_at,
        }
    }
}

/// What a sync brought: the records to send, and how many from the account
/// did not open with the vault key and were left out.
pub struct Merged {
    pub outgoing: Vec<Record>,
    pub rejected: usize,
}

fn open_entry(key: &Key, id: &str, revision: u64, ciphertext: &str) -> Option<Entry> {
    let plain = crypto::open(key, &entry_aad(id, revision), ciphertext).ok()?;
    serde_json::from_slice(&plain).ok()
}

/// What an entry from the account holds, if it opens with the vault key:
/// the entry, or `None` for a (sealed) deletion.
fn open_remote(key: &Key, item: &RemoteItem) -> Result<Option<Entry>, ()> {
    if item.deleted {
        crypto::open(key, &tombstone_aad(&item.id, item.revision), &item.ciphertext)
            .map(|_| None)
            .map_err(|_| ())
    } else {
        open_entry(key, &item.id, item.revision, &item.ciphertext).map(Some).ok_or(())
    }
}

/// Shows or removes an entry in the open vault.
fn apply(unlocked: &mut Unlocked, id: &str, entry: Option<Entry>) {
    match entry {
        Some(entry) => {
            unlocked.entries.insert(id.to_string(), Zeroizing::new(entry));
        }
        None => {
            unlocked.entries.remove(id);
        }
    }
}

/// `entry` (or a deletion, for `None`) sealed as the revision after the
/// account's `their_revision`, to send over it.
fn sealed(
    key: &Key,
    id: &str,
    entry: Option<&Entry>,
    updated_at: u64,
    their_revision: u64,
) -> Result<Record, String> {
    let revision = their_revision + 1;
    let (deleted, ciphertext) = match entry {
        Some(entry) => {
            let plain = Zeroizing::new(serde_json::to_vec(entry).map_err(|e| e.to_string())?);
            (false, crypto::seal(key, &entry_aad(id, revision), &plain)?)
        }
        None => (true, crypto::seal(key, &tombstone_aad(id, revision), TOMBSTONE)?),
    };
    Ok(Record {
        id: id.to_string(),
        revision,
        deleted,
        updated_at,
        ciphertext,
        dirty: true,
        base_revision: Some(their_revision),
    })
}

/// An entry changed on two PCs since they last synced: the newer change
/// wins. When both kept it and the passwords differ, the losing password
/// goes into the history, so no password is ever lost. A deletion wins
/// only when it is newer than the edit.
fn resolve(ours: Option<Entry>, ours_at: u64, theirs: Option<Entry>, theirs_at: u64) -> Option<Entry> {
    match (ours, theirs) {
        (Some(ours), Some(theirs)) => {
            let (mut win, lose) = if ours_at >= theirs_at { (ours, theirs) } else { (theirs, ours) };
            if !lose.password.is_empty() && lose.password != win.password {
                win.history.insert(
                    0,
                    OldPassword {
                        password: lose.password.clone(),
                        changed_at: now(),
                    },
                );
                win.history.truncate(HISTORY);
            }
            Some(win)
        }
        (Some(ours), None) => (ours_at >= theirs_at).then_some(ours),
        (None, Some(theirs)) => (theirs_at > ours_at).then_some(theirs),
        (None, None) => None,
    }
}

#[allow(dead_code)]
/// "Riot Client" for "riot client", "riot", "riotclient": a title and a
/// program name that share a word of at least four letters.
fn matches_name(title: &str, name: &str) -> bool {
    let words = |s: &str| {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 4)
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let title_words = words(title);
    let squashed_name: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    words(name).iter().any(|w| title_words.contains(w))
        || title_words
            .iter()
            .any(|w| squashed_name.contains(w.as_str()))
}

fn clean_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault() -> (PathBuf, Vault) {
        let path =
            std::env::temp_dir().join(format!("myle-vault-test-{}.json", uuid::Uuid::new_v4()));
        let vault = Vault::open(path.clone()).unwrap();
        (path, vault)
    }

    fn input(title: &str, password: &str) -> EntryInput {
        EntryInput {
            id: None,
            title: title.into(),
            username: "me@example.com".into(),
            password: Some(password.into()),
            urls: vec!["https://example.com".into()],
            apps: Vec::new(),
            notes: String::new(),
            favorite: false,
            folder: String::new(),
            totp: None,
        }
    }

    #[test]
    fn browser_filling_is_on_by_default_and_turned_on_once_for_older_vaults() {
        let (path, mut vault) = temp_vault();
        assert!(vault.prefs().browser_filling, "on before there is a vault");
        vault.create("master one", KdfParams::cheap_for_tests()).unwrap();
        assert!(vault.prefs().browser_filling);
        assert!(!vault.apply_filling_default().unwrap(), "a new vault needs nothing");

        // A vault from before: filling off, and no sign it was ever turned on.
        let mut text: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        text["prefs"] = serde_json::json!({ "autoLockMinutes": 5, "browserFilling": false, "websiteIcons": true });
        std::fs::write(&path, serde_json::to_vec(&text).unwrap()).unwrap();
        let mut older = Vault::open(path.clone()).unwrap();
        assert!(!older.prefs().browser_filling);
        assert!(older.apply_filling_default().unwrap());
        assert!(Vault::open(path.clone()).unwrap().prefs().browser_filling, "saved");

        // Turned off afterwards: it stays off.
        let mut prefs = older.prefs();
        prefs.browser_filling = false;
        older.set_prefs(prefs).unwrap();
        let mut again = Vault::open(path.clone()).unwrap();
        assert!(!again.apply_filling_default().unwrap());
        assert!(!again.prefs().browser_filling);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_2fa_key_is_kept_changed_or_removed_and_newer_fields_survive_an_edit() {
        let (path, mut vault) = temp_vault();
        vault.create("master one", KdfParams::cheap_for_tests()).unwrap();
        let mut with_key = input("GitHub", "p1");
        with_key.totp = Some("HXDM VJEC JJWS RB3H WIZR 4IFU GFTM XBOZ".into());
        let id = vault.save(&with_key).unwrap();
        let key = vault.totp(&id).unwrap();
        assert_eq!((key.issuer.as_str(), key.account.as_str()), ("GitHub", "me@example.com"), "named after the login");
        assert!(vault.summaries().unwrap()[0].has_totp);

        // An edit that leaves the key alone keeps it.
        let mut edit = input("GitHub", "p2");
        edit.id = Some(id.clone());
        vault.save(&edit).unwrap();
        assert_eq!(vault.totp(&id).unwrap(), key);
        // A key that is not one is refused, and nothing changes.
        edit.totp = Some("not a key".into());
        assert!(vault.save(&edit).is_err());
        assert_eq!(vault.totp(&id).unwrap(), key);
        // Emptied: removed.
        edit.totp = Some(String::new());
        vault.save(&edit).unwrap();
        assert!(vault.totp(&id).is_err());
        assert!(!vault.summaries().unwrap()[0].has_totp);

        // A field from a newer MYLE comes through an edit here unchanged.
        let mut entry: Entry = serde_json::from_value(serde_json::json!({
            "title": "Newer", "password": "x", "fromLater": [{ "site": "example.com" }]
        }))
        .unwrap();
        assert!(entry.extra.contains_key("fromLater"));
        vault.add_all(vec![std::mem::take(&mut entry)]).unwrap();
        let newer = vault.summaries().unwrap().into_iter().find(|s| s.title == "Newer").unwrap().id;
        let mut rename = input("Newer, renamed", "x");
        rename.id = Some(newer.clone());
        vault.save(&rename).unwrap();
        let stored = &vault.unlocked().unwrap().entries[&newer];
        assert_eq!(stored.extra["fromLater"][0]["site"], "example.com");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_passkey_joins_its_login_and_replaces_one_of_the_same_account() {
        use super::super::passkeys::Passkey;
        let (path, mut vault) = temp_vault();
        vault.create("master one", KdfParams::cheap_for_tests()).unwrap();
        let login = vault.save(&input("Example", "p1")).unwrap();
        let key = |credential: &str, handle: &str| Passkey {
            credential_id: credential.into(),
            rp_id: "example.com".into(),
            user_handle: handle.into(),
            user_name: "me@example.com".into(),
            key: "sealed with the entry".into(),
            ..Passkey::default()
        };
        assert_eq!(vault.add_passkey(Some(&login), key("c1", "h1"), "Example", "https://example.com").unwrap(), login);
        // The site made a new one for the same account: it replaces the old.
        vault.add_passkey(Some(&login), key("c2", "h1"), "Example", "https://example.com").unwrap();
        let found = vault.passkeys_for("example.com", &[]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].2.credential_id, "c2");

        // Another account with no login yet: a login of its own.
        let other = vault.add_passkey(None, key("c3", "h2"), "Example", "https://example.com").unwrap();
        assert_ne!(other, login);
        assert_eq!(vault.passkeys_for("example.com", &[]).unwrap().len(), 2);
        assert_eq!(vault.passkeys_for("example.com", &["c3".to_string()]).unwrap().len(), 1, "only those allowed");
        assert!(vault.passkeys_for("other.com", &[]).unwrap().is_empty());

        // The page sees them, never their keys.
        let summaries = vault.summaries().unwrap();
        let shown = summaries.iter().find(|s| s.id == other).unwrap();
        assert_eq!(shown.username, "me@example.com");
        assert!(!shown.has_password);
        assert_eq!(shown.passkeys[0].credential_id, "c3");
        assert!(!serde_json::to_string(&summaries).unwrap().contains("sealed with the entry"));

        // Kept through locking, and deleted one by one.
        vault.lock();
        vault.unlock("master one").unwrap();
        vault.delete_passkey(&login, "c2").unwrap();
        assert!(vault.delete_passkey(&login, "c2").is_err());
        assert_eq!(vault.passkeys_for("example.com", &[]).unwrap().len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_2fa_key_from_a_site_joins_its_login_or_makes_one() {
        use super::super::totp::Totp;
        let (path, mut vault) = temp_vault();
        vault.create("master one", KdfParams::cheap_for_tests()).unwrap();
        let login = vault.save(&input("Example", "p1")).unwrap();
        let key = Totp::parse("otpauth://totp/Example:me@example.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ").unwrap();
        vault.set_totp(&login, &key).unwrap();
        assert_eq!(vault.totp(&login).unwrap().code_at(59), key.code_at(59));
        assert_eq!(vault.password(&login).unwrap().as_str(), "p1", "nothing else changes");

        // A key alone, for a site with no login yet: named after the site.
        let bare = Totp::parse("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP").unwrap();
        let id = vault.add_totp_login("shop.example.org", "https://shop.example.org", &bare).unwrap();
        let entry = vault.summaries().unwrap().into_iter().find(|e| e.id == id).unwrap();
        assert!(entry.has_totp && !entry.has_password);
        assert_eq!(entry.urls, ["https://shop.example.org"]);
        assert_eq!(vault.totp(&id).unwrap().issuer, "shop.example.org");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn entries_survive_locking_and_reopening_but_only_with_the_master_password() {
        let (path, mut vault) = temp_vault();
        assert_eq!(vault.status(), Status::New);
        let code = vault
            .create("master one", KdfParams::cheap_for_tests())
            .unwrap();
        let id = vault.save(&input("Example", "hunter2")).unwrap();
        vault.lock();
        assert_eq!(vault.status(), Status::Locked);
        assert!(vault.summaries().is_err());

        let mut reopened = Vault::open(path.clone()).unwrap();
        assert!(reopened.unlock("master two").is_err());
        reopened.unlock("master one").unwrap();
        assert_eq!(reopened.password(&id).unwrap().as_str(), "hunter2");

        // The file on disk never holds the password or the title in clear.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("hunter2") && !raw.contains("Example"));

        // A forgotten master password: the recovery code sets a new one.
        reopened.lock();
        reopened
            .recover(&code, "master three", KdfParams::cheap_for_tests())
            .unwrap();
        reopened.lock();
        reopened.unlock("master three").unwrap();
        assert!(reopened.clone_status_is_unlocked());
        let _ = std::fs::remove_file(path);
    }

    impl Vault {
        fn clone_status_is_unlocked(&self) -> bool {
            self.status() == Status::Unlocked
        }
    }

    #[test]
    fn changing_a_password_keeps_the_old_one_and_the_master_change_keeps_the_entries() {
        let (path, mut vault) = temp_vault();
        vault.create("first", KdfParams::cheap_for_tests()).unwrap();
        let id = vault.save(&input("Site", "old-pass")).unwrap();
        let mut change = input("Site", "new-pass");
        change.id = Some(id.clone());
        vault.save(&change).unwrap();
        assert_eq!(vault.history(&id).unwrap()[0].password, "old-pass");

        vault
            .change_master("first", "second", KdfParams::cheap_for_tests())
            .unwrap();
        vault.lock();
        assert!(vault.unlock("first").is_err());
        vault.unlock("second").unwrap();
        assert_eq!(vault.password(&id).unwrap().as_str(), "new-pass");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn importing_what_the_vault_already_has_adds_no_copies() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        vault.save(&input("Mail", "p1")).unwrap();
        let imported = |title: &str, password: &str| Entry {
            title: title.into(),
            username: "Me@Example.com ".into(),
            password: password.into(),
            urls: vec!["https://example.com".into()],
            apps: Vec::new(),
            notes: String::new(),
            favorite: false,
            folder: String::new(),
            history: Vec::new(),
            created_at: 0,
            updated_at: 0,
            ..Entry::default()
        };
        // The same login again, twice in the file, and one really new.
        let file = vec![imported("Mail", "p1"), imported("Bank", "p2"), imported("Bank", "p2")];
        assert_eq!(vault.add_all(file).unwrap(), 1);
        assert_eq!(vault.summaries().unwrap().len(), 2);
        // A changed password is a different login: it is added.
        assert_eq!(vault.add_all(vec![imported("Mail", "p3")]).unwrap(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reused_passwords_are_flagged_and_deletions_are_kept_for_the_sync() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        let a = vault.save(&input("A", "same")).unwrap();
        vault.save(&input("B", "same")).unwrap();
        assert!(vault.summaries().unwrap().iter().all(|s| s.reused));
        vault.delete(&a).unwrap();
        let record = vault
            .file
            .as_ref()
            .unwrap()
            .records
            .iter()
            .find(|r| r.id == a)
            .unwrap();
        assert!(record.deleted && !record.ciphertext.is_empty() && record.revision == 2, "a sealed deletion");
        assert_eq!(vault.summaries().unwrap().len(), 1);
        let _ = std::fs::remove_file(path);
    }

    /// What the account would hold after `vault` sent everything.
    fn account_copy(vault: &Vault) -> (RemoteHeader, Vec<RemoteItem>) {
        let (header, _) = vault.sync_header().unwrap();
        let items = vault.file.as_ref().unwrap().records.iter().map(RemoteItem::from_record).collect();
        (header, items)
    }

    fn send_all(vault: &mut Vault) {
        let dirty: Vec<(String, u64)> = vault
            .file
            .as_ref()
            .unwrap()
            .records
            .iter()
            .filter(|r| r.dirty)
            .map(|r| (r.id.clone(), r.revision))
            .collect();
        for (id, revision) in dirty {
            vault.mark_pushed(&id, revision).unwrap();
        }
    }

    #[test]
    fn a_second_pc_takes_the_account_vault_and_opens_it_with_the_same_master_password() {
        let (path_a, mut a) = temp_vault();
        a.create("shared master", KdfParams::cheap_for_tests()).unwrap();
        let id = a.save(&input("Mail", "p1")).unwrap();
        send_all(&mut a);
        let (header, items) = account_copy(&a);

        let (path_b, mut b) = temp_vault();
        b.adopt(header, items).unwrap();
        assert_eq!(b.status(), Status::Locked);
        b.unlock("shared master").unwrap();
        assert_eq!(b.password(&id).unwrap().as_str(), "p1");
        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }

    #[test]
    fn an_entry_changed_on_two_pcs_keeps_the_newer_and_both_passwords() {
        let (path_a, mut a) = temp_vault();
        a.create("m", KdfParams::cheap_for_tests()).unwrap();
        let id = a.save(&input("Mail", "first")).unwrap();
        send_all(&mut a);
        let (header, items) = account_copy(&a);
        let (path_b, mut b) = temp_vault();
        b.adopt(header, items).unwrap();
        b.unlock("m").unwrap();

        // B changes it and sends it first; A changes it too, a little later.
        let mut on_b = input("Mail", "from-b");
        on_b.id = Some(id.clone());
        b.save(&on_b).unwrap();
        send_all(&mut b);
        let (_, b_items) = account_copy(&b);
        let mut on_a = input("Mail", "from-a");
        on_a.id = Some(id.clone());
        a.save(&on_a).unwrap();
        // Make A's edit clearly the newer one.
        a.file.as_mut().unwrap().records[0].updated_at += 10;

        // Locked, A cannot merge: it waits, still to be sent.
        a.lock();
        let waiting = a.merge(b_items.clone()).unwrap().outgoing;
        assert!(waiting.iter().any(|r| r.id == id && r.revision == 2 && r.base_revision == Some(1)));

        a.unlock("m").unwrap();
        let out = a.merge(b_items).unwrap().outgoing;
        let merged = out.iter().find(|r| r.id == id).unwrap();
        assert_eq!((merged.revision, merged.base_revision), (3, Some(2)));
        assert_eq!(a.password(&id).unwrap().as_str(), "from-a");
        let history: Vec<String> = a.history(&id).unwrap().into_iter().map(|h| h.password).collect();
        assert!(history.contains(&"from-b".to_string()), "{history:?}");
        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }

    #[test]
    fn a_deletion_and_a_new_entry_from_the_account_arrive() {
        let (path_a, mut a) = temp_vault();
        a.create("m", KdfParams::cheap_for_tests()).unwrap();
        let keep = a.save(&input("Keep", "1")).unwrap();
        let gone = a.save(&input("Gone", "2")).unwrap();
        send_all(&mut a);
        let (header, items) = account_copy(&a);
        let (path_b, mut b) = temp_vault();
        b.adopt(header, items).unwrap();
        b.unlock("m").unwrap();

        a.delete(&gone).unwrap();
        let added = a.save(&input("Added", "3")).unwrap();
        send_all(&mut a);
        let (_, items) = account_copy(&a);
        assert!(b.merge(items).unwrap().outgoing.is_empty(), "nothing of B's to send");
        let titles: Vec<String> = b.summaries().unwrap().into_iter().map(|s| s.title).collect();
        assert_eq!(titles.len(), 2);
        assert!(b.password(&keep).is_ok() && b.password(&added).is_ok() && b.password(&gone).is_err());
        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }

    #[test]
    fn wrong_master_passwords_slow_down_and_the_export_asks_again() {
        let (path, mut vault) = temp_vault();
        vault.create("right master!", KdfParams::cheap_for_tests()).unwrap();
        vault.lock();
        for _ in 0..FREE_TRIES {
            assert_eq!(vault.unlock("wrong").unwrap_err(), "Wrong master password.");
        }
        // Now even the right one has to wait.
        assert!(vault.unlock("right master!").unwrap_err().starts_with("Too many wrong tries"));
        vault.retry_at = None;
        vault.unlock("right master!").unwrap();
        assert_eq!(vault.failed_tries, 0);
        // Open, but an export still needs the master password.
        assert!(vault.verify_master("guess").is_err());
        assert!(vault.verify_master("right master!").is_ok());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn nothing_from_the_account_is_taken_in_while_locked() {
        let (path_a, mut a) = temp_vault();
        a.create("m", KdfParams::cheap_for_tests()).unwrap();
        a.save(&input("Mail", "p")).unwrap();
        send_all(&mut a);
        let (header, items) = account_copy(&a);
        let (path_b, mut b) = temp_vault();
        b.adopt(header, items).unwrap();
        b.unlock("m").unwrap();
        let added = b.save(&input("New", "x")).unwrap();
        send_all(&mut b);
        let (_, from_b) = account_copy(&b);

        a.lock();
        let merged = a.merge(from_b.clone()).unwrap();
        assert!(merged.outgoing.is_empty() && merged.rejected == 0);
        a.unlock("m").unwrap();
        assert!(a.password(&added).is_err(), "not taken in while locked");
        a.merge(from_b).unwrap();
        assert_eq!(a.password(&added).unwrap().as_str(), "x");
        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }

    #[test]
    fn forged_entries_and_deletions_from_the_account_are_refused() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        let id = vault.save(&input("Bank", "secret")).unwrap();
        send_all(&mut vault);

        // Someone with the account but not the key: a "deletion" and a new
        // entry of their own.
        let forged_delete = RemoteItem {
            id: id.clone(),
            revision: 5,
            deleted: true,
            ciphertext: String::new(),
            updated_at: now(),
        };
        let forged_new = RemoteItem {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 1,
            deleted: false,
            ciphertext: crypto::seal(&Key::random(), &entry_aad("x", 1), b"{}").unwrap(),
            updated_at: now(),
        };
        let merged = vault.merge(vec![forged_delete, forged_new]).unwrap();
        assert_eq!(merged.rejected, 2);
        assert_eq!(vault.password(&id).unwrap().as_str(), "secret", "still there");
        assert_eq!(vault.summaries().unwrap().len(), 1, "the forged entry is not shown");
        // This PC's version goes back over the forged row, one revision up.
        let resend = merged.outgoing.iter().find(|r| r.id == id).unwrap();
        assert_eq!((resend.revision, resend.base_revision, resend.deleted), (6, Some(5), false));

        // A real deletion (sealed with the key) is taken in.
        let (_, before) = account_copy(&vault);
        vault.delete(&id).unwrap();
        let deletion = vault.file.as_ref().unwrap().records.iter().find(|r| r.id == id).unwrap().clone();
        assert!(deletion.deleted && !deletion.ciphertext.is_empty(), "sealed");
        let (path_b, mut other) = temp_vault();
        let (header, _) = vault.sync_header().unwrap();
        other.adopt(header, before).unwrap();
        other.unlock("m").unwrap();
        assert!(other.password(&id).is_ok());
        let merged = other.merge(vec![RemoteItem::from_record(&deletion)]).unwrap();
        assert_eq!(merged.rejected, 0);
        assert!(other.password(&id).is_err(), "deleted there too");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path_b);
    }

    #[test]
    fn a_damaged_entry_does_not_keep_the_vault_shut() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        let good = vault.save(&input("Good", "1")).unwrap();
        let bad = vault.save(&input("Bad", "2")).unwrap();
        vault.lock();
        let record = vault.file.as_mut().unwrap().records.iter_mut().find(|r| r.id == bad).unwrap();
        record.ciphertext = crypto::seal(&Key::random(), &entry_aad(&bad, record.revision), b"{}").unwrap();
        vault.unlock("m").unwrap();
        assert_eq!(vault.damaged(), 1);
        assert_eq!(vault.password(&good).unwrap().as_str(), "1");
        assert!(vault.password(&bad).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn only_the_user_keeps_the_vault_open() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        let id = vault.save(&input("Site", "p")).unwrap();
        let before = Instant::now() - Duration::from_secs(600);
        vault.last_used = before;
        // What the browser or the sync does leaves the idle clock alone.
        vault.summaries().unwrap();
        vault.password(&id).unwrap();
        assert_eq!(vault.last_used, before);
        vault.touch();
        assert!(vault.last_used > before);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn website_icons_are_kept_sealed_and_only_while_open() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        vault.icons().unwrap().icons.insert(
            "github.com".into(),
            CachedIcon { data: Some("data:image/png;base64,AAAA".into()), at: 1 },
        );
        vault.save_icons().unwrap();
        let on_disk = std::fs::read_to_string(vault.icons_path()).unwrap();
        assert!(!on_disk.contains("github"), "the sites are not readable on disk");

        vault.lock();
        assert!(vault.icons().is_err(), "nothing while locked");
        vault.unlock("m").unwrap();
        assert!(vault.icons().unwrap().icons.contains_key("github.com"), "read back with the key");

        vault.forget_icons();
        assert!(!vault.icons_path().exists());
        assert!(vault.icons().unwrap().icons.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn programs_find_their_linked_entries_first() {
        let (path, mut vault) = temp_vault();
        vault.create("m", KdfParams::cheap_for_tests()).unwrap();
        let mut riot = input("Riot Games", "x");
        riot.apps = vec![AppLink {
            exe: "riotclientux.exe".into(),
            name: "Riot Client".into(),
        }];
        let linked = vault.save(&riot).unwrap();
        let guessed = vault.save(&input("Steam account", "y")).unwrap();
        assert_eq!(
            vault.for_app("RiotClientUx.exe", "Riot Client").unwrap(),
            vec![(linked, true)]
        );
        // Not linked yet: offered by name, marked as a guess.
        assert_eq!(
            vault.for_app("steam.exe", "Steam").unwrap(),
            vec![(guessed, false)]
        );
        assert!(vault.for_app("notepad.exe", "Notepad").unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }
}
