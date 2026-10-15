//! Android's private vault adapter. System caller validation belongs to the
//! native service; every secret operation also needs a fresh, destination-bound
//! verification token here. The normal app and services share this VaultCell.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use myle_vault::VaultCell;
use myle_vault::crypto::Key;
use myle_vault::passkeys::{self, ClientContext, User};
use myle_vault::vault::{AppLink, EntryInput, Status, Summary, Vault};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::{Zeroize, Zeroizing};

const SESSION_FOR: Duration = Duration::from_secs(90);
const VERIFY_AGAIN: &str = "Verify your fingerprint or master password again.";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Scope {
    origin: String,
    site: String,
    package_name: String,
    certificate: String,
}

impl Scope {
    fn read(payload: &Value) -> Result<Self, String> {
        let scope: Self = serde_json::from_value(payload["scope"].clone()).map_err(|_| "badRequest")?;
        if scope.package_name.is_empty() || scope.package_name.len() > 200 ||
            !scope.package_name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_') {
            return Err("badRequest".into());
        }
        let certificate = passkeys::unb64(&scope.certificate).filter(|bytes| bytes.len() == 32).ok_or("badRequest")?;
        if let Some(hash) = scope.origin.strip_prefix("android:apk-key-hash:") {
            if passkeys::unb64(hash).as_deref() != Some(certificate.as_slice()) {
                return Err("rpMismatch".into());
            }
        } else {
            let origin = reqwest::Url::parse(&scope.origin).map_err(|_| "rpMismatch")?;
            if origin.scheme() != "https" || origin.origin().ascii_serialization() != scope.origin ||
                origin.host_str().is_none() {
                return Err("rpMismatch".into());
            }
        }
        if !scope.manual_app() && passkeys::rp_id_for(&scope.site, Some(&scope.site))? != scope.site {
            return Err("rpMismatch".into());
        }
        Ok(scope)
    }

    fn native_app(&self) -> bool { self.origin.starts_with("android:apk-key-hash:") }
    fn manual_app(&self) -> bool { self.native_app() && self.site == self.package_name }
    fn app_link(&self) -> String { format!("android:{}:{}", self.package_name, self.certificate) }

    fn password_destination(&self) -> Result<(), String> {
        if !self.native_app() && reqwest::Url::parse(&self.origin).ok().and_then(|u| u.host_str().map(str::to_string)).as_deref() != Some(self.site.as_str()) {
            return Err("rpMismatch".into());
        }
        Ok(())
    }

    fn matches(&self, entry: &Summary) -> bool {
        if self.manual_app() { return true; } // private, explicit authenticated chooser
        entry.urls.iter().any(|url| myle_vault::sites::saved_host(url).is_some_and(|host| {
            passkeys::rp_id_for(&host, Some(&host)).is_ok() &&
                (host == self.site || self.site.ends_with(&format!(".{host}")))
        })) || entry.passkeys.iter().any(|key| passkeys::same_rp(&key.rp_id, &self.site))
    }
}

#[derive(Clone)]
struct Session {
    scope: Scope,
    vault_id: String,
    created: Instant,
    created_wall: SystemTime,
    opened: bool,
}

impl Session {
    fn expired(&self) -> bool {
        self.created.elapsed() >= SESSION_FOR ||
            !SystemTime::now().duration_since(self.created_wall).is_ok_and(|age| age < SESSION_FOR)
    }
}

type LockListener = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct Provider {
    pub(crate) cell: VaultCell,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    lock_listener: Arc<Mutex<Option<LockListener>>>,
}

impl Provider {
    pub(crate) fn new(cell: VaultCell) -> Self { Self { cell, sessions: Arc::default(), lock_listener: Arc::default() } }

    pub(crate) fn set_lock_listener(&self, listener: LockListener) {
        *self.lock_listener.lock().unwrap_or_else(|p| p.into_inner()) = Some(listener);
    }

    pub(crate) fn call(&self, action: &str, mut payload: Value) -> Result<Value, String> {
        let result = self.call_inner(action, &payload);
        erase(&mut payload);
        result
    }

    fn call_inner(&self, action: &str, payload: &Value) -> Result<Value, String> {
        if action == "status" { return Ok(json!({"hasVault":self.cell.has_vault()})); }
        if action == "cancel" {
            let token = text(payload, "session")?;
            let session = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(token);
            if let Some(session) = session { self.finish(&session); }
            return Ok(Value::Null);
        }
        let scope = Scope::read(payload)?;
        if action == "unlock" || action == "unlockKey" {
            let (vault_id, opened) = self.cell.with(|vault| {
                let opened = vault.status() != Status::Unlocked;
                if action == "unlock" { vault.unlock(text(payload, "password")?)?; }
                else {
                    if vault.vault_id().as_deref() != Some(text(payload, "vaultId")?) {
                        return Err("Fingerprint setup belongs to another vault. Use the master password.".into());
                    }
                    let key = Key::from_text(text(payload, "key")?)?;
                    if !opened {
                        // Validate the supplied key cryptographically against
                        // the known current key without changing open entries.
                        let witness = myle_vault::crypto::seal(&vault.key_copy()?, "myle-provider-key-check", b"verified")?;
                        myle_vault::crypto::open(&key, "myle-provider-key-check", &witness)
                            .map_err(|_| "Fingerprint setup stopped working. Use the master password.")?;
                    }
                    vault.unlock_with_key(key)?;
                    if vault.summaries()?.is_empty() && vault.damaged() != 0 {
                        vault.lock();
                        return Err("Fingerprint setup stopped working. Use the master password.".into());
                    }
                }
                Ok((vault.vault_id().ok_or("There is no vault yet.")?, opened))
            })?;
            let token = Key::random().to_text().to_string();
            let session = Session { scope, vault_id, created: Instant::now(), created_wall: SystemTime::now(), opened };
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            if sessions.len() >= 64 {
                drop(sessions);
                self.finish(&session);
                return Err("Too many pending requests. Try again shortly.".into());
            }
            sessions.insert(token.clone(), session);
            drop(sessions);
            // Cold-started native activities have no Tauri idle watcher yet.
            // Expire authorization and wipe a vault this request opened even
            // when the caller disappears without sending cancel.
            let (sessions, cell, listener, expiry_token) = (self.sessions.clone(), self.cell.clone(), self.lock_listener.clone(), token.clone());
            std::thread::spawn(move || {
                std::thread::sleep(SESSION_FOR);
                let expired = sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&expiry_token);
                if let Some(session) = expired { lock_owned(&cell, &session, &listener); }
            });
            return Ok(json!({"session":token}));
        }
        let token = text(payload, "session")?;
        let consume = action != "list";
        let session = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            if consume { sessions.remove(token) } else { sessions.get(token).cloned() }
        }.ok_or(VERIFY_AGAIN)?;
        if session.expired() || session.scope != scope {
            self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(token);
            self.finish(&session);
            return Err(VERIFY_AGAIN.into());
        }
        let result = self.cell.with(|vault| {
            if vault.status() != Status::Unlocked || vault.vault_id().as_deref() != Some(session.vault_id.as_str()) {
                return Err(VERIFY_AGAIN.into());
            }
            match action {
                "list" => self.list(vault, &scope, payload),
                "fill" => self.fill(vault, &scope, payload),
                "create" => self.create(vault, &scope, payload),
                "get" => self.get(vault, &scope, payload),
                "save" => self.save(vault, &scope, payload),
                "enrollKey" => Ok(json!({"key":vault.key_copy()?.to_text().as_str(), "vaultId":session.vault_id})),
                _ => Err("badRequest".into()),
            }
        });
        if consume || result.is_err() {
            self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(token);
            self.finish(&session);
        }
        result
    }

    fn finish(&self, session: &Session) {
        lock_owned(&self.cell, session, &self.lock_listener);
    }

    fn list(&self, vault: &mut Vault, scope: &Scope, payload: &Value) -> Result<Value, String> {
        if text(payload, "kind")? == "passkey" {
            let request = options(payload)?;
            check_rp(scope, &request, false)?;
            let allow = descriptors(&request, "allowCredentials")?;
            let keys = Zeroizing::new(vault.passkeys_for(&scope.site, &allow)?);
            Ok(Value::Array(keys.iter().map(|(id, title, key)| json!({"id":id,"title":title,"username":key.user_name,"displayName":key.user_display_name,"hasPassword":false,"hasTotp":false,"credentialId":key.credential_id})).collect()))
        } else if text(payload, "kind")? == "password" {
            scope.password_destination()?;
            Ok(Value::Array(vault.summaries()?.iter().filter(|entry| scope.matches(entry) && (entry.has_password || entry.has_totp))
                .map(|entry| json!({"id":entry.id,"title":entry.title,"username":entry.username,"hasPassword":entry.has_password,"hasTotp":entry.has_totp})).collect()))
        } else { Err("badRequest".into()) }
    }

    fn fill(&self, vault: &mut Vault, scope: &Scope, payload: &Value) -> Result<Value, String> {
        scope.password_destination()?;
        let id = text(payload, "id")?;
        if !vault.summaries()?.iter().any(|entry| entry.id == id && scope.matches(entry)) { return Err("noCredential".into()); }
        let fields = payload["fields"].as_array().filter(|a| !a.is_empty() && a.len() <= 3).ok_or("badRequest")?;
        let mut result = serde_json::Map::new();
        for field in fields {
            match field.as_str().ok_or("badRequest")? {
                "username" => { result.insert("username".into(), json!(vault.username(id)?)); }
                "password" => { result.insert("password".into(), json!(vault.password(id)?.as_str())); }
                "totp" => { result.insert("totp".into(), json!(vault.totp(id)?.now().code)); }
                _ => return Err("badRequest".into()),
            }
        }
        Ok(Value::Object(result))
    }

    fn create(&self, vault: &mut Vault, scope: &Scope, payload: &Value) -> Result<Value, String> {
        let request = options(payload)?;
        check_rp(scope, &request, true)?;
        check_extensions(&request)?;
        if request["authenticatorSelection"]["authenticatorAttachment"] == "cross-platform" { return Err("notSupported".into()); }
        if let Some(params) = request["pubKeyCredParams"].as_array()
            && !params.is_empty() && !params.iter().any(|p| p["type"] == "public-key" && p["alg"] == passkeys::ES256) {
            return Err("notSupported".into());
        }
        let exclude = descriptors(&request, "excludeCredentials")?;
        if !exclude.is_empty() && !vault.passkeys_for(&scope.site, &exclude)?.is_empty() { return Err("excluded".into()); }
        let user = User { handle: text(&request["user"], "id")?.into(), name: text(&request["user"], "name")?.into(), display_name: request["user"]["displayName"].as_str().unwrap_or("").into() };
        let hash = client_hash(payload, scope)?;
        let context = ClientContext { origin: &scope.origin, android_package: scope.native_app().then_some(scope.package_name.as_str()), hash: hash.as_deref() };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "badRequest")?.as_secs();
        let (key, mut response) = passkeys::create_with_context(&scope.site, request["rp"]["name"].as_str().unwrap_or(&scope.site), &user, text(&request, "challenge")?, &context, true, now)?;
        let target = vault.summaries()?.into_iter().find(|entry| entry.username == user.name && scope.matches(entry)).map(|entry| entry.id);
        vault.add_passkey(target.as_deref(), key, request["rp"]["name"].as_str().unwrap_or(&scope.site), &format!("https://{}", scope.site))?;
        response["clientExtensionResults"] = if request["extensions"]["credProps"] == true { json!({"credProps":{"rk":true}}) } else { json!({}) };
        Ok(response)
    }

    fn get(&self, vault: &mut Vault, scope: &Scope, payload: &Value) -> Result<Value, String> {
        let request = options(payload)?;
        check_rp(scope, &request, false)?;
        check_extensions(&request)?;
        let allow = descriptors(&request, "allowCredentials")?;
        let keys = Zeroizing::new(vault.passkeys_for(&scope.site, &allow)?);
        let selected = text(payload, "credentialId")?;
        let key = keys.iter().find(|(_, _, key)| passkeys::same_credential(&key.credential_id, selected)).map(|(_, _, key)| key).ok_or("noCredential")?;
        let hash = client_hash(payload, scope)?;
        let context = ClientContext { origin: &scope.origin, android_package: scope.native_app().then_some(scope.package_name.as_str()), hash: hash.as_deref() };
        let mut response = passkeys::sign_in_with_context(key, text(&request, "challenge")?, &context, true)?;
        response["clientExtensionResults"] = json!({});
        Ok(response)
    }

    fn save(&self, vault: &mut Vault, scope: &Scope, payload: &Value) -> Result<Value, String> {
        scope.password_destination()?;
        let username = payload["username"].as_str().unwrap_or("");
        let password = text(payload, "password")?;
        let marker = scope.app_link();
        let existing = vault.summaries()?.into_iter().find(|entry| entry.username == username &&
            if scope.manual_app() { entry.apps.iter().any(|app| app.exe == marker) } else { scope.matches(entry) });
        let mut input: EntryInput = serde_json::from_value(json!({
            "id":existing.as_ref().map(|e| &e.id),
            "title":existing.as_ref().map(|e| e.title.as_str()).unwrap_or(&scope.site),
            "username":username,"password":password,
            "urls":existing.as_ref().map(|e| e.urls.clone()).unwrap_or_else(|| if scope.manual_app() { vec![] } else { vec![format!("https://{}", scope.site)] }),
            "apps":existing.as_ref().map(|e| e.apps.clone()).unwrap_or_default(),
            "notes":existing.as_ref().map(|e| e.notes.as_str()).unwrap_or(""),
            "folder":existing.as_ref().map(|e| e.folder.as_str()).unwrap_or(""),
            "favorite":existing.as_ref().is_some_and(|e| e.favorite),
        })).map_err(|e| e.to_string())?;
        if scope.native_app() && !input.apps.iter().any(|app| app.exe == marker) {
            input.apps.push(AppLink { exe: marker, name: scope.package_name.clone() });
        }
        Ok(json!({"id":vault.save(&input)?}))
    }
}

fn lock_owned(cell: &VaultCell, session: &Session, listener: &Mutex<Option<LockListener>>) {
    if !session.opened { return; }
    let locked = cell.with_quiet(|vault| {
        let owned = vault.vault_id().as_deref() == Some(session.vault_id.as_str()) && vault.status() == Status::Unlocked;
        if owned { vault.lock(); }
        Ok(owned)
    }).unwrap_or(false);
    if locked {
        let callback = listener.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(callback) = callback { callback(); }
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key].as_str().filter(|text| !text.is_empty() && text.len() <= 16_384).ok_or_else(|| "badRequest".into())
}

fn options(payload: &Value) -> Result<Value, String> {
    let value = if let Some(text) = payload["request"].as_str() {
        if text.len() > 128 * 1024 { return Err("badRequest".into()); }
        serde_json::from_str(text).map_err(|_| "badRequest")?
    } else { payload["request"].clone() };
    if !value.is_object() { return Err("badRequest".into()); }
    Ok(value)
}

fn check_rp(scope: &Scope, request: &Value, create: bool) -> Result<(), String> {
    if scope.manual_app() { return Err("rpMismatch".into()); }
    let asked = if create { request["rp"]["id"].as_str() } else { request["rpId"].as_str() }.ok_or("badRequest")?;
    if !passkeys::same_rp(asked, &scope.site) { return Err("rpMismatch".into()); }
    Ok(())
}

fn check_extensions(request: &Value) -> Result<(), String> {
    if request["extensions"].as_object().is_some_and(|e| e.iter().any(|(key, value)| key != "credProps" && !value.is_null() && value != &Value::Bool(false))) {
        return Err("notSupported".into());
    }
    Ok(())
}

fn descriptors(request: &Value, name: &str) -> Result<Vec<String>, String> {
    if request[name].is_null() { return Ok(vec![]); }
    let values = request[name].as_array().filter(|a| a.len() <= 512).ok_or("badRequest")?;
    values.iter().map(|value| {
        if value["type"] != "public-key" { return Err("badRequest".into()); }
        let id = text(value, "id")?;
        if !passkeys::unb64(id).is_some_and(|bytes| !bytes.is_empty() && bytes.len() <= 1024) { return Err("badRequest".into()); }
        Ok(id.to_string())
    }).collect()
}

fn client_hash(payload: &Value, scope: &Scope) -> Result<Option<Vec<u8>>, String> {
    if scope.native_app() {
        if !payload["clientDataHash"].is_null() { return Err("badRequest".into()); }
        Ok(None)
    } else {
        // Only allowlisted Android browsers may supply this field, and their
        // actual client data must be retained rather than reconstructed here.
        let hash = passkeys::unb64(text(payload, "clientDataHash")?).filter(|b| b.len() == 32).ok_or("badRequest")?;
        Ok(Some(hash))
    }
}

pub(crate) fn erase(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(erase),
        Value::Object(values) => values.values_mut().for_each(erase),
        _ => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myle_vault::crypto::KdfParams;
    use myle_vault::vault::EntryInput;

    fn fixture() -> (Provider, std::path::PathBuf, Value) {
        let dir = std::env::temp_dir().join(format!("myle-provider-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("passwords.vault");
        let cell = VaultCell::new(move || Ok(path.clone()));
        cell.with(|vault| {
            let kdf = KdfParams { memory: 256, iterations: 1, ..KdfParams::new() };
            vault.create("a long test master password", kdf)?;
            let entry: EntryInput = serde_json::from_value(json!({"title":"Example", "username":"alice", "password":"website secret", "urls":["https://example.com"]})).unwrap();
            vault.save(&entry)?;
            vault.lock();
            Ok(())
        }).unwrap();
        let provider = Provider::new(cell);
        let scope = json!({"origin":"https://example.com","site":"example.com","packageName":"com.android.chrome","certificate":passkeys::b64(&[3;32])});
        (provider, dir, scope)
    }

    fn unlock(provider: &Provider, scope: &Value) -> String {
        provider.call("unlock", json!({"password":"a long test master password", "scope":scope})).unwrap()["session"].as_str().unwrap().to_string()
    }

    #[test]
    fn wrong_password_is_rejected_even_when_the_app_is_already_unlocked() {
        let (provider, dir, scope) = fixture();
        let _first = unlock(&provider, &scope);
        assert!(provider.call("unlock", json!({"password":"wrong", "scope":scope})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_cannot_change_destination_or_replay_a_fill() {
        let (provider, dir, scope) = fixture();
        let session = unlock(&provider, &scope);
        let list = provider.call("list", json!({"session":session,"scope":scope,"kind":"password"})).unwrap();
        let id = list[0]["id"].as_str().unwrap();
        let mut other = scope.clone();
        other["site"] = json!("evil.com");
        other["origin"] = json!("https://evil.com");
        assert!(provider.call("fill", json!({"session":session,"scope":other,"id":id,"fields":["password"]})).is_err());
        let session = unlock(&provider, &scope);
        let payload = json!({"session":session,"scope":scope,"id":id,"fields":["password"]});
        assert_eq!(provider.call("fill", payload.clone()).unwrap()["password"], "website secret");
        assert!(provider.call("fill", payload).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expired_cancelled_and_locked_sessions_cannot_reveal_secrets() {
        let (provider, dir, scope) = fixture();
        let session = unlock(&provider, &scope);
        provider.sessions.lock().unwrap().get_mut(&session).unwrap().created = Instant::now() - Duration::from_secs(91);
        assert!(provider.call("list", json!({"session":session,"scope":scope,"kind":"password"})).is_err());
        let session = unlock(&provider, &scope);
        provider.call("cancel", json!({"session":session})).unwrap();
        assert!(provider.call("list", json!({"session":session,"scope":scope,"kind":"password"})).is_err());
        let session = unlock(&provider, &scope);
        provider.cell.with(|vault| { vault.lock(); Ok(()) }).unwrap();
        assert!(provider.call("list", json!({"session":session,"scope":scope,"kind":"password"})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn one_site_never_lists_or_fills_another_sites_credentials() {
        let (provider, dir, mut scope) = fixture();
        let session = unlock(&provider, &scope);
        let id = provider.call("list", json!({"session":session,"scope":scope,"kind":"password"})).unwrap()[0]["id"].as_str().unwrap().to_string();
        provider.call("cancel", json!({"session":session})).unwrap();
        scope["site"] = json!("evil.com");
        scope["origin"] = json!("https://evil.com");
        let session = unlock(&provider, &scope);
        assert!(provider.call("list", json!({"session":session,"scope":scope,"kind":"password"})).unwrap().as_array().unwrap().is_empty());
        assert!(provider.call("fill", json!({"session":session,"scope":scope,"id":id,"fields":["password"]})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_creates_a_new_entry_and_returns_only_requested_fields() {
        let (provider, dir, scope) = fixture();
        let token = unlock(&provider, &scope);
        let saved = provider.call("save", json!({"session":token,"scope":scope,"username":"bob","password":"new secret"})).unwrap();
        let token = unlock(&provider, &scope);
        let result = provider.call("fill", json!({"session":token,"scope":scope,"id":saved["id"],"fields":["username"]})).unwrap();
        assert_eq!(result, json!({"username":"bob"}));
        assert_eq!(provider.cell.with_quiet(|v| Ok(v.status())).unwrap(), Status::Locked, "cold activity does not keep vault open");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacing_the_vault_invalidates_authorization() {
        let (provider, dir, scope) = fixture();
        let token = unlock(&provider, &scope);
        provider.cell.clear_local().unwrap();
        provider.cell.with(|vault| { vault.create("another master password", KdfParams {memory:256,iterations:1,..KdfParams::new()})?; Ok(()) }).unwrap();
        assert!(provider.call("list", json!({"session":token,"scope":scope,"kind":"password"})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cold_activity_lock_is_reported_and_clock_rollback_expires_proof() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (provider, dir, scope) = fixture();
        let locks = Arc::new(AtomicUsize::new(0));
        let observed = locks.clone();
        provider.set_lock_listener(Arc::new(move || { observed.fetch_add(1, Ordering::SeqCst); }));
        let token = unlock(&provider, &scope);
        provider.sessions.lock().unwrap().get_mut(&token).unwrap().created_wall = SystemTime::now() + Duration::from_secs(30);
        assert!(provider.call("list", json!({"session":token,"scope":scope,"kind":"password"})).is_err());
        assert_eq!(locks.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fingerprint_key_requires_its_authenticated_vault_binding() {
        let (provider, dir, scope) = fixture();
        let token = unlock(&provider, &scope);
        let enrolled = provider.call("enrollKey", json!({"session":token,"scope":scope})).unwrap();
        assert!(provider.call("unlockKey", json!({"key":enrolled["key"],"vaultId":"different vault","scope":scope})).is_err());
        let reply = provider.call("unlockKey", json!({"key":enrolled["key"],"vaultId":enrolled["vaultId"],"scope":scope})).unwrap();
        assert!(provider.call("unlockKey", json!({"key":Key::random().to_text().as_str(),"vaultId":enrolled["vaultId"],"scope":scope})).is_err());
        provider.call("cancel", json!({"session":reply["session"]})).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn creation() -> Value {
        json!({"rp":{"id":"example.com","name":"Example"},"user":{"id":passkeys::b64(b"alice-account"),"name":"alice","displayName":"Alice"},"challenge":passkeys::b64(b"registration challenge"),"pubKeyCredParams":[{"type":"public-key","alg":-7}],"extensions":{"credProps":true}})
    }

    #[test]
    fn passkeys_are_durable_and_enforce_exclusions_and_allow_ids() {
        let (provider, dir, scope) = fixture();
        let token = unlock(&provider, &scope);
        let hash = passkeys::b64(&[8;32]);
        let made = provider.call("create", json!({"session":token,"scope":scope,"request":creation(),"clientDataHash":hash})).unwrap();
        assert!(made["response"].get("clientDataJSON").is_none());
        assert_eq!(made["clientExtensionResults"]["credProps"]["rk"], true);
        let id = made["id"].as_str().unwrap();
        let mut another = creation();
        another["excludeCredentials"] = json!([{"type":"public-key","id":id}]);
        let token = unlock(&provider, &scope);
        assert_eq!(provider.call("create", json!({"session":token,"scope":scope,"request":another,"clientDataHash":hash})).unwrap_err(), "excluded");
        let token = unlock(&provider, &scope);
        let request = json!({"rpId":"example.com","challenge":passkeys::b64(b"login challenge"),"allowCredentials":[{"type":"public-key","id":format!("{id}==")}]});
        let keys = provider.call("list", json!({"session":token,"scope":scope,"kind":"passkey","request":request})).unwrap();
        assert_eq!(keys.as_array().unwrap().len(), 1);
        let signed = provider.call("get", json!({"session":token,"scope":scope,"request":request,"credentialId":id,"clientDataHash":hash})).unwrap();
        assert_eq!(signed["id"], id);
        let token = unlock(&provider, &scope);
        let disallowed = json!({"rpId":"example.com","challenge":passkeys::b64(b"login"),"allowCredentials":[{"type":"public-key","id":passkeys::b64(b"different credential")}]});
        assert_eq!(provider.call("get", json!({"session":token,"scope":scope,"request":disallowed,"credentialId":id,"clientDataHash":hash})).unwrap_err(), "noCredential");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_scopes_algorithms_and_browser_hashes_do_not_create_credentials() {
        let (provider, dir, scope) = fixture();
        for variant in ["algorithm","extension","hash","rp"] {
            let token = unlock(&provider, &scope);
            let mut request = creation();
            let mut hash = passkeys::b64(&[8;32]);
            match variant {
                "algorithm" => request["pubKeyCredParams"] = json!([{"type":"public-key","alg":-257}]),
                "extension" => request["extensions"]["prf"] = json!({}),
                "hash" => hash = passkeys::b64(&[8;31]),
                "rp" => request["rp"]["id"] = json!("evil.com"),
                _ => unreachable!(),
            }
            assert!(provider.call("create", json!({"session":token,"scope":scope,"request":request,"clientDataHash":hash})).is_err(), "{variant}");
        }
        provider.cell.with(|v| { v.unlock("a long test master password")?; assert!(v.passkeys_for("example.com", &[])?.is_empty()); Ok(()) }).unwrap();
        let mut spoofed = scope.clone();
        spoofed["origin"] = json!("http://example.com");
        assert!(provider.call("unlock", json!({"scope":spoofed,"password":"a long test master password"})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
