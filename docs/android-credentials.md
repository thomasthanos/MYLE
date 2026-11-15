# Android passwords, passkeys and 2FA

MYLE Passwords on Android is a viewer for the synced vault. It does not act as an Android Autofill service or a Credential Manager provider: those were removed in 10.0.7, and the release workflow (`.github/workflows/mobile.yml`) refuses an APK whose manifest registers either.

What the Android app does:

- **Passwords.** Open, search, view and copy logins. A copied password is cleared from the clipboard after a short while.
- **2FA codes.** Saved TOTP codes show with their countdown and a copy button. A site's QR code can be scanned to add one; the scanner stops when the vault locks.
- **Passkeys.** Not shown or used on Android. They stay in the encrypted vault for MYLE on the PC and on iPhone: a login that holds a passkey cannot be deleted on Android, and "Delete all" keeps passkeys. Manage them in MYLE on your PC.
- **Sync.** The same encrypted vault as MYLE on the PC, through the signed-in account.
- **Quick unlock.** A fingerprint or face unlocks the vault (`backend/mobile/src/unlock.rs`). A random key, kept by the Android Keystore through `tauri-plugin-biometry`, wraps the vault key; only the wrapped copy is stored in the app's folder. When the fingerprints or faces on the phone change, Android invalidates that key: quick unlock turns itself off and the app says why. The master password always works.

The app's data stays on the phone: Android backup and device-to-device transfer are turned off for it (`scripts/android-backup.mjs`, checked on the built APK), and the app switcher shows no picture of the vault.
