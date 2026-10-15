# Android Credentials Implementation Plan

> **For agentic workers:** Execute the independent Kotlin and Rust/desktop domains in parallel under superpowers:dispatching-parallel-agents, with a final independent review. Use superpowers:test-driven-development and superpowers:verification-before-completion for each domain.

**Goal:** Add protected Android passkey/password provider and password/TOTP autofill using MYLE's existing vault.

**Architecture:** Native Android system services return explicit PendingIntents into a private credential activity. Kotlin validates Android caller information and handles biometric CryptoObject prompts; Rust retains vault encryption, request authorization and passkey cryptography. The normal Tauri app and native activities share one VaultCell per process.

**Tech Stack:** Rust, JNI 0.21.1, Kotlin, AndroidX Credentials 1.6.0, AndroidX Biometric, existing Tauri/Svelte app.

**Spec:** `docs/superpowers/specs/2026-10-14-android-credentials.md`

## Global Constraints

- Credential Manager provider API 34+, Autofill API 26+, existing application minSdk 24.
- No plaintext persistent credentials, secrets in logs or additional SMS permissions.
- Strong fingerprint or validated master password before each secret operation; one-use authorization expires after 90 seconds.
- Existing encrypted vault format and AAGUID remain unchanged.
- No external AAGUID submission and no real credentials created for testing.

## Review Focus

- Spoofed app webDomain, browser signature or app asset-link must not receive a site's password/passkey.
- Process death, cancellation, stale pending intent and vault replacement must invalidate authorization.
- An already-open vault must not cause a wrong master password to count as verification.
- Browser clientDataHash, rotated app certificates and related origins must retain the actual caller context.
- Biometric enrollment changes and unsupported Android versions must offer master-password fallback safely.

## Shared interfaces

Native class `uk.thomast.myle.mobile.NativeVault` loads `myle_passwords_lib` and declares:

```kotlin
external fun call(dataDir: String, action: String, payload: String): String
```

`dataDir` is `context.applicationInfo.dataDir` (Tauri's app_data_dir), not filesDir. JSON replies are `{"ok":true,"result":...}` or `{"ok":false,"error":"..."}`. Kotlin must not expose this API to other apps or log payloads/results.

Request scope is JSON `{origin, site, packageName, certificate}`. `site` is the normalized, validated destination host/RP; `origin` comes from trusted framework caller information or verified Autofill context; `packageName` and SHA-256 `certificate` bind native app requests. Browser certificate validation and native app asset links happen in Kotlin before any metadata/secrets request. Rust additionally validates scope shape and all operation/request IDs.

- `status {}` -> `{hasVault}`; no secret/account metadata.
- `unlock {password,scope}` or `unlockKey {key,scope}` -> `{session}` after fresh cryptographic verification, even if the shared cell is already open.
- `list {session,scope,kind,request?}` -> array `{id,title,username,hasPassword,hasTotp,credentialId?}`. `kind` is `password` or `passkey`; passkey `request` is the JSON options and must enforce allow IDs.
- `fill {session,scope,id,fields}` -> `{username?,password?,totp?}`; `fields` contains only requested `username`, `password`, `totp`; consumes session.
- `create {session,scope,request,clientDataHash?}` -> WebAuthn registration JSON, consumes session; request is full JSON options.
- `get {session,scope,request,credentialId,clientDataHash?}` -> assertion JSON, consumes session and enforces allowed credential IDs.
- `save {session,scope,username,password}` -> `{id}`; consumes session and preserves an existing matching entry's passkeys/TOTP.
- `enrollKey {session,scope}` -> `{key,vaultId}` for the private biometric enrollment activity only, consumes session.
- `cancel {session}` -> null, invalidates session.

`scope` must compare exactly between unlock and later actions. Kotlin may acquire a separate session after re-verification when a selector transition changes request scope. No token goes to the destination app.

## Task 1: Shared vault backend and Android WebAuthn context

Files: create `backend/mobile/src/provider.rs`, `backend/mobile/src/android.rs`; modify `backend/mobile/src/lib.rs`, `backend/mobile/src/passwords.rs`, `backend/mobile/Cargo.toml`, `backend/Cargo.lock`, `backend/vault/src/passkeys.rs`.

- [x] Add failing unit tests for already-unlocked wrong-password rejection, scope mismatch, expiry, cancellation, token replay, unrelated-RP allow IDs and vault replacement.
- [x] Extend passkey creation/assertion context without changing desktop wrappers: native package client data and optional exact 32-byte clientDataHash; omit clientDataJSON when a browser supplies its hash.
- [x] Verify signatures with an independent verifier, altered hash/challenge/origin/UV rejection and native-origin JSON.
- [x] Implement shared cell, authorization tokens, scoped list/fill/save/create/get and JNI exception/panic boundaries. PasswordsState::new uses the same cell on Android.
- [x] Run `cargo test --manifest-path backend/Cargo.toml -p myle-vault -p myle-passwords` and relevant clippy checks; record output.

## Task 2: Native Android services and protected selection

Owner: Kotlin implementation agent, exclusively `backend/mobile/plugin/android/**`; root owns other paths.

Files: native `NativeVault.kt`, `CredentialScope.kt`, `MyleCredentialProviderService.kt`, `MyleAutofillService.kt`, `MyleCredentialActivity.kt`, `ProviderBiometry.kt`; XML services/provider configuration/themes; Gradle dependencies and Kotlin tests. Modify `MyleMobilePlugin.kt` only for setup/status/settings commands.

- [x] Write unit tests for field classification, exact app/browser identity, malicious domain claims and one-time scope selection; run them RED before fixes where possible.
- [x] Bind provider/autofill services with Android's dedicated permissions, disable unavailable APIs with versioned boolean resources, and use private secure activities/explicit unique PendingIntents.
- [x] Implement two-phase Credential Manager creation/get/locked authentication actions using AndroidX request/response APIs, all callbacks and cancellation outcomes.
- [x] Implement destination-scoped login selection and requested username/password/TOTP fields. Password save requires explicit user confirmation and fresh verification.
- [x] Add fingerprint enrollment/unlock using per-use Keystore AES-GCM CryptoObjects, with invalidation and master-password fallback. No standalone biometric prompt followed by freely accessible decryption.
- [x] Add `providerStatus`, `providerSettings`, `providerBiometry` commands to native plugin. Status reports SDK/API availability, enabled provider/autofill and fingerprint availability/enrollment. Setup activity obtains its own master-password session before enrolling.
- [ ] Native unit tests plus Android build are required; static source checks alone are insufficient.

## Task 3: In-app setup and compatibility routing

Files: mobile frontend provider setup component/state, mobile Rust/plugin setup command wrappers, desktop extension routing tests and implementation in a separate ownership domain.

- [x] Add Android-only setup with actual provider/autofill status and links to system settings, fingerprint setup and master fallback text; gracefully omit unavailable passkey provider on API <34.
- [x] Desktop unsupported authenticator attachment/PRF/largeBlob constraints route to original browser implementation. RP mismatch routes native without loosening vault scope checks. Denied iframe policy and cross-origin ancestor contexts stay native.
- [x] Test native routing and abort behavior with existing VM harness; keep supported MYLE responses unchanged.
- [x] Run Node suite, `npm run check` and mobile frontend build.

## Task 4: Build, review and release

- [ ] Run all relevant native/Rust/Node tests and full checks; build Android on the existing GitHub workflow without publishing an unverified patch.
- [x] Independently review caller binding, biometric key access, JNI lifecycle, pending-intent mutation and provider context integrity.
- [x] Resolve material findings with reproducing tests. Record OEM/live-site tests as pending if phones/account access are unavailable.
- [ ] Produce patch APK and extension ZIPs, bump patch version consistently, commit/push and publish after build checks pass, using the user's existing release authorization.

## Evidence ledger

- Baseline 10.0.4: 261 Node tests, 641 Rust tests (9 previously ignored), clippy and app/setup/mobile frontend builds passed. GitHub Release, CI and mobile workflows succeeded. Downloaded extension source/version and release SHA-256/update feed verified.
- Worktree base: `ae92c48`, branch `codex/android-credentials`. Device models supplied; installed OS versions and physical-device testing unavailable. Android capability checks will run on-device.
- Ruling: proceed with already-authorized implementation and review without an additional plan approval gate; no new external-maintainer communication or credential/account changes are authorized.
- Scoped verification: 10 provider authorization tests, 8 passkey/context tests and 4 independent SimpleWebAuthn RP scenarios passed. Native Kotlin compilation and 17 JUnit tests passed; fresh reproduction and Gradle/APK checks follow separately.
- Independent native review identified three capability/compatibility issues, now fixed with tests: TOTP-only entries advertised as passwords, forms whose selected login lacked a requested secret, and rejection of valid nondefault HTTPS ports. Structural caller equality and selected-entry option filtering were verified against pinned AndroidX/AOSP and required no change.
- Frontend verification: Svelte/TypeScript reported zero errors/warnings; desktop and setup production builds passed. Full workspace checks and actual Android build remain pending at this ledger point.
- Final host verification: 654 Rust tests passed (9 previously ignored), workspace/all-targets Clippy passed with `-D warnings`, all 310 Node tests passed, and mobile production frontend build passed. The first parallel Node attempt timed out waiting for Cargo's shared build lock; sequential rerun completed all 310 tests in 13 seconds without code changes.
- Fresh native reproduction compiled against the exact locked Tauri 2.11.6 and AndroidX 1.6.0 and passed all 17 JUnit tests. Consumer rules preserve the JNI symbol under R8. Independent final JNI/setup review found no concrete blockers; actual Android target/Gradle compilation remains the next gate before publishing.
