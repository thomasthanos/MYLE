# Android credential provider

User request: make MYLE on Redmi Note 14 Pro, Samsung A33 and other Android phones support passkeys, password filling, TOTP 2FA and fingerprint/master-password protection. Actual OS versions are unknown; discover capabilities at runtime.

Use Android Credential Manager provider APIs on API 34+, Autofill on API 26+, the existing encrypted and synced vault, and the existing ES256 passkey implementation. Never expose secrets before explicit selection and fingerprint or master-password verification. Store any fingerprint unlock key only in Android Keystore with per-operation strong biometric authentication. Master password remains available; changed biometric enrollment invalidates the fingerprint key. Credential activities hide screenshots and do not log secrets.

Provider requests must validate caller origin/certificate, browser privileged allowlist and native app Digital Asset Links. Reject unverified website claims. Bind temporary verification tokens to the vault, destination and operation; expire and consume tokens, including cancellation. Do not create a plaintext credential mirror or duplicate writable vault instance. Only Android can bind the exported services; credential activities are private and pending intents explicit and uniquely addressed.

Android browser clientDataHash must be used for signing and clientDataJSON omitted in that case, matching AndroidX. Native app clientData includes the certificate-based origin and androidPackageName. Respect algorithms, allow/exclude IDs and unsupported WebAuthn extensions. Do not claim universal site compatibility or that a provider can force its display name everywhere.

TOTP means codes from vault secrets; no SMS access is requested. Filling respects the current destination, fields and the selected login. Credentials survive app termination, and the existing app synchronizes changes when opened. Existing vault format and credentials remain usable.

Validation includes independent RP verification, scope/cancellation/expiry tests, Node/Rust suites, an actual Android build and native unit tests. Device/OEM and real account tests remain explicitly pending when phones or account reauthentication are unavailable. Do not submit external AAGUID registry PRs. Preserve already-published 10.0.4.
