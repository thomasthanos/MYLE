# Android passwords, passkeys and 2FA

Open MYLE Passwords, sign in and unlock/sync the vault. In **Account → Use MYLE in other apps**:

1. Select MYLE as the Android Autofill service.
2. On Android 14+, enable MYLE as a credential provider for passkeys and Credential Manager passwords.
3. Enable **Fingerprint for filling** if desired. Setup verifies the master password and fingerprint. Otherwise each operation uses the master password.
4. In Chrome, enable **Autofill using another service** if browser filling is not offered.

Android settings names vary on Xiaomi/HyperOS and Samsung/One UI. MYLE reads capabilities and enabled-service state from the device; the model name alone does not establish the installed Android version.

| Operation | Requirement |
| --- | --- |
| Android Credential Manager passkey/password provider | Android 14+; provider enabled |
| Native app password/TOTP Autofill | Android 8+; Autofill enabled |
| Protected Autofill save; HTTPS browser Autofill | Android 9+ |
| Fingerprint filling | Enrolled strong biometric; per-operation Keystore authorization |
| TOTP | A saved TOTP key for the chosen entry; no SMS permission |

Select the intended login in MYLE's protected chooser. A website claim must come from a certificate-allowlisted browser or a native app's verified Digital Asset Links. Native apps without a website claim use an explicit manual chooser displaying the app identity. Unverified website claims are rejected. A login must contain every secret field requested by the form; TOTP-only entries do not appear as Credential Manager passwords.

Fingerprint material is encrypted with an authentication-bound Android Keystore AES-GCM key, with the vault ID authenticated as associated data. Changed enrollment or a replaced vault makes old setup unusable and the master password remains available. Private activities block screenshots; pending intents carry opaque bounded tickets rather than vault secrets. Rust re-verifies passwords even when the normal app has the vault open. Cold provider sessions wipe the vault they opened after completion, cancellation or expiry.

The app and providers share one encrypted vault and one in-process VaultCell. Created/updated records are durable immediately and synchronize through the existing app when it runs. Existing credentials and the MYLE AAGUID remain unchanged. Same-account discoverable passkey replacement follows WebAuthn's RP/user-handle semantics; use a site's existing-key exclusion protection when registering another credential.

Supported WebAuthn responses use ES256/P-256. Hardware attestation and extensions such as PRF/largeBlob are not emulated. The desktop wrapper delegates unsupported requests to the browser. No provider can force a website to display the name MYLE; supported naming-field assistance remains conditional.

Verification covers Rust authorization/crypto tests, an independent SimpleWebAuthn RP verifier, extension routing tests, native Kotlin tests and the Android build. Physical Xiaomi/Samsung behavior, fingerprint enrollment/prompt interaction, browser selectors and real account ceremonies remain separate device tests when those devices/accounts are unavailable. Passing protocol tests is not a claim of universal website acceptance.

Primary references: <a href="https://developer.android.com/identity/sign-in/credential-provider"><kbd>Android credential providers</kbd></a>, <a href="https://developer.android.com/identity/autofill/autofill-services"><kbd>Autofill services</kbd></a>, <a href="https://www.w3.org/TR/webauthn-3/"><kbd>WebAuthn Level 3</kbd></a>, <a href="https://simplewebauthn.dev/docs/packages/server"><kbd>SimpleWebAuthn verification</kbd></a>.
