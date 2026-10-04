//! The vault as an app holds it: opened from its file the first time it is
//! used, and shared by the app's commands, the sync and the website icons.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::vault::Vault;

type PathFn = dyn Fn() -> Result<PathBuf, String> + Send + Sync;

#[derive(Clone)]
pub struct VaultCell {
    vault: Arc<Mutex<Option<Vault>>>,
    path: Arc<PathFn>,
}

impl VaultCell {
    /// `path`: where the vault's file is (asked once, when it is first used).
    pub fn new(path: impl Fn() -> Result<PathBuf, String> + Send + Sync + 'static) -> Self {
        Self {
            vault: Arc::default(),
            path: Arc::new(path),
        }
    }

    /// The vault as it is now, `None` until first used.
    pub fn lock(&self) -> MutexGuard<'_, Option<Vault>> {
        self.vault.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Runs `f` on the vault, opening its file the first time. It does not
    /// count as using the vault: background work (the sync, the icons, a
    /// browser's look-ups) never keeps an unattended vault open.
    pub fn with_quiet<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        let mut slot = self.lock();
        if slot.is_none() {
            *slot = Some(Vault::open((self.path)()?)?);
        }
        f(slot.as_mut().expect("opened above"))
    }

    /// The same, for something the user did: the vault stays open longer.
    pub fn with<T>(&self, f: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        self.with_quiet(|vault| {
            let result = f(vault);
            vault.touch();
            result
        })
    }

    /// Whether this device has a vault (made here or taken from the account).
    pub fn has_vault(&self) -> bool {
        self.with_quiet(|vault| Ok(vault.status() != crate::vault::Status::New))
            .unwrap_or(false)
    }
}
