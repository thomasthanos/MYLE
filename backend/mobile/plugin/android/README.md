# Android credential integration

Credential Manager requires Android 14/API 34; Autofill requires Android 8/API 26. The manifest enables each system-bound service only on supported Android versions. Credential activities are private and use FLAG_SECURE.

NativeVault shares the Tauri vault at applicationInfo.dataDir. Every secret operation verifies the master password or decrypts the wrapped vault key with a per-use strong-biometric AES-GCM CryptoObject. The authenticated record's vault ID is passed to unlockKey. Enrollment always verifies the master password independently. Fingerprint records live in noBackupFilesDir and contain encrypted key bytes only.

The bundled privileged-browser allowlist is a 2026-10-14 snapshot of https://www.gstatic.com/gpm-passkeys-privileged-apps/apps.json, linked by https://developer.android.com/identity/sign-in/credential-provider. Only production certificates are accepted. Native website/RP claims require exact package/current-certificate Digital Asset Links with delegate_permission/common.get_login_creds over HTTPS, without redirects. Credential Manager browser Related Origin Requests use the trusted browser's supplied 32-byte clientDataHash. Autofill uses the current verified page host; conflicting frame/domain claims are rejected.

Android 11+ package visibility hides most apps from MYLE. Credential Manager callers are identified by the signers Android supplies in CallingAppInfo (the system credential selector, not the caller, starts MYLE); a caller MYLE can see must also match its installed signers. Autofill gets no signers from Android: a fill makes the requesting app visible to MYLE, a save does not, so the manifest's `<queries>` name launcher apps and HTTPS browsers. A WebAuthn request without an RP ID is for the browser page's own host; an app must name its website. MYLE never fills or offers itself: its own package is excluded from Autofill and Credential Manager, and its webview is not important for autofill, so no service (MYLE's or another's) fills or saves the master password or vault fields. `adb logcat -s MYLE` shows why a request stopped (identities and reasons only, never secrets).

On API 26/27, browser Autofill cannot safely verify HTTPS because webScheme was introduced in API 28; those website claims are rejected. Native app Autofill is available. Protected Autofill save delegation requires SaveCallback.onSuccess(IntentSender), introduced in API 28. On earlier versions Android receives a clear save failure directing the user to MYLE. No SMS permission is requested; 2FA fills vault TOTP codes only.

After `npx tauri android build --apk --ci --target aarch64 --target armv7` generates the Gradle plugin settings during its Rust build, run from backend/mobile/gen/android (Android init alone is insufficient):

    ./gradlew :tauri-plugin-myle-mobile:testDebugUnitTest :tauri-plugin-myle-mobile:assembleDebug

Run the app APK build through the existing mobile workflow. Local compile evidence is Kotlin/JVM compilation against actual Android API 34/AndroidX 1.6.0 classes, including the Tauri plugin source; this does not replace Gradle manifest/resource merging, APK compilation, or physical OEM/browser tests.
