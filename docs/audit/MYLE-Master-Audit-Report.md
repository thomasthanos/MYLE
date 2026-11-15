# MYLE — Master Engineering Report (Bug, Security, Architecture & Performance Audit)

**Ημερομηνία:** 2026-11-14 · **Commit:** `0a4af9a` (main, καθαρό working tree) · **Έκδοση app:** 11.0.1 · **Extension:** 11.1.0

## Μεθοδολογία

- 8 subagents δούλεψαν παράλληλα, μόνο με ανάγνωση: κανένα αρχείο δεν άλλαξε και δεν έτρεξε κανένα `cargo build/test`. Επτά κάλυψαν από ένα κομμάτι του repo και ο όγδοος ήλεγξε τα σημεία όπου συνδέονται τα κομμάτια (IPC, capabilities, extension↔app, Worker/Supabase, elevated helpers, installer↔app).
- Κάθε εύρημα έχει ετικέτα επαλήθευσης:
  - **✔** το επαλήθευσα ο ίδιος στον κώδικα, μετά τον subagent·
  - **C** CONFIRMED: ο subagent ακολούθησε όλη τη διαδρομή του κώδικα·
  - **P** PLAUSIBLE: πιθανό, αλλά εξαρτάται από κάτι που δεν ήταν δυνατό να επαληθευτεί στατικά (αναφέρεται τι ακριβώς).
- Ό,τι εκτελέστηκε:
  - `node --test scripts/tests/*.test.mjs`: **526/526 πέρασαν** (42 αρχεία, Node 26· το CI τρέχει Node 22)·
  - μια διασταύρωση IPC με script: 287 κλήσεις `invoke()` έναντι 282 `#[tauri::command]`·
  - μια μέτρηση μήκους command line σε PowerShell (για το BUG-04).
- Κανένα εύρημα δεν χαρακτηρίστηκε **CRITICAL**: δεν βρέθηκε απομακρυσμένη εκτέλεση κώδικα ούτε απώλεια δεδομένων που συμβαίνει χωρίς καμία ενέργεια του χρήστη. Τα P0/P1 είναι όλα **HIGH**.
- Ένας παράγοντας ενισχύει πολλά ευρήματα: το `backend/Cargo.toml:108` ορίζει `panic = "abort"` στο release profile. Κάθε panic, μέσα σε command, σε background thread ή στο installer, **σκοτώνει όλο το process**. Αυτό περιλαμβάνει tray, scheduled backups και installer rollback. Γι' αυτό τα «απλά» slice panics πιο κάτω μετρούν ως crash bugs.

---

## 1. Executive System Architecture Overview

### 1.1 Αρχιτεκτονικός χάρτης

```
                 ┌──────── Installer (myle-setup, Tauri window "setup") ─────────┐
                 │ HKCU Uninstall + HKCU\Software\ThomasThanos\MakeYourLifeEasier │
                 │ .lnk + AppUserModelID (shared shell_identity.rs via #[path])   │
                 │ XZ payload → .myle-new/.myle-old transaction → commit/rollback │
                 └──────────▲─────────────────────────────────────────────────────┘
       /S /UPDATE /LIVE  or /P /UPDATE /R  (χωρίς /D=)
                            │
 R2 latest.json / GitHub ──► Updater (updater.rs: SHA-256 από το feed, host allow-list,
                            │            μόνο το asset που προσφέρθηκε, no downgrade)
┌──────────────────── Desktop process MYLE.exe (Tauri 2.11.6) ─────────────────────────┐
│ Svelte 5 UI (splash / main / game-saves-notice, tauri://localhost, CSP 'self')        │
│   │ invoke: 272 commands (+ owner gate σε github_releases_/project_backups_/steam_)   │
│   ▲ emit: 16 events · Channel<T>: 51 streams                                          │
│ Rust command layer ── myle_vault crate (vault, crypto, sync, sites, totp, passkeys)   │
│   ├─ Steam webviews (remote https· IPC κλειστό μόνο λόγω `!is_local` του Tauri)        │
│   ├─ Elevated helpers: PowerShell Start-Process -Verb RunAs → ίδιο exe               │
│   │    --{cleaner|disk-analyzer|debloat}-elevated-helper · pipe \\.\pipe\myle-<uuid>  │
│   │    JSON lines, ΧΩΡΙΣ version· Auto-Logon: ShellExecuteExW + locked request file   │
│   └─ Browser bridge: \\.\pipe\myle-passwords-<hash>, reject_remote_clients,          │
│        έλεγχος ίδιου exe path, rate limits 120/20 ανά λεπτό                           │
└───────────────────────────────────────────────────────────────────────────────────────┘
     ▲ named pipe                                 │ HTTPS: anon apikey + user JWT
MYLE.exe σε native-host mode ◄── stdio (4-byte len) │ PostgREST: password_vault, password_items
     ▲                                             │ (RLS own rows + forward-only revision trigger),
Browser extension (MV3/Firefox): passkeys.js(MAIN) │ user_settings (DDL/RLS ΔΕΝ υπάρχουν στο repo)
 → content.js(isolated) → background.js            ▼
 sendNativeMessage("com.thomasthanos.myle")     Supabase ◄── Cloudflare Worker (auth-worker.js):
                                                             μόνο /authorize → 302 με allow-list params
Mobile app (myle-passwords, Android/iOS) ── ίδιο myle_vault::sync ──► Supabase
 deep link uk.thomast.myle.passwords://auth-callback + PKCE· biometry/clipboard/barcode plugins
```

**Ροές και αυθεντικοποίηση ανά ακμή**

| Ακμή | Μεταφορά | Αυθεντικοποίηση | Αδύναμο σημείο |
|---|---|---|---|
| UI → Rust | Tauri IPC | invoke key· κάθε τοπικό window παίρνει **όλα** τα commands (δεν υπάρχει AppManifest) | INS-22 |
| Extension → native host | stdio | extension ID από argv + όνομα αρχείου του browser process | VLT-04 (spoofable) |
| Native host → app | named pipe | και οι δύο πλευρές ελέγχουν ότι τρέχει το ίδιο exe path | δεν κόβει same-user processes |
| App → elevated helper | named pipe | το app δημιουργεί πρώτο το pipe (first instance) + έλεγχος exe/PID | default DACL, κανένας έλεγχος elevation (WIN-04) |
| App/Mobile → Supabase | HTTPS | user JWT + RLS | header χωρίς αυθεντικοποίηση (SEC-03) |
| Updater → installer | τοπικό spawn | SHA-256 από το ίδιο κανάλι με το URL | **καμία υπογραφή** (SEC-02) |

### 1.2 Health & Maturity Scorecard (1–10)

| Υποσύστημα | Security | Stability | Performance | Code Quality | Test Coverage |
|---|---|---|---|---|---|
| Vault crate (`backend/vault`) | 7 | 7 | 7 | 8 | 8 |
| Passwords desktop (`backend/src/passwords`) | 7 | 7 | 6 | 7 | 7 |
| Account / OAuth / Sync | 8 | 7 | 8 | 8 | 7 |
| Browser extension | 8 | 7 | 6 | 7 | 7 |
| Worker + Supabase SQL | 8 | 6 | 8 | 7 | 3 |
| Elevated pipe | 8 | 8 | 8 | 8 | 6 |
| Disk Analyzer (backend) | 7 | 8 | 7 | 8 | 8 |
| Cleaner + Software Health | 6 | 5 | 6 | 6 | 6 |
| Debloat | 8 | 6 | 7 | 7 | 7 |
| Windows Optimization / Auto-Logon | 9 | 9 | 9 | 8 | 8 |
| Maintenance | 6 | 4 | 8 | 7 | 6 |
| Cloud / online-only | 9 | 8 | 5 | 8 | 8 |
| Steam Automation | 8 | 7 | 7 | 8 | 8 |
| Steam Helper | 9 | 8 | 8 | 8 | 8 |
| Game Saves | 8 | 5 | 5 | 6 | 7 |
| Project Backups | 8 | 5 | 7 | 8 | 8 |
| Auto Clicker | 9 | 8 | 7 | 9 | 8 |
| App shell / lifecycle | 8 | 7 | 7 | 7 | 6 |
| WebView management | 8 | 8 | 7 | 8 | 6 |
| Installer | 7 | 7 | 8 | 8 | 8 |
| Updater | 5 | 8 | 8 | 8 | 7 |
| Install Apps (winget/custom/creative) | 6 | 6 | 7 | 6 | 6 |
| GitHub Releases | 7 | 8 | 7 | 7 | 6 |
| Spotify Hub | 6 | 7 | 7 | 7 | 6 |
| Mobile Rust backend | 6 | 7 | 7 | 7 | 5 |
| Android plugin | 5 | 7 | 8 | 7 | 2 |
| iOS plugin | 6 | 6 | 9 | 7 | 1 |
| Mobile frontend | 6 | 7 | 8 | 7 | 5 |
| Desktop frontend: shell & lib | 9 | 8 | 8 | 7 | 7 |
| Password Manager UI | 8 | 5 | 6 | 8 | 5 |
| Disk Analyzer UI | 9 | 8 | 8 | 8 | 7 |
| Υπόλοιπες σελίδες (σύνολο) | 8 | 6 | 7 | 6 | 6 |
| Installer / Splash / Notice UI | 8 | 7 | 8 | 8 | 5 |

| Build & Delivery | Βαθμός | Αιτιολόγηση |
|---|---|---|
| CI workflows | 6 | `contents: read`, χωρίς secrets, πραγματικό smoke test· αλλά actions pinned σε tag, unpinned toolchain, χωρίς `timeout-minutes` |
| Release / signing pipeline | 4 | PFX και password ορατά σε όλο το build, unsigned release με απλό warning, per-tag concurrency race |
| Mobile feeds | 6 | versioned ονόματα και έλεγχος hash, αλλά το `update-sidestore.mjs` έχει ήδη αποκλίνει |
| JS test suite | 7 | 526 tests που τρέχουν πραγματικό κώδικα· λίγα κάνουν μόνο regex πάνω στον πηγαίο κώδικα |
| Rust test coverage | 6 | ~1.248 `#[test]`· λίγα σε vault crypto/sync, mobile unlock, clipboard/session, elevated pipe |
| Build config | 7 | LTO, strip, native-tls μόνο, χωρίς sourcemaps· αλλά `panic="abort"` και διπλά fonts |
| Site | 8 | αυστηρό CSP, `textContent` παντού· λείπει HSTS |

**Συνολική εικόνα.** Ο κώδικας είναι αισθητά πάνω από τον μέσο όρο. Πολλά από τα πιο δύσκολα κομμάτια είναι υποδειγματικά:

- ο elevated cleaner διαγράφει μέσω file ID και handle·
- το Auto-Logon αποθηκεύει τον κωδικό ως LSA secret·
- το Project Backups ελέγχει κάθε backup διπλά·
- το extension αμύνεται σε clickjacking·
- το IPC contract συμφωνεί σε όλα τα arguments και events.

Τα προβλήματα συγκεντρώνονται σε τρία σημεία:
1. **Εμπιστοσύνη στην αλυσίδα παράδοσης:** signing secrets και updater χωρίς υπογραφή.
2. **Επιθέσεις στα όρια:** header του vault από το cloud, native bridge, κλειδώματα στο mobile.
3. **Edge cases που γίνονται crash, λόγω του `panic="abort"`.**

---

## 2. Critical & High-Severity Bugs & Security Vulnerabilities (P0 / P1)

### SEC-01 [HIGH] Τα code-signing secrets είναι ορατά σε όλο το release build · ✔
- **Location:**
  - `.github/workflows/release.yml:84-101` (Prepare code signing), `:106-113` (Build)
  - επίσης `:43`, `:50`, `:136-142`
- **Root Cause Analysis:**
  - Το βήμα «Prepare code signing» αποκωδικοποιεί το `.pfx` στο `$RUNNER_TEMP\myle-sign.pfx`, το οποίο δεν διαγράφεται ποτέ.
  - Γράφει επίσης `MYLE_SIGN_PFX` και `MYLE_SIGN_PASSWORD` στο `$GITHUB_ENV`.
  - Από εκεί και πέρα τα διαβάζει **κάθε** επόμενο βήμα: το `npm run build` (Vite, όλα τα npm packages, κάθε `build.rs` και proc-macro του Cargo, `tauri build`), το `actions/upload-artifact@v4` και τα post-job steps των `setup-node@v4` και `Swatinem/rust-cache@v2`.
  - **Πώς ενεργοποιείται:** αρκεί ένα compromised transitive dependency ή ένα action tag που μετακινήθηκε (τα actions είναι pinned σε tag και όχι σε SHA, βλ. CI-05). Διαβάζει `env:MYLE_SIGN_PASSWORD` και το PFX, και από εκεί μπορεί να υπογράψει malware ως «ThomasThanos».
- **Concrete Fix:** build χωρίς secrets και υπογραφή σε σύντομα βήματα που κρατούν το secret μόνο όσο τρέχουν. Χρειάζεται ένας διακόπτης `-Phase` στο `scripts/build-setup.ps1`, που χωρίζει το build στη γραμμή ~110.
```yaml
- name: Build app + uninstaller (no secrets)
  run: ./scripts/build-setup.ps1 -Phase binaries
- name: Sign app + uninstaller
  if: env.HAS_CERT == 'true'
  shell: pwsh
  env: { CERT_B64: "${{ secrets.WIN_CSC_LINK }}", CERT_PASS: "${{ secrets.WIN_CSC_KEY_PASSWORD }}" }
  run: |
    $pfx = Join-Path $env:RUNNER_TEMP "s.pfx"
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:CERT_B64.Trim()))
    try {
      $env:MYLE_SIGN_PFX = $pfx; $env:MYLE_SIGN_PASSWORD = $env:CERT_PASS
      'MYLE.exe','uninstall.exe' | % { ./scripts/sign.ps1 -Path "backend/target/release/$_" }
    } finally { Remove-Item $pfx -Force }
- name: Pack + setup (no secrets)
  run: ./scripts/build-setup.ps1 -Phase setup
# …ένα ακόμα sign step με το ίδιο pattern για το bundle/setup/MYLE.exe
```
Επίσης: τα secrets του πιστοποιητικού και του R2 να μπουν σε GitHub `environment:` με required reviewers, και το `sign.ps1:36` να μη βάζει το password στο command line του signtool (βλ. CI-14).

### SEC-02 [HIGH] Τα updates δεν είναι αυθεντικοποιημένα· το release βγαίνει unsigned με απλό warning · ✔
- **Location:**
  - `backend/src/updater.rs:261-299`, `:550-578`, `:46` (`DOWNLOAD_PREFIXES`)
  - `.github/workflows/release.yml:39`, `:106-113`, `:129-133`, `:181-195`
- **Root Cause Analysis:**
  - Το SHA-256 που ελέγχει ο updater (`updater.rs:562`) έρχεται από το ίδιο κανάλι με το URL: το `latest.json` του ίδιου R2 bucket ή το asset digest του GitHub. Δεν υπάρχει `WinVerifyTrust`, minisign ή Ed25519 πουθενά στο `backend/` (επαληθεύτηκε με grep).
  - Όποιος αποκτήσει τα R2 credentials, π.χ. μέσω SEC-01 ή ενός workflow σε άλλο branch, αφού κανένα workflow δεν έχει `environment:`, ανεβάζει `MYLE.exe` και ένα `latest.json` που ταιριάζουν. Κάθε client το εγκαθιστά σιωπηλά (`/S /UPDATE /LIVE`).
  - Το `DOWNLOAD_PREFIXES` δέχεται **οποιοδήποτε** `https://github.com/`, όχι μόνο `thomasthanos/MYLE`.
  - Αν το secret του πιστοποιητικού λήξει ή χαθεί, το `release.yml:110` απλώς τυπώνει `::warning::` και δημοσιεύει unsigned setup.
  - Ο έλεγχος στο `:129-133` κοιτάζει μόνο αν **υπάρχει** `SignerCertificate`, όχι `Status == Valid`, thumbprint ή timestamp.
- **Concrete Fix:**
```yaml
# release.yml — πριν από κάθε upload
- name: Refuse unsigned releases
  if: github.ref_type == 'tag' && env.HAS_CERT != 'true'
  run: echo "::error::No signing certificate; refusing to publish"; exit 1
# Collect the installer:
#  if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate -or
#      $signature.SignerCertificate.Thumbprint -ne $env:EXPECTED_THUMBPRINT) { throw "bad signature" }
```
```rust
// updater.rs — offline Ed25519 υπογραφή του "MYLE {version} {sha256}" (Cargo: ed25519-dalek = { version = "2", default-features = false })
const UPDATE_KEY: [u8; 32] = *include_bytes!("../keys/update-ed25519.pub");
const DOWNLOAD_PREFIXES: &[&str] = &[
    "https://downloads.thomast.uk/",
    "https://github.com/thomasthanos/MYLE/releases/download/",
];
fn verify_signature(version: &str, sha256: &str, sig_b64: &str) -> Result<(), String> {
    use base64::Engine; use ed25519_dalek::{Signature, VerifyingKey};
    let raw = base64::engine::general_purpose::STANDARD.decode(sig_b64).map_err(|_| "bad update signature")?;
    let sig = Signature::from_slice(&raw).map_err(|_| "bad update signature")?;
    VerifyingKey::from_bytes(&UPDATE_KEY).map_err(|e| e.to_string())?
        .verify_strict(format!("MYLE {version} {sha256}").as_bytes(), &sig)
        .map_err(|_| "The update is not signed with MYLE's release key.".into())
}
// install_update(), αμέσως μετά το `actual == expected`:
// verify_signature(&asset.version, &actual, asset.signature.as_deref().ok_or("Unsigned update.")?)?;
```
Εναλλακτικά, ή και επιπλέον: `WinVerifyTrust` στο κατεβασμένο setup με pinned subject/thumbprint πριν από το spawn. Το ίδιο κενό υπάρχει και στο mobile (MOB-02).

### SEC-03 [HIGH] Το header του vault από το Supabase γίνεται δεκτό χωρίς αυθεντικοποίηση, και οι παράμετροι Argon2 δεν έχουν όρια · ✔
- **Location:**
  - `backend/vault/src/vault.rs:1100-1107` (`take_header`), καλείται από `backend/vault/src/sync.rs:205-206` και `:183-200` (`adopt`)
  - `backend/vault/src/crypto.rs:96-121` (`derive`)
  - `backend/vault/src/import.rs:243-248` (backup import)
- **Root Cause Analysis:**
  - Η `take_header` αντικαθιστά τα `kdf`, `wrapped_key` και `recovery_wrapped_key` με ό,τι γράφει η γραμμή `password_vault`. Η μόνη συνθήκη είναι το `vault_id` να ταιριάζει, και αυτό διαβάζεται από την ίδια γραμμή.
  - Αυτό έρχεται σε αντίθεση με το threat model του ίδιου του κώδικα (`vault.rs:26-27`, `:1137-1143`: «Nothing from the account is trusted»).
  - Όποιος κατέχει το Supabase session του χρήστη (hijacked Google/Discord login, κλεμμένο refresh token, compromised project) μπορεί:
    1. να γράψει σκουπίδια στα wraps: σε κάθε sync όλες οι συσκευές τα γράφουν τοπικά, και ούτε ο master password ούτε ο recovery code λειτουργούν πια. Χωρίς ενεργό Windows Hello, **το vault χάνεται οριστικά**·
    2. να βάλει `iterations: 4294967295` ή `memory: 4294967295`: το `Params::new` το δέχεται, οπότε το unlock κολλάει **ενώ κρατά το `VaultCell` mutex** (παγώνουν browser filling και sync) ή κάνει abort σε αποτυχία allocation.
  - Το ίδιο κενό ορίων υπάρχει και για ένα κακόβουλο αρχείο `.myle-vault` κατά το import.
- **Concrete Fix:**
```rust
// crypto.rs
pub fn check_kdf(p: &KdfParams) -> Result<(), String> {
    if p.algorithm != "argon2id"
        || !(8 * 1024..=1024 * 1024).contains(&p.memory)   // KiB: 8 MiB..1 GiB
        || !(1..=10).contains(&p.iterations)
        || !(1..=8).contains(&p.parallelism) {
        return Err("This vault's key settings are out of range.".into());
    }
    Ok(())
}
pub fn derive(password: &str, params: &KdfParams) -> Result<Key, String> {
    check_kdf(params)?;
    /* …όπως σήμερα… */
}

// vault.rs — το header σφραγίζεται με το vault key στα create()/rewrap()
fn header_aad(h: &RemoteHeader) -> String {
    format!("myle-vault|header|{}|{}|{}|{}", h.vault_id,
        serde_json::to_string(&h.kdf).unwrap_or_default(), h.wrapped_key, h.recovery_wrapped_key)
}
pub fn take_header(&mut self, header: RemoteHeader) -> Result<(), String> {
    crypto::check_kdf(&header.kdf)?;
    let Some(u) = self.unlocked.as_ref() else {
        self.pending_header = Some(header);  // ελέγχεται μετά το unlock· δεν αντικαθιστά ποτέ το τοπικό
        return Ok(());
    };
    crypto::open(&u.key, &header_aad(&header), &header.seal)
        .map_err(|_| "The account's vault header was not made with this vault's key.")?;
    let file = self.file.as_mut().ok_or("There is no vault yet.")?;
    file.kdf = header.kdf;
    file.wrapped_key = header.wrapped_key;
    file.recovery_wrapped_key = header.recovery_wrapped_key;
    file.header_dirty = false;
    self.write()
}
```
Χρειάζεται μια νέα στήλη `header_seal` στο `password_vault`, συν migration: τα headers χωρίς seal γίνονται δεκτά μόνο μία φορά και σφραγίζονται στο επόμενο unlock. Επιπλέον `check_kdf` στο `import.rs` πριν από το `derive`.

### SEC-04 [HIGH] Το auto-lock του mobile μετρά μόνο τον χρόνο που η συσκευή είναι ξύπνια· το vault μένει ανοιχτό μετά από ώρες ύπνου · ✔ (κώδικας) / C (συμπεριφορά του clock)
- **Location:**
  - `backend/mobile/src/passwords.rs:38` (`hidden_at: Instant`), `:86-98` (`lock_now_if_away`), `:131-132` (idle loop), `:156-161`
  - `backend/vault/src/vault.rs:280` (`last_used: Instant`), `:496`, `:507`
- **Root Cause Analysis:**
  - Το `std::time::Instant` διαβάζει `CLOCK_MONOTONIC` στο Android/Linux και `CLOCK_UPTIME_RAW` στο iOS. **Κανένα από τα δύο δεν προχωρά όσο η συσκευή είναι σε suspend.**
  - Παράδειγμα: auto-lock ρυθμισμένο στα 5 λεπτά, ο χρήστης κάνει unlock και πατά power, και το κινητό μένει 8 ώρες στην τσέπη. Από αυτές μετρούν μόνο λίγα δευτερόλεπτα ξύπνιας λειτουργίας.
  - Κατά την επιστροφή το `left.elapsed() < AWAY_LOCK`, οπότε το vault παραμένει ανοιχτό. Αρκεί να ξεκλειδώσει κάποιος το κινητό (γνωστό PIN, Smart Lock, δανεικό κινητό). Το αποκρυπτογραφημένο vault μένει στη μνήμη όλο αυτό το διάστημα.
- **Concrete Fix:**
```rust
use std::time::{Instant, SystemTime};
// struct PasswordsState
hidden_at: Arc<Mutex<Option<(Instant, SystemTime)>>>,

fn lock_now_if_away(&self) -> bool {
    let Some((mono, wall)) = self.hidden_since().take() else { return false };
    // Το Instant σταματά όσο το κινητό κοιμάται· ο wall clock όχι. Clock που πήγε πίσω = «πολύ καιρό μακριά».
    let away = mono.elapsed().max(SystemTime::now().duration_since(wall).unwrap_or(AWAY_LOCK));
    if away < AWAY_LOCK { return false; }
    let mut slot = self.cell.lock();
    let Some(vault) = slot.as_mut() else { return false };
    if vault.status() != Status::Unlocked || vault.prefs().auto_lock_minutes == 0 { return false; }
    vault.lock();
    true
}
// passwords_app_hidden:
state.hidden_since().get_or_insert_with(|| (Instant::now(), SystemTime::now()));
```
Το ίδιο χρειάζεται και στο `vault.last_used`: κρατήστε επίσης ένα `last_used_wall: SystemTime` και στο idle loop χρησιμοποιήστε το `max` των δύο.

### BUG-01 [HIGH] Το `strip_zip` κόβει UTF-8 στη μέση: ένα όνομα όπως `App_αρχείο.7z` ρίχνει το MYLE, ακόμη και σε loop · ✔
- **Location:**
  - `backend/src/project_backups/naming.rs:95-101`
  - καλείται από `naming.rs:69` (`parse_backup_name`), μέσω του `next_version` (`:137`) σε κάθε backup
  - επίσης από `engine.rs:276` (`list_backups_checked`), που τρέχει στο background change-check thread (`changes.rs:214`) και στο Restore/list
- **Root Cause Analysis:**
  - Το `&name[name.len() - 4..]` κόβει ένα `&str` χωρίς έλεγχο `is_char_boundary`. Το `belongs_to` (`:109`) κάνει αυτόν τον έλεγχο, το `strip_zip` όχι.
  - Ένα όνομα σε φάκελο μήνα που αρχίζει με `<App>_` και τελειώνει σε ελληνικό χαρακτήρα ακολουθούμενο από 1 ή 3 ASCII χαρακτήρες (`App_σημειώσεις.md`, `App_αρχείο.7z`, `App_παλιό_V3`) προκαλεί panic, και με `panic="abort"` αυτό είναι **abort**.
  - Αν το όνομα είναι φάκελος, το change-check thread το ξαναβρίσκει σε κάθε έλεγχο, οπότε η εφαρμογή κρασάρει ξανά λίγα δευτερόλεπτα μετά από κάθε εκκίνηση.
  - Πιθανότητα: υψηλή για ελληνόφωνους χρήστες.
- **Concrete Fix:**
```rust
pub(crate) fn strip_zip(name: &str) -> &str {
    match name.len().checked_sub(4).filter(|&at| at > 0 && name.is_char_boundary(at)) {
        Some(at) if name[at..].eq_ignore_ascii_case(".zip") => &name[..at],
        _ => name,
    }
}
```
Προσθέστε unit test με `"App_αρχείο.7z"`, `"App_παλιό_V3"` και `"App_σημειώσεις.md"`.

### BUG-02 [HIGH] Το Restore του Project Backups επαναφέρει σιωπηλά παλιότερη κατάσταση όταν ένα increment δεν διαβάζεται · ✔
- **Location:**
  - `backend/src/project_backups/incremental.rs:483-491` (`restore`)
  - `engine.rs:232-234` (`list_backups` = `unwrap_or_default`)
  - `incremental.rs:179-197` (`chain`, που σταματά στο πρώτο μη αναγνώσιμο increment)
- **Root Cause Analysis:**
  - Το Restore χρησιμοποιεί τις «επιεικείς» `engine::list_backups` και `chain()`. Αντίθετα, το change detection χρησιμοποιεί σκόπιμα τις `list_backups_checked` και `checked_chain`, και το σχόλιο στο `incremental.rs:198-199` εξηγεί ακριβώς γιατί.
  - Παράδειγμα: το increment V8-3 από 5 είναι online-only στο Google Drive και το Drive δεν τρέχει, ή το αρχείο είναι κλειδωμένο. Το restore ξαναχτίζει το project όπως ήταν στο V8-2, όλα τα CRC περνούν και αναφέρει **επιτυχία**. Η δουλειά των increments 3–5 λείπει χωρίς καμία προειδοποίηση.
  - Το ίδιο συμβαίνει αν αποτύχει το listing ενός φακέλου μήνα: επιλέγεται παλιότερο full backup.
- **Concrete Fix:**
```rust
pub(crate) fn restore(app_dir: &Path, app_name: &str, target: &Path, cancelled: &dyn Fn() -> bool)
    -> Result<Restored, String> {
    let base = engine::list_backups_checked(app_dir, app_name)?
        .into_iter().next().ok_or("This project has no backup yet.")?;
    if base.broken {
        return Err(format!("The newest backup ({}) is incomplete; nothing was restored.", base.name));
    }
    if base.kind != "zip" {
        return Err("The newest backup is a folder: copy it from the backups folder instead.".into());
    }
    let base_path = engine::resolve_backup(app_dir, app_name, &base.id)?;
    let chain = checked_chain(app_dir, &base)?; // ένα μη αναγνώσιμο increment είναι σφάλμα, όχι μικρότερη αλυσίδα
    /* …υπόλοιπο όπως σήμερα… */
}
```
Μακροπρόθεσμα: το `engine::list_backups` να γίνει `#[cfg(test)]` ή να μετονομαστεί, ώστε να μην ξαναχρησιμοποιηθεί σε μονοπάτι που επηρεάζει δεδομένα.

### BUG-03 [HIGH] Το «recycle» του Software Health μπορεί να διαγράψει οριστικά χωρίς καμία ειδοποίηση · ✔
- **Location:**
  - `backend/src/cleaner/health/sys.rs:465-494` (`recycle`)
  - callers: `cleaner/health/mod.rs:912-928` και `:944-958`
- **Root Cause Analysis:**
  - Καλείται `SHFileOperationW(FO_DELETE, FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT)`.
  - Όταν ένα αντικείμενο **δεν μπορεί** να πάει στον Κάδο (μεγαλύτερο από το quota, Κάδος απενεργοποιημένος στον δίσκο, πολύ μακρύ path, volume χωρίς Κάδο), το shell ρωτά «permanently delete?». Το `FOF_NOCONFIRMATION` απαντά αυτόματα «ναι» και δεν υπάρχει `FOF_WANTNUKEWARNING`.
  - Το αντικείμενο καταστρέφεται και αναφέρεται επιτυχία. Το journal μάλιστα γράφει `Undo::Recycled` («επαναφορά από τον Κάδο»), που είναι ψευδές.
  - Παράδειγμα: ένας φάκελος «leftovers» 30 GB σε ProgramData ή AppData, ή οποιοδήποτε PC με «Don't move files to the Recycle Bin».
  - Αντίθετα, το `disk_analyzer/delete.rs:110-117` προστατεύει ήδη ακριβώς αυτή την περίπτωση με progress sink του `IFileOperation`.
- **Concrete Fix:** επαναχρησιμοποίηση της προστατευμένης υλοποίησης· το `disk_analyzer::delete` γίνεται `pub(crate)`:
```rust
// cleaner/health/sys.rs
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    let _com = crate::disk_analyzer::delete::Com::init();
    for path in paths {
        if !path.is_absolute() || path.parent().is_none() {
            return Err(format!("Refused: {}", path.display()));
        }
        // Ακυρώνει στο PreDeleteItem αντί να διαγράψει οριστικά (επιστρέφει NO_BIN).
        crate::disk_analyzer::delete::recycle(path, None)?;
    }
    Ok(())
}
// health/mod.rs: κλήση μέσα σε tauri::async_runtime::spawn_blocking (STA COM),
// και remember(Undo::Recycled) ΜΟΝΟ μετά την επιτυχία (βλ. DSK-06).
```

### BUG-04 [HIGH] Το Check Disk δεν ξεκινά ποτέ: το elevated command line ξεπερνά το όριο των Windows · ✔ (διαδρομή) / C (μέτρηση)
- **Location:**
  - `backend/src/maintenance/runner.rs:96-109` (`run_disk_scan` → `watch(.., true)`)
  - `:170-172` (`elevate`), `:291-298` (`disk_script`)
  - `backend/src/apps/process.rs:34-64` (`run_elevated`, `-EncodedCommand`)
- **Root Cause Analysis:**
  - Το `disk_scan.ps1` (1.491 χαρακτήρες) κωδικοποιείται ως base64 UTF-16 (περίπου 2,67×) σε τρία φωλιασμένα επίπεδα: cmd tail → `Start-Process -ArgumentList` → εξωτερικό `-EncodedCommand`.
  - Μετρημένα μήκη: inner script 5.021 χαρακτήρες → elevated command line 13.503 → wrapper 13.650 → **εξωτερικό command line 36.527**. Το όριο του `CreateProcessW` είναι 32.767.
  - Αποτέλεσμα: `os error 206` πριν καν εμφανιστεί το UAC, **κάθε φορά**.
  - Τα tests δεν το εντοπίζουν επειδή καλούν `watch(.., with_uac=false)`, που παρακάμπτει ακριβώς το εξωτερικό επίπεδο.
- **Concrete Fix:** ο μη-elevated wrapper διαβάζεται από stdin αντί για το command line:
```rust
// apps/process.rs — run_elevated (και αντίστοιχα το start_elevated)
use tokio::io::AsyncWriteExt;
let mut child = hidden(powershell())
    .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
    .stdin(std::process::Stdio::piped())
    .spawn().map_err(|e| e.to_string())?;
let mut stdin = child.stdin.take().ok_or("PowerShell has no stdin")?;
stdin.write_all(format!("{script}\n").as_bytes()).await.map_err(|e| e.to_string())?;
drop(stdin); // EOF
let status = child.wait().await.map_err(|e| e.to_string())?;
Ok(status.code().unwrap_or(-1))
```
Επαληθεύστε ότι το `exit N` ορίζει ακόμη το exit code με `-Command -`. Προσθέστε επίσης ένα test που ελέγχει ότι το command line **κάθε** maintenance action είναι κάτω από 32.000 χαρακτήρες.

### BUG-05 [HIGH] Το Password Manager κρασάρει μόνιμα όταν μια εγγραφή έχει δύο φορές το ίδιο website · ✔
- **Location:**
  - `frontend/app/pages/password-manager/EntryView.svelte:126` (`{#each entry.urls as url (url)}`)
  - `EntryEditor.svelte:184` (καμία αφαίρεση διπλοτύπων)
  - `backend/vault/src/vault.rs:1413-1419` (`clean_list`, επίσης καμία αφαίρεση διπλοτύπων)
- **Root Cause Analysis:**
  - Στο Svelte 5.57.1 ένα keyed `{#each}` με διπλό key πετάει `each_key_duplicate` **και σε production build** (`node_modules/svelte/src/internal/client/dom/blocks/each.js:356-361`).
  - Παράδειγμα: New login → `github.com` → «Another website» → `github.com` → Save. Ακολουθεί panel view, το `each_key_duplicate` πετιέται και εμφανίζεται το PageBoundary.
  - Επειδή το `passwords.panel` είναι module-level state, το «Try again» και κάθε επιστροφή στη σελίδα κρασάρουν ξανά. Η μόνη διέξοδος είναι να κλειδώσει το vault ή restart.
  - Η ίδια εγγραφή μπορεί να έρθει και από sync, import ή το extension, και το mobile χρησιμοποιεί το ίδιο component.
- **Concrete Fix:**
```svelte
<!-- EntryView.svelte:126 -->
{#each entry.urls as url, i (i)}
```
```ts
// EntryEditor.svelte:184 (και στη γραμμή 60, στον έλεγχο dirty)
urls: [...new Set(urls.map((u) => u.trim()).filter(Boolean))],
```
```rust
// vault.rs — clean_list
fn clean_list(values: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values.iter().map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && seen.insert(v.to_lowercase()))
        .collect()
}
```
Γενικός κανόνας (UI-15): τα keys σε `{#each}` να είναι id ή index, ποτέ κείμενο που βλέπει ο χρήστης ή που έρχεται από έξω.

---
## 3. Medium & Low-Severity Bugs, Edge Cases & Logic Flaws (P2 / P3)

Στην παρένθεση κάθε εύρημα έχει το αρχικό ID του subagent, για ιχνηλασιμότητα.

### 3.1 Vault / Passwords / Extension

**VLT-01 [MEDIUM] Οποιαδήποτε HTTPS σελίδα, χωρίς user gesture, ξυπνά το MYLE, ανοίγει Windows Hello και μετά το unlock κάνει sign-in με το μοναδικό passkey χωρίς κλικ** (S1-02) · C
- **Location:** η αλυσίδα περνά από τα εξής σημεία:
  1. `extension/passkeys.js:387`
  2. `extension/content.js:1679` → `:1342` (`wake: true`) → `:1378-1381` (αυτόματο `open` + `carryOnWhenUnlocked`)
  3. `extension/background.js:736`
  4. `backend/src/passwords/browser.rs:526-531`, `:494-497` (εκκίνηση του MYLE), `:1519-1524` (`ask_to_unlock`)
  5. `frontend/.../password-manager/state.svelte.ts:224-231` → `UnlockVault.svelte:55-78` (αυτόματο `unlockWithHello`)
  6. `content.js:1584-1597` (sign-in χωρίς κλικ μετά το unlock)
  7. `browser.rs:1631-1633` (με UV `discouraged` δεν βγαίνει δεύτερο Hello)
- **Throttling:** μόνο 4 s (`browser.rs:1718`) και 30 ανά λεπτό ανά tab (`background.js:559`).
- **Επίπτωση:**
  - Focus stealing και επαναλαμβανόμενα prompts από κάθε σελίδα.
  - Ένα subdomain (`evil.example.com`) παίρνει assertion για το `example.com` από χρήστη που νόμιζε ότι απλώς ξεκλείδωνε το MYLE.
- **Fix:**
```js
// content.js passkeysHere()
const activated = navigator.userActivation?.isActive === true;
const listed = await send({ type: "passkeyList", rpId: options.rpId, allow: options.allow ?? [], wake: activated });
if (activated && (!closed || listed?.known)) {
  if (closed || locked) void send({ type: "open" });
  waiting = true; void carryOnWhenUnlocked(id, text, retry);
} // αλλιώς ξεκινά μόνο από το κουμπί «Unlock in MYLE» του prompt (genuine click)
// offerSignIn(): αυτόματο sign-in μόνο αν το unlock ξεκίνησε από αυτό το prompt
if (afterUnlock && startedHere && listed.passkeys.length === 1) { /* … */ }
```

**VLT-02 [MEDIUM] Μία εγγραφή πάνω από το όριο ciphertext του server σταματά οριστικά το upload όλων των επόμενων εγγραφών, σε όλες τις συσκευές** (S1-03, S8-03) · C
- **Location:**
  - `docs/supabase/password-manager.sql:29` (`check (length(ciphertext) <= 262144)`)
  - `backend/vault/src/vault.rs:813` και `:875-882` (κανένα όριο στο client)
  - `sync.rs:165-169` (κάθε μη-409 αποτυχία γίνεται `Err`)
  - `sync.rs:240-244` (`push_outgoing` επιστρέφει στο πρώτο σφάλμα)
  - `backend/src/passwords/mod.rs:99` (το background loop καταπίνει το σφάλμα)
- **Root cause:** δεν υπάρχει `maxlength` στο `EntryEditor.svelte`. Ένα note γύρω στα 190 KB παίρνει 400, και κάθε pass κολλάει πάνω του.
- **Fix:**
```rust
// vault.rs, μετά τη σφράγιση
const MAX_CIPHERTEXT: usize = 262_144; // = docs/supabase/password-manager.sql
if ciphertext.len() > MAX_CIPHERTEXT { return Err("This entry is too large to sync; shorten its notes.".into()); }
// sync.rs push_item: μια γραμμή που απορρίφθηκε δεν σταματά τις υπόλοιπες
s if s.is_client_error() && s != StatusCode::UNAUTHORIZED && s != StatusCode::FORBIDDEN => Ok(false),
```
Ορίστε επίσης `maxlength` στα notes του editor.

**VLT-03 [MEDIUM] Οι IPv4 hosts λογίζονται ως ίδιο «site» με κάθε άλλη IP που έχει τα ίδια δύο τελευταία octets: logins προτείνονται και συμπληρώνονται σε λάθος IP** (S8-01) · ✔
- **Location:**
  - `backend/src/passwords/browser.rs:1075-1077` (`site()` → `psl::domain_str`)
  - ο κανόνας εφαρμόζεται στα `:1182` (logins), `:1238` (fill), `:1253`, `:1273`, `:1297` (totp), `:1373` (copy), `:1695` (`same_login`)
  - το extension (`extension/background.js:72`) θεωρεί σωστά κάθε IP ξεχωριστό site
  - το `backend/vault/src/passkeys.rs:128` εξαιρεί ήδη τις IPs
- **Root cause:** με τον default κανόνα `*` του PSL, το `domain_str("192.168.1.1")` και το `domain_str("10.20.1.1")` δίνουν και τα δύο `"1.1"`. Αν ο χρήστης έχει login για `https://192.168.1.1` (router/NAS), μια σελίδα στο `https://203.0.1.1` με έγκυρο IP certificate περνά τον έλεγχο `wrongSite`.
- **Fix:**
```rust
fn site(host: &str) -> String {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.parse::<std::net::IpAddr>().is_ok() || !host.contains('.') {
        return host.to_string(); // μια IP ή ένα single label είναι δικό του site
    }
    psl::domain_str(host).unwrap_or(host).to_string()
}
```
Μεταφέρετέ το στο `myle_vault::sites` και φτιάξτε κοινό JSON με test vectors για το `extension-sites.test.mjs` και ένα Rust test.

**VLT-04 [MEDIUM] Το native bridge δέχεται οποιοδήποτε process του ίδιου χρήστη· η ταυτότητα του caller βασίζεται σε argv και ονόματα αρχείων που πλαστογραφούνται** (S8-02, S1 informational) · C
- **Location:**
  - `browser.rs:359-381` (το mode επιλέγεται από argv)
  - `:327-355` (`ancestors()` συγκρίνει μόνο `szExeFile`)
  - `:393-405` (`which_copy` από argv)
  - `:912-918` (ίδιο exe path)
  - `:1230-1244` (`fill` επιστρέφει plaintext)
- **Πώς ενεργοποιείται:** όσο το vault είναι ανοιχτό, ένα process του ίδιου χρήστη αντιγράφεται ως `chrome.exe` και τρέχει `MYLE.exe chrome-extension://<id>/`. Μετά στέλνει `logins` και `fill`. Rate limit: 20 passwords/λεπτό, 120 enumerations/λεπτό.
- **Fix:**
  - Pairing token: ο χρήστης πατά «Pair» στο MYLE και ένα random token αποθηκεύεται στο `chrome.storage.local`.
  - Το token απαιτείται σε όλα εκτός από `status`, `open` και `pair`, με constant-time σύγκριση.
  - Επίσης έλεγχος του full image path και του Authenticode signer του γονικού browser.
```rust
#[derive(Deserialize)] struct Envelope { browser: String, #[serde(default)] copy: String,
    #[serde(default)] version: String, #[serde(default)] token: String, request: Request }
if !matches!(request, Request::Status | Request::Open) && !pairing::matches(&token) {
    return json!({ "ok": false, "error": "notPaired" });
}
```

**VLT-05 [MEDIUM] Το sign-out στο PC αποσυνδέει και το κινητό** (S1-09, S8-04) · ✔
- **Location:**
  - `backend/src/account/mod.rs:283` → `backend/vault/src/account.rs:157-159` (`logout` = `scope=global`)
  - το mobile χρησιμοποιεί `logout_local` (`backend/mobile/src/account.rs:368`)
  - το σχόλιο στο desktop («Revokes the refresh token») δείχνει ότι ήθελε μόνο το δικό του session
- **Επίπτωση:** μέσα σε περίπου μία ώρα το refresh του κινητού απορρίπτεται και το sync σταματά σιωπηλά.
- **Fix:** `myle_vault::account::logout_local(&client()?, &session).await;` και ξεχωριστή επιλογή «Sign out everywhere».

**VLT-06 [LOW] Αλλαγή master password κατά τη διάρκεια ενός sync μπορεί να επαναφέρει σιωπηλά τον παλιό κωδικό** (S1-04) · C
- **Location:** `sync.rs:202-204` και `:188-190`· `vault.rs:1091-1095` (`header_synced` καθαρίζει χωρίς όρους το `header_dirty`).
- **Root cause:** αν η αλλαγή γίνει όσο εκκρεμεί το PATCH, το νέο header μαρκάρεται synced χωρίς να έχει σταλεί. Στο επόμενο pass το `take_header(remote)` επαναφέρει το παλιό wrap, που μπορεί να είναι ένας κωδικός που ο χρήστης απέσυρε επειδή διέρρευσε.
- **Fix:**
```rust
pub fn header_synced(&mut self, sent: &RemoteHeader) -> Result<(), String> {
    let unchanged = self.sync_header().is_some_and(|(now, _)| &now == sent);
    let file = self.file.as_mut().ok_or("There is no vault yet.")?;
    if unchanged { file.header_dirty = false; }
    self.write()
}
```

**VLT-07 [LOW] Τα ιδιωτικά κλειδιά των passkeys αντιγράφονται χωρίς zeroize, και ένα κρατημένο αντίγραφο υπογράφει ακόμη κι αφού κλειδώσει το vault** (S1-05) · C
- **Location:** `vault.rs:648-671` (clone στο `:666`)· `browser.rs:1534`, `:1540`, `:1583`, `:1603-1621`.
- **Root cause:**
  - Το `Passkey` είναι `Zeroize` αλλά όχι `ZeroizeOnDrop`.
  - Κλωνοποιείται σε κάθε list ή exclude, μέχρι 120 φορές το λεπτό.
  - Το κλειδί κρατιέται όσο περιμένει το Hello (έως 120 s) και χρησιμοποιείται ακόμη κι αν το vault κλείδωσε στο μεταξύ.
- **Fix:** τα listings επιστρέφουν `PasskeyInfo`· η υπογραφή γίνεται μέσα στο lock μετά την επαλήθευση:
```rust
let credential = operation.commit(|| state.with_quiet(|vault| {
    let (_, _, key) = vault.passkeys_for(&rp_id, std::slice::from_ref(&credential_id))?
        .into_iter().next().ok_or("notFound")?;      // «The vault is locked.» αν κλείδωσε
    let key = zeroize::Zeroizing::new(key);
    vault.touch();
    passkeys::sign_in(&key, &challenge, &origin, verified)
}))?;
```

**VLT-08 [LOW] Ένας κωδικός που αντιγράφηκε μένει στο clipboard αν το MYLE κλείσει ή γίνει update μέσα σε 30 s, ή αν κλειδώσει το vault· υπάρχει και race στο καθάρισμα** (S1-06) · C
- **Location:** `backend/src/passwords/clipboard.rs:69-73`, `:92-100`· exit handler στο `backend/src/lib.rs:661-673`.
- **Root cause:**
  - Το καθάρισμα είναι ένα detached thread που κοιμάται 30 s και πεθαίνει μαζί με το process.
  - Το exit, το `watch()` και το `lock_now` δεν καθαρίζουν ποτέ το clipboard.
  - Ο έλεγχος του sequence number γίνεται πριν από το `OpenClipboard`, που ξαναδοκιμάζει για έως 300 ms· κάτι που αντιγράφεται σε αυτό το διάστημα σβήνεται.
- **Fix:**
```rust
static PENDING: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);
pub fn clear_pending() {
    if let Some(seq) = PENDING.lock().unwrap_or_else(|p| p.into_inner()).take() { clear_if_unchanged(seq) }
}
pub fn clear_if_unchanged(sequence: u32) {
    if open().is_err() { return; }
    unsafe { if GetClipboardSequenceNumber() == sequence { EmptyClipboard(); } CloseClipboard(); }
}
// copy_secret(): *PENDING.lock()… = Some(sequence);  — clear_pending() στο RunEvent::Exit και σε κάθε lock
```

**VLT-09 [LOW] Στη λειτουργία «both» το Windows fill πληκτρολογεί τον κωδικό όπου κι αν καταλήξει το Tab** (S1-08) · C
- **Location:** `backend/src/passwords/windows_fill.rs:395-398`.
- **Root cause:** μετά το `press(VK_TAB)` δεν ξαναελέγχεται το `IsPassword`. Αν το Tab καταλήξει σε πεδίο 2FA ή captcha, ο κωδικός γράφεται εκεί σε απλό κείμενο.
- **Fix:**
```rust
press(VK_TAB)?;
std::thread::sleep(Duration::from_millis(120));
if focused_field_is_password() == Some(false) {
    return Err(format!("The next field shows what is typed. Click the password field and press {hotkey} again."));
}
typing(&password, focused_control(selected.thread))?;
```

**VLT-10 [LOW] Το settings sync χάνει ενημερώσεις όταν δύο PC στέλνουν σχεδόν ταυτόχρονα· το DDL και το RLS του `user_settings` δεν υπάρχουν στο repo** (S1-10, S8-08) · C / P (RLS)
- **Location:** `backend/src/account/mod.rs:299-327`, `account/settings_sync.rs:26-60`, `mod.rs:18`.
- **Root cause:** γίνεται GET και μετά POST `merge-duplicates`, που αντικαθιστά όλη τη στήλη `data` χωρίς compare-and-swap. Το client στέλνει `user_id` στο body και βασίζεται σε RLS που δεν μπορεί να ελεγχθεί από το repo.
- **Fix:**
  - PATCH με φίλτρο `updated_at=eq.<seen>`· αν δεν ενημερωθεί καμία γραμμή, re-pull και επανάληψη. Εναλλακτικά, ένα RPC που κάνει server-side `jsonb` merge.
  - Commit του DDL και του RLS του `user_settings` στο `docs/supabase/`.

**VLT-11 [LOW] Οι master passwords και τα αποκρυπτογραφημένα δεδομένα δεν μηδενίζονται με συνέπεια** (S1-11) · C
- **Location:**
  - `backend/src/passwords/mod.rs:157-205`, `:844-853`: απλά `String` στα unlock, create, recover, change_master και export (ενώ τα `verify_answer` και `delete_all` χρησιμοποιούν `Zeroizing`)
  - `vault.rs:803-810`, `:1043-1050` (`all_entries`/`history`)
  - `vault.rs:843` (truncate του history χωρίς wipe)
  - `vault.rs:981-982` (διπλότυπα του import)
  - `clipboard.rs:56` (buffer του `wide()`)
- **Fix:** `Zeroizing<String>` και `Zeroizing<Vec<_>>` παντού· `zeroize()` πριν από κάθε `truncate`.

**VLT-12 [LOW] Τα conflicts τα αποφασίζει ένα πεδίο χωρίς αυθεντικοποίηση** (S1 quality) · C
- **Location:** `vault.rs:1209-1210` (`resolve()` κοιτάζει το plaintext `updated_at` της γραμμής).
- **Root cause:** ο server μπορεί να διαλέξει τον νικητή ενός conflict, ακόμη και να κάνει μια παλιότερη διαγραφή να υπερισχύσει μιας νεότερης τοπικής επεξεργασίας.
- **Fix:** χρήση του σφραγισμένου `entry.updated_at`, και ο χρόνος διαγραφής να μπει μέσα στο σφραγισμένο περιεχόμενο του tombstone.

**VLT-13 [LOW] Το pagination του `get_items` υποθέτει ότι ο server επιστρέφει πάντα 1.000 γραμμές ανά σελίδα** (S1) · C
- **Location:** `backend/vault/src/sync.rs:20`, `:126`.
- **Root cause:** με μικρότερο `db-max-rows` στο project, το sync σταματά σιωπηλά μετά την πρώτη σελίδα.
- **Fix:** συγκρίνετε με το μέγεθος της σελίδας που πραγματικά ήρθε, ή χρησιμοποιήστε το `Content-Range`.

**VLT-14 [LOW] Το `Session` κάνει derive `Debug` ενώ περιέχει access και refresh tokens** (S1) · C
- **Location:** `backend/vault/src/account.rs:50`.
- **Fix:** χειροκίνητο `impl Debug` που κάνει redact τα tokens, όπως ήδη κάνει το Steam session (`steam_automation/session.rs:16-20`).

**VLT-15 [LOW] Το «Load unpacked» αντίγραφο του extension κρατά αρχεία που δεν υπάρχουν πια στο bundle** (S1) · C
- **Location:** `backend/src/passwords/extension_folder.rs:2-15` (`copy_tree`).
- **Fix:** αντιγραφή σε φάκελο staging και rename, ή διαγραφή όσων αρχείων δεν βρίσκονται στο manifest του bundle.

**VLT-16 [LOW] Το πρωτόκολλο του native bridge δεν έχει version ούτε feature negotiation** (S8-10) · C
- **Location:** `browser.rs:955-1041` (αυστηρό enum `tag="type"`), `:946`· `extension/background.js:291`, `:720`.
- **Root cause:** το extension (11.1.0) ενημερώνεται ανεξάρτητα από το app (11.0.1). Ένα νέο request type σε παλιότερο MYLE εμφανίζεται ως `badRequest`.
- **Fix:** variant `#[serde(other)] Unknown` → `"unknownType"`, και `protocol`/`features` στην απάντηση του `status`.

**VLT-17 [LOW] Μικρές αποκλίσεις κανόνων μεταξύ extension και Rust** (S8-12) · C
- Το `www.` αφαιρείται μία φορά στο JS (`background.js:63`) αλλά επαναληπτικά στο Rust (`browser.rs:1082`, `vault/src/sites.rs:11`).
- Τα content scripts δεν ταιριάζουν το `http://[::1]` (`extension/manifest.json`), ενώ το `background.js:101` και το `browser.rs:1070` το επιτρέπουν.
- Ο uninstaller αφαιρεί τα native-host keys (`installer/src/registry.rs:172-185`) αλλά όχι το `HKCU\Software\Classes\myle` που γράφει το `browser.rs:607-620`, οπότε τα `myle://` links καταλήγουν σε exe που δεν υπάρχει.
- **Fix:**
  - ενοποίηση του κανόνα για το `www.`·
  - προσθήκη του `http://[::1]/*` στο manifest, ή αφαίρεσή του από το Rust·
  - διαγραφή του `Software\Classes\myle` στο uninstall.

**VLT-18 [LOW] Το `delete_passkey` συγκρίνει credential ids με ακριβή σύγκριση, ενώ οι αναζητήσεις χρησιμοποιούν `same_credential`** (S1) · C
- **Location:** `vault.rs:730`.
- **Fix:** χρήση του `same_credential` και εδώ.

**VLT-19 [LOW] Η page world του extension αντικαθιστά καθολικά το `Function.prototype.toString`** (S1) · C
- **Location:** `extension/passkeys.js:75-84`.
- **Root cause:** κίνδυνος συμβατότητας σε κάθε HTTPS σελίδα.
- **Fix:** περιορίστε το override στις συναρτήσεις του MYLE (WeakSet) και τεκμηριώστε το.

### 3.2 Disk Analyzer / Cleaner / Cloud

**DSK-01 [MEDIUM] Stack overflow στον elevated cleaner με βαθύ δέντρο φακέλων** (S2-03) · ✔
- **Location:** `backend/src/cleaner/elevated.rs:238` (`let mut buffer = [0usize; 8192];`), αναδρομή στο `:372`, `MAX_DEPTH = 128` στο `:34`.
- **Root cause:**
  - Κάθε frame του `visit_directory` κρατά 64 KB στη στοίβα κατά την αναδρομή.
  - Ο helper τρέχει στο main thread (`main.rs`), που έχει στοίβα 1 MB, άρα αντέχει περίπου 15 επίπεδα.
  - Οποιοσδήποτε χρήστης μπορεί να φτιάξει `C:\Windows\Temp\a\a\…` βάθους 16 επιπέδων. Το «Clean as administrator» τότε αποτυγχάνει πάντα με `STATUS_STACK_OVERFLOW`.
- **Fix:** `let mut buffer = vec![0usize; 8192];`, μία φορά ανά φάκελο, έξω από το `loop`. Προσθέστε test με δέντρο βάθους 40 επιπέδων.

**DSK-02 [LOW-MEDIUM] Το `recheck` της διαγραφής δεν ελέγχει αν κάποιος γονικός φάκελος έγινε junction μετά το scan** (S2-06) · C
- **Location:** `backend/src/disk_analyzer/delete.rs:170-189`.
- **Root cause:** ελέγχει μόνο αν το ίδιο το αντικείμενο είναι link. Αν το `D:\x\link\` έγινε junction προς τον Roaming φάκελο του MYLE, η διαγραφή του `D:\x\link\vault` παίρνει το vault.
- **Fix:**
```rust
let parent = path.parent().ok_or_else(|| OUTSIDE.to_string())?;
let real = std::fs::canonicalize(long_path(parent)).map_err(|_| GONE.to_string())?;
if protect::parts(&real) != protect::parts(parent) {
    return Err(CHANGED.into()); // κάποιος γονικός φάκελος έγινε link μετά το scan
}
```

**DSK-03 [LOW] Τα σύνολα του elevated cleaner είναι ελλιπή** (S2-09) · C
- **Location:** `cleaner/elevated.rs:153-158`.
- **Root cause:**
  - Το `handle()` δεν αθροίζει το `skipped_bytes`, οπότε το «still in use» μετά το administrator pass δείχνει πάντα 0 (`cleaner/mod.rs:257`).
  - Το `visit_target(..)?` σταματά όλο το αίτημα στο πρώτο target που αποτυγχάνει, αφού έχουν ήδη διαγραφεί τα προηγούμενα· αυτές οι διαγραφές αναφέρονται μόνο ως σφάλμα.
- **Fix:** `total.skipped_bytes += one.skipped_bytes;` και ένα target που αποτυγχάνει να μετρά ως skipped αντί για `?`.

**DSK-04 [LOW] Το Health reveal δέχεται οποιοδήποτε raw path από τη σελίδα** (S2-12) · C
- **Location:** `backend/src/cleaner/health/mod.rs:841-882` (`cleaner_health_reveal(path)`).
- **Root cause:** το `exists()` σε UNC path ανοίγει σύνδεση SMB, άρα NTLM leak αν παραβιαστεί το webview· και ανοίγει οποιονδήποτε φάκελο.
- **Fix:** να δέχεται finding id και να βρίσκει το path στο `FOUND`, ή να απορρίπτει `\\` και `\\?\UNC`.

**DSK-05 [LOW] Το Software Health μπορεί να μείνει «busy» για πάντα** (S2-14) · P
- **Location:** `cleaner/health/sys.rs:34-74` (`.output()` χωρίς timeout)· lock στο `health/mod.rs:537`.
- **Root cause:** ένα WMI query ή `Get-ScheduledTask` που κολλάει κρατά το `OPERATION` lock μέχρι το restart.
- **Fix:** τα pipes να διαβάζονται σε threads, με kill μετά από timeout 60–120 s (ίδιο pattern με το WIN-03).

**DSK-06 [LOW] Το restore journal γράφεται πριν από την ενέργεια** (S2-15) · C
- **Location:** `cleaner/health/mod.rs:917-927`, `:937-958`, `:1282-1285`.
- **Root cause:** ένα recycle ή registry delete που αποτυγχάνει αφήνει εγγραφή επαναφοράς για αλλαγή που δεν έγινε ποτέ. Οι εγγραφές `Undo::Recycled` δεν μπορούν να αφαιρεθούν.
- **Fix:** registry export, μετά η ενέργεια, και μόνο μετά η εγγραφή στο journal. Οι εγγραφές `Recycled` να μπορούν να απορριφθούν από τον χρήστη.

**DSK-07 [LOW] Ο MFT reader εμπιστεύεται μεγέθη από τον δίσκο** (S2-16) · C
- **Location:** `backend/src/disk_analyzer/mft.rs:591-599`, `:246-247`, `:440`.
- **Root cause:** το `count = size / record` (έως `u32::MAX`) οδηγεί σε `Table::new(count)`, δηλαδή περίπου 128 GB και abort. Τα `clusters * cluster` και `slot.alloc +=` κάνουν wrap στο release.
- **Fix:** `checked_mul` και όριο στο `count` βάσει του συνόλου των extents και του μεγέθους του volume (total sectors × sector size).

**DSK-08 [LOW] Τα hard links δίνουν λάθος «freed bytes» στις διαγραφές του fast scan** (S2) · C
- **Location:** `disk_analyzer/tree.rs:622-662`.
- **Root cause:** μετριούνται bytes που δεν ελευθερώνονται όσο υπάρχουν άλλα ονόματα του ίδιου αρχείου.
- **Fix:** αφαίρεση των bytes μόνο όταν το link count φτάσει στο 1, αλλιώς ένδειξη «shared».

**DSK-09 [LOW] Δεν υπάρχει προειδοποίηση για φακέλους που συγχρονίζονται στο cloud** (S2) · C
- **Location:** `disk_analyzer/protect.rs`.
- **Root cause:** το recycle σε sync root διαγράφει το αντικείμενο και στο cloud και στις άλλες συσκευές.
- **Fix:** επίπεδο `Caution` για τα `cloud::sync_roots()`.

**DSK-10 [LOW] Μη ατομική εγγραφή στο undo του Chrome `Preferences`** (S2) · C
- **Location:** `cleaner/health/mod.rs:1268` (`std::fs::write`, ενώ το apply χρησιμοποιεί atomic writer).
- **Fix:** ο ίδιος atomic writer και στο undo.

**DSK-11 [LOW] Ελληνικά strings μέσα στο αγγλικό UI του permissions scan** (S2) · C
- **Location:** `cleaner/health/scans.rs:1132-1134`, `:1162-1164`, `:1198-1205`.
- **Fix:** μετάφραση στα αγγλικά, για συνέπεια με την υπόλοιπη εφαρμογή.

**DSK-12 [LOW] Dead command: `cleaner_health_apply`** (S8-12) · C
- **Location:** `lib.rs:489`, `cleaner/health/mod.rs:595`· το UI καλεί μόνο το `_batch`.
- **Fix:** αφαίρεση.

### 3.3 Debloat / Windows Optimization / Maintenance / Elevated pipe

**WIN-01 [MEDIUM] Το Start μπορεί να μείνει κλειδωμένο για pin και unpin** (S2-04) · P
- **Location:** `backend/src/debloat/mod.rs:512-533`, `:535-545`· `debloat/start_menu.rs:309-335`.
- **Root cause:**
  - Το policy `ConfigureStartPins` καθαρίζεται από ένα δεύτερο helper request, περίπου 1,4 s αργότερα, με `let _ = ask(...)`.
  - Αν το app ή ο helper πεθάνει σε αυτό το διάστημα, το policy μένει.
  - Το `status()` δεν το αναφέρει, οπότε μετά από restart ο χρήστης δεν μπορεί να κάνει pin και δεν ξέρει γιατί.
- **Fix:**
  - retry και επιστροφή του σφάλματος του clear·
  - στο `status()`, ανίχνευση `ConfigureStartPins_WinningProvider == PROVIDER_GUID` με πρόταση καθαρισμού·
  - ιδανικά, ένα helper request που εφαρμόζει και καθαρίζει μόνο του.

**WIN-02 [MEDIUM] TOCTOU στο log του elevated maintenance script: ένα process του ίδιου χρήστη μπορεί να κάνει elevated write σε οποιοδήποτε αρχείο** (S2-05) · P
- **Location:** `backend/src/maintenance/runner.rs:259-269` (`script_start`), `:241-248` (`cmd_tail`), `:32-43` (workspace στο `%TEMP%`).
- **Root cause:**
  - Ο έλεγχος ότι ο φάκελος δεν είναι reparse point και ότι το `output.log` δεν υπάρχει γίνεται πριν από το `cmd >> "<dir>\output.log"`, που ανοίγει το αρχείο με το όνομα.
  - Στο μεταξύ, ο φάκελος μπορεί να γίνει junction προς `\RPC Control` και το `output.log` symlink προς το `C:\Windows\System32\x`.
- **Fix:**
```powershell
$fs = [IO.File]::Open($log, 'CreateNew', 'Write', 'ReadWrite')   # πριν από το cmd
# έλεγχος του τελικού path του handle (GetFinalPathNameByHandle) == αναμενόμενο· το $fs μένει ανοιχτό όσο γράφει το cmd
```
Εναλλακτικά: workspace σε φάκελο που δημιουργεί ο admin, με ACL που επιτρέπει στον χρήστη μόνο ανάγνωση.

**WIN-03 [LOW-MEDIUM] Deadlock στο `run_script` του debloat όταν το stdout ξεπερνά το buffer του pipe** (S2-08) · P
- **Location:** `backend/src/debloat/helper.rs:217-240`.
- **Root cause:** το stdout διαβάζεται μόνο μετά το exit. Με πάνω από ~4 KB warnings το PowerShell μπλοκάρει, και το «PowerShell took too long» βγαίνει έως 15 λεπτά αργότερα, ακόμη κι αν η ενέργεια ολοκληρώθηκε.
- **Fix:**
```rust
let mut out = child.stdout.take().ok_or("no stdout")?;
let reader = std::thread::spawn(move || { let mut b = Vec::new(); let _ = std::io::Read::read_to_end(&mut out, &mut b); b });
let started = Instant::now();
while child.try_wait().map_err(|e| e.to_string())?.is_none() {
    if started.elapsed() > timeout { let _ = child.kill(); let _ = child.wait(); return Err("PowerShell took too long.".into()); }
    std::thread::sleep(Duration::from_millis(200));
}
Ok(String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned())
```

**WIN-04 [LOW] Το elevated pipe έχει default DACL και δεν ελέγχει ότι ο client είναι elevated** (S2-07) · C
- **Location:** `backend/src/elevated_pipe.rs:42-46`, `:72-74`, `:112-118`.
- **Root cause:**
  - Το default security descriptor δίνει read στους Everyone και Anonymous: ένας άλλος χρήστης ή ένα low-IL process μπορεί να ανοίξει πρώτο το pipe (DoS του UAC flow).
  - Ένα process του ίδιου χρήστη μπορεί να ξεκινήσει μη-elevated το `MYLE.exe --cleaner-elevated-helper serve <pipe> <pid>` και να κερδίσει τον αγώνα· τα requests τρέχουν τότε χωρίς δικαιώματα admin.
- **Fix:**
  - SDDL `D:P(A;;GA;;;<user SID>)(A;;GA;;;BA)(A;;GA;;;SY)` μέσω `ServerOptions::create_with_security_attributes_raw`·
  - στο `client_is_this_program`, `GetTokenInformation(TokenElevation)` πρέπει να είναι elevated.

**WIN-05 [LOW] Το undo του debloat είναι σιωπηλά no-op για τιμές REG_QWORD, REG_BINARY και REG_MULTI_SZ** (S2-10) · C
- **Location:** `backend/src/debloat/system.rs:90-108` (→ `Value::Other`), `:138-154` (`Other => Ok(())`)· `debloat/helper.rs:174-176`.
- **Root cause:** το apply αντικαθιστά την τιμή, το undo «πετυχαίνει» χωρίς να γράψει τίποτα και η εγγραφή ξεχνιέται. Επιπλέον το REG_EXPAND_SZ επαναφέρεται ως REG_SZ.
- **Fix:** άρνηση εφαρμογής όταν η τρέχουσα τιμή είναι `Value::Other` (ή αποθήκευση των raw bytes μαζί με τον τύπο), και ξεχωριστό variant `ExpandSz`.

**WIN-06 [LOW] Η επαναφορά του «Recommended» στο Start σβήνει τιμές που δεν έγραψε το MYLE** (S2-11) · C
- **Location:** `backend/src/debloat/start_menu.rs:252-269`.
- **Root cause:** το hide γράφει `IsEducationEnvironment=1`, με παρενέργειες πέρα από το Start, και τρία policies, χωρίς να κρατά την προηγούμενη κατάσταση. Το unhide τα διαγράφει όλα, μαζί με όσα είχαν βάλει GPO ή MDM.
- **Fix:** καταγραφή των προηγούμενων τιμών όπως κάνει το debloat undo store, και επαναφορά μόνο όσων έγραψε το MYLE.

**WIN-07 [LOW] Μετά από μία έγκριση UAC, κάθε helper εξυπηρετεί για 30 λεπτά** (S2-17) · C
- **Location:** `elevated_pipe.rs:29`, `:298-304`.
- **Root cause:** με over-the-shoulder elevation (standard χρήστης, ο admin πληκτρολογεί credentials), ο standard χρήστης κρατά για 30 λεπτά ενέργειες με δικαιώματα admin, όπως MFT listing όλων των profiles.
- **Fix:** τερματισμός του helper μετά το request όταν ο χρήστης του app δεν είναι administrator.

**WIN-08 [LOW] Race στο idle exit του helper** (S2) · C
- **Location:** `elevated_pipe.rs:298-304`.
- **Root cause:** το `busy` ελέγχεται και μετά καλείται `exit` χωρίς lock. Ένα request που φτάνει στο όριο των 30 λεπτών μπορεί να διακοπεί στη μέση, π.χ. κατά την αφαίρεση του Edge.
- **Fix:** ο έλεγχος και το exit κάτω από το ίδιο mutex.

**WIN-09 [LOW] Τα πρωτόκολλα των elevated helpers δεν έχουν version, και μετά από live update μπορεί να μιλά παλιό app με νέο helper** (S8-06) · P
- **Location:**
  - τέσσερα διαφορετικά σχήματα: `cleaner/elevated.rs:65-75`, `disk_analyzer/elevated.rs:12-25` (binary snapshot χωρίς version), `debloat/helper.rs:19-34`, `windows_optimization/autologon.rs:408-415` (το μόνο με `FORMAT_VERSION`)
  - transport: `elevated_pipe.rs:81-91`· identity: `:184-189`
  - το `apps/process.rs:68-91` αντιστοιχίζει **κάθε** exception του `Start-Process` σε `ERROR_CANCELLED`, άρα σε `DECLINED`
- **Root cause:** το UI δείχνει «Administrator approval was declined» (`debloat/state.svelte.ts:210`) ακόμη και για αποτυχίες που δεν σχετίζονται με το UAC.
- **Fix:**
  - κοινό envelope `{ v: 1, build: env!("CARGO_PKG_VERSION"), op }` και ο helper αρνείται διαφορετικό `build`·
  - identity με file ID ή hash αντί για path·
  - ένα runas primitive (`ShellExecuteExW`) με ακριβές `ERROR_CANCELLED`.

### 3.4 Steam / Game Saves / Project Backups / Auto Clicker

**AUT-01 [MEDIUM] Panic που κάνει abort στο scan του Game Saves όταν ένα path τελειώνει σε `\Users`** (S3-03) · ✔
- **Location:** `backend/src/game_saves/parser.rs:395-396` (`&parts[users + 2..]`). Το καλεί το `build_scan` (`game_saves/commands.rs:1249`) σε κάθε scan, και στο headless backup.
- **Root cause:** όταν το `users` είναι το τελευταίο component, το slice ξεπερνά το μήκος. Συμβαίνει π.χ. σε νέο PC με save που υπάρχει μόνο στο backup, στο `D:\Games\Foo\Users` ή απευθείας στο `C:\Users`.
- **Fix:** `let after = parts.get(users + 2..)?;`

**AUT-02 [MEDIUM] Τα saves στο registry δεν καλύπτονται ούτε από το safety copy ούτε από το Undo** (S3-04) · C
- **Location:** `backend/src/game_saves/safety.rs:64-79` (`targets` = μόνο `entry.files`)· χρήση στα `commands.rs:859`, `:900`, `:974`· registry στο `parser.rs:46-47`.
- **Root cause:** αν ένα restore παιχνιδιού που κρατά saves στο HKCU αποτύχει στη μέση, επανέρχονται τα αρχεία αλλά όχι οι τιμές του registry. Το αποτέλεσμα είναι μείγμα παλιών και νέων saves, ενώ τα docs υπόσχονται «put back exactly as it was».
- **Fix:**
```rust
pub(crate) fn registry_targets(api: &ApiOutput) -> Vec<(String, String)> {
    api.games.iter().flat_map(|(game, entry)| entry.registry.iter()
        .filter(|(_, item)| !item.ignored)
        .map(move |(key, _)| (game.clone(), key.replace('/', "\\")))).collect()
}
fn export_key(slot: &Path, index: usize, key: &str) -> Result<Option<PathBuf>, String> {
    let file = slot.join(format!("reg-{index}.reg"));
    let status = std::process::Command::new(crate::apps::process::system32("reg.exe"))
        .args(["export", key]).arg(&file).arg("/y")
        .creation_flags(CREATE_NO_WINDOW).status().map_err(|e| e.to_string())?;
    Ok(status.success().then_some(file)) // None: το key δεν υπήρχε, οπότε στο rollback διαγράφεται
}
// put_back: `reg delete <key> /f`, μετά `reg import <file>` όταν υπάρχει αρχείο.
```
Αν αυτό μείνει για αργότερα, το UI να λέει τουλάχιστον ότι τα saves στο registry δεν καλύπτονται.

**AUT-03 [MEDIUM] Ένα αρχείο που εξαφανίζεται κατά τη διάρκεια του backup κάνει όλο το backup να αποτύχει** (S3-05) · C
- **Location:** `backend/src/project_backups/walk.rs:256-257`· `archive.rs:113-115`.
- **Root cause:** συμβαίνει με IDE ή dev server ανοιχτό, π.χ. με `vite.config.ts.timestamp-*.mjs`, `~$x.docx` ή `*.db-journal`.
- **Fix:**
```rust
// walk.rs, Kind::File
let meta = match fs::metadata(&path) {
    Ok(meta) => meta,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue, // χάθηκε μετά το listing
    Err(e) => return Err(format!("Can't read \"{}\": {e}", path.display())),
};
// archive.rs write_entries
let mut input = match File::open(&source.abs) {
    Ok(f) => f,
    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
    Err(e) => return Err(io_failure(format!("Can't read \"{}\"", source.rel), e, &source.abs)),
};
```
Τα incremental backups μένουν ασφαλή: ο έλεγχος πληρότητας στο `incremental.rs:422-433` εξακολουθεί να αρνείται increment με αρχεία που λείπουν.

**AUT-04 [MEDIUM] Βαρύ συγχρονισμένο I/O μέσα στα async commands του Game Saves** (S3-06) · C
- **Location:** `game_saves/commands.rs`:
  - `build_scan` στο `:1249` (με `parser.rs:292-296`, `:494`, `:501-524`)
  - `verify_snapshot` στο `:869` (αποσυμπιέζει ολόκληρα zips, ίσως από cloud)
  - `safety::take` στο `:900`, `put_back` στο `:974`
  - `locked` στα `:875`, `:931`· `ensure_room` στα `:883-889`
  - `settings::initialize`
- **Root cause:** όλα τρέχουν στα tokio workers, που είναι περίπου ένα ανά πυρήνα. Ένα restore από το Drive τα μπλοκάρει για λεπτά.
- **Fix:**
```rust
let to_verify: Vec<(String, PathBuf)> = /* …κλώνοι των titles/paths… */;
tauri::async_runtime::spawn_blocking(move || to_verify.iter().try_for_each(|(title, path)|
    safety::verify_snapshot(path).map_err(|e| format!("{title} was not restored, and nothing was changed: its backup is damaged. {e}"))))
    .await.map_err(|e| e.to_string())??;
// Ίδιο pattern για build_scan, safety::take, put_back, scan_cache::save (commands.rs:570).
```

**AUT-05 [MEDIUM] Το text diff δεν έχει timeout** (S3-07) · P (δεν επαληθεύτηκαν τα defaults του `similar`)
- **Location:** `backend/src/project_backups/compare.rs:776` (`TextDiff::from_lines`).
- **Root cause:** ο Myers αλγόριθμος κοστίζει O((N+M)·D), και επιτρέπονται αρχεία έως 2 MiB. Ένα regenerated lockfile ή JSON κρατά ένα blocking thread για δεκάδες δευτερόλεπτα.
- **Fix:** `similar::TextDiff::configure().timeout(Duration::from_secs(2)).diff_lines(&old, &new)`.

**AUT-06 [MEDIUM] Το pagination του inventory στο Steam μπορεί να κάνει ατέρμονο loop** (S3-08) · P (εξαρτάται από το αν το Steam επαναλαμβάνει cursor)
- **Location:** `backend/src/steam_automation/inventory.rs:156-171` (`read_pages`)· το `read_listings` έχει ήδη guard στο `:291`.
- **Root cause:** με `more_items: 1` και τον ίδιο `last_assetid`, το `sell_duplicates` (`engine.rs:466-470`) κάνει loop για πάντα, με ένα request ανά 3 s. Farming, idling και achievements σταματούν.
- **Fix:**
```rust
let mut pages = 0u32;
loop {
    let (found, more) = parse(&api.get(&url).await?).map_err(CommunityError::Other)?;
    all.extend(found); pages += 1;
    match more {
        Some(next) if start.as_deref() != Some(next.as_str()) && pages < 100 => start = Some(next),
        Some(_) => return Err(CommunityError::Other("Steam kept sending the same inventory page.".into())),
        None => return Ok(all),
    }
}
```

**AUT-07 [LOW-MEDIUM] Ο parser του git index αγνοεί το split index, οπότε ένας tracked φάκελος build εξαιρείται από το backup** (S3-09) · P
- **Location:** `backend/src/project_backups/gitindex.rs:41-83`, χρήση στα `rules.rs:557-585`, `:536-545`· `hash_size` στο `gitindex.rs:33-39`.
- **Root cause:**
  - Με `core.splitIndex` το `.git/index` περιέχει μόνο τις αλλαγές. Ένας tracked φάκελος `build/` ή `bin/` φαίνεται untracked και εξαιρείται ως «build output».
  - Επιπλέον, για worktrees το `config` διαβάζεται από το gitdir αντί για το commondir.
- **Fix:**
```rust
let end = bytes.len().saturating_sub(hash_size);
let mut ext = at;
while ext.checked_add(8).is_some_and(|e| e <= end) {
    if &bytes[ext..ext + 4] == b"link" { return None; } // split index: οι εγγραφές είναι στο sharedindex.*
    let size = u32::from_be_bytes(bytes[ext + 4..ext + 8].try_into().ok()?) as usize;
    ext = ext.checked_add(8 + size)?;
}
Some(paths)
```

**AUT-08 [LOW-MEDIUM] Ένα αρχείο ρυθμίσεων του Project Backups που δεν διαβάζεται μηδενίζει όλη τη λίστα projects** (S3-10) · P
- **Location:** `backend/src/project_backups/store.rs:146-154`.
- **Root cause:** το αρχείο μετονομάζεται, επιστρέφονται defaults και το επόμενο `store::edit` αποθηκεύει κενή λίστα. Αυτό συμβαίνει π.χ. σε downgrade, όπου το παλιό build συναντά άγνωστο `CloudProvider`, και το memory του project λέει ρητά ότι τα παλιά builds διαβάζουν αυτά τα δεδομένα. Το Game Saves το χειρίζεται σωστά (`game_saves/settings.rs:37-38`).
- **Fix:** `Err(format!("The Project Backups settings could not be read ({error}). They were left unchanged at {}.", path.display()))`.

**AUT-09 [LOW] Το `project_backups_relocate_projects` παρακάμπτει τον έλεγχο του `check_project`** (S3-11) · C
- **Location:** `backend/src/project_backups/commands.rs:1222-1260`.
- **Root cause:** επιτρέπει project μέσα στο backup root (κάθε zip περιέχει τα προηγούμενα), μη απόλυτο path, και μετακίνηση ενώ τρέχει backup.
- **Fix:**
```rust
if state.is_running() { return Err("Wait for the backup to finish.".into()); }
if !path.is_absolute() { return Err("Use a full path.".into()); }
if let Some(root) = settings.backup_root()
    && (super::walk::same_path(&path, &root) || store::inside(&path, &root)) {
    return Err("The project folder is inside the backups folder.".into());
}
```

**AUT-10 [LOW] Τα παιχνίδια για idle δεν περιορίζονται στα 32, οπότε τα 33+ παίρνουν ψεύτικα «strikes»** (S3-12) · C
- **Location:** `steam_automation/decide.rs:89`· `engine.rs:286-309`· `runner.rs:45` (`MAX_GAMES`).
- **Fix:** `FarmStep::Done if world.idle_on => world.idle_games.iter().copied().take(MAX_GAMES).collect(),`

**AUT-11 [LOW] Το cache του Steam Helper θεωρεί fresh για πάντα μια εγγραφή με χρονοσφραγίδα στο μέλλον** (S3-14) · C
- **Location:** `steam_helper/cache.rs:62`, `:81-82` (`saturating_sub`).
- **Fix:** `Some(entry) if entry.saved_at <= now && now - entry.saved_at < limit => …`

**AUT-12 [LOW] Το `parse_price` δέχεται οποιοδήποτε πλήθος ψηφίων μετά το διαχωριστικό, και το `seller_gets` είναι O(cents)** (S3-15) · P
- **Location:** `steam_automation/inventory.rs:137-143`, `:149-154`.
- **Root cause:** το `1.234€` διαβάζεται ως 334 cents, άρα λίστα πολύ κάτω από την αξία. Το Steam Guard confirmation μετριάζει τον κίνδυνο. Επιπλέον το `seller * 5` κάνει overflow πάνω από περίπου 8,6 εκατ. cents.
- **Fix:**
```rust
let cents: u32 = match cents { "--" => 0, c if c.len() == 2 => c.parse().ok()?, _ => return None };
whole.checked_mul(100)?.checked_add(cents)
// και στο list_item: άρνηση για buyer_cents > 2_000_00 (όριο του Steam) πριν από το seller_gets
```

**AUT-13 [LOW] Χαμένη ενημέρωση στο `picks.json`** (S3-16) · C
- **Location:** `steam_helper/picks.rs:107-128`.
- **Root cause:** δύο ταυτόχρονα `steam_helper_picks` κάνουν read-modify-write χωρίς lock, οπότε χάνεται η εβδομάδα ενός φίλτρου και τα παιχνίδια της προηγούμενης εβδομάδας μπορεί να ξαναβγούν.
- **Fix:** `static PICKS: tokio::sync::Mutex<()>`, κρατημένο από το `read` μέχρι το `save`.

**AUT-14 [LOW] Ακρίβεια και κόστος στο Auto Clicker** (S3-17) · C
- **Location:**
  - `autoclicker/engine.rs:23-29`: το `Condvar::wait_timeout` ακολουθεί το tick των ~15,6 ms, άρα με το ελάχιστο των 10 ms πετυχαίνει περίπου 64 cps αντί για 100.
  - `autoclicker/native.rs:178`: `WindowFromPoint` σε κάθε κίνηση του ποντικιού.
  - `native.rs:254`: pump κάθε 10 ms.
- **Fix:**
  - hit test μόνο στο `WM_LBUTTONDOWN`·
  - `timeBeginPeriod(1)`/`timeEndPeriod(1)` όταν το interval είναι κάτω από 16 ms, ή `CreateWaitableTimerExW(.., CREATE_WAITABLE_TIMER_HIGH_RESOLUTION)`·
  - `MsgWaitForMultipleObjects` στο picking loop.

**AUT-15 [LOW] Τα release builds δέχονται τα dev environment overrides** (S3) · C
- **Location:** `game_saves/engine.rs:269-274` (`MYLE_LUDUSAVI`, compiled-in path `CARGO_MANIFEST_DIR`), `game_saves/settings.rs:18-22` (`MYLE_GAME_SAVES_CONFIG`).
- **Root cause:** ένα env var του χρήστη μπορεί να αλλάξει ποιο `ludusavi.exe` τρέχει.
- **Fix:** `#[cfg(debug_assertions)]` γύρω από τα overrides.

**AUT-16 [LOW] Το `std::env::args()` κάνει panic όταν ένα argument δεν είναι έγκυρο Unicode** (S3) · C
- **Location:** `steam_automation/helper.rs:104`, `game_saves/notice.rs:98`.
- **Root cause:** με `panic="abort"` αυτό σημαίνει abort κατά το startup.
- **Fix:** `std::env::args_os()`.

**AUT-17 [LOW] Το `atomic::write` αφήνει πίσω το temp file όταν αποτύχει το πρώτο `fs::rename`** (S3) · C
- **Location:** `game_saves/atomic.rs:52`.
- **Root cause:** το guard κρατά το temp όταν το destination δεν υπάρχει.
- **Fix:** αφαίρεση του temp σε κάθε μονοπάτι αποτυχίας που δεν είναι «replace partially done».

**AUT-18 [LOW] Ο μετρητής `waiting_confirmation` του Steam engine μόνο αυξάνεται** (S3) · C
- **Location:** `steam_automation/engine.rs:356`.
- **Root cause:** αθροίζεται από run σε run, οπότε ο αριθμός που φαίνεται μεγαλώνει διαρκώς.
- **Fix:** μηδενισμός στην αρχή κάθε `run_inventory`.

### 3.5 Installer / Updater / Install Apps / GitHub Releases / Spotify Hub / App shell

**INS-01 [MEDIUM] Το elevated cleanup του Spotify Hub έχει race με junctions σε φακέλους που γράφει ο χρήστης** (S4-02) · P
- **Location:** `backend/src/spotify_hub/actions.rs:882-907`, καλείται από `:648-672`.
- **Root cause:**
  - Γίνεται έλεγχος για reparse points και μετά `takeown /R`, `icacls /T` και `Remove-Item -Recurse` του PowerShell 5.1, που ακολουθεί junctions.
  - Malware του ίδιου χρήστη μπορεί, μετά την έγκριση του UAC, να βάλει junction προς το `C:\Windows\…`, με αποτέλεσμα elevated αλλαγή owner, reset ACL και διαγραφή.
- **Fix:**
```rust
let script = format!(
 "$ErrorActionPreference='SilentlyContinue'; $sys=\"$env:SystemRoot\\System32\"; \
  foreach($p in @({literals})) {{ $i = Get-Item -LiteralPath $p -Force; \
   if ($i -and -not ($i.Attributes -band [IO.FileAttributes]::ReparsePoint)) {{ \
     & \"$sys\\takeown.exe\" /F $p /D Y | Out-Null; & \"$sys\\icacls.exe\" $p /reset /C /Q | Out-Null; \
     if ($i.PSIsContainer) {{ [IO.Directory]::Delete($p, $true) }} else {{ [IO.File]::Delete($p) }} }} }};");
// χωρίς /R και /T· το [IO.Directory]::Delete(recursive) αφαιρεί το ίδιο το junction αντί να μπει μέσα του
```

**INS-02 [MEDIUM] Setup και uninstall κλείνουν ή σκοτώνουν κάθε πρόγραμμα κάτω από τον φάκελο εγκατάστασης** (S4-03) · P
- **Location:** `backend/installer/src/processes.rs:74-102`, `:195-219`· `engine.rs:145-168`· callers `lib.rs:99`, `:211`, `ui.rs:250`, `:301`.
- **Root cause:** το `running_in` ταιριάζει κάθε process μέσα στο `dir`· μετά ακολουθεί WM_CLOSE και, 8 s αργότερα, `TerminateProcess`. Με `InstallLocation` όπως `D:\Games` ή `%LOCALAPPDATA%` (από παλιό NSIS ή `/D=`), σκοτώνονται άσχετα προγράμματα και χάνονται τα μη αποθηκευμένα δεδομένα τους.
- **Fix:**
```rust
fn ours(dir: &Path) -> Vec<PathBuf> {
    let files: Vec<String> = std::fs::read_to_string(dir.join(crate::product::INSTALL_MANIFEST)).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str().map(String::from)).collect()))
        .unwrap_or_else(crate::product::legacy_files);
    files.iter().filter(|f| f.to_ascii_lowercase().ends_with(".exe"))
        .filter_map(|f| crate::payload::relative_path(f).ok()).map(|r| dir.join(r)).collect()
}
// running_in(): ours.iter().any(|exe| shell::same_file(exe, &path)) αντί για shell::is_within(&path, &dir)
// check_dir(): άρνηση για μη κενό φάκελο που δεν περιέχει exe_name(), legacy_exe_name() ή INSTALL_MANIFEST
```

**INS-03 [MEDIUM] Το Creative Hub αντικαθιστά, και αργότερα διαγράφει, ένα αρχείο του χρήστη στο Downloads** (S4-04) · C
- **Location:** `backend/src/apps/creative.rs:181`, `:208`, `:214`, `:218`· `backend/src/download.rs:79`, `:225`.
- **Root cause:**
  - Το path είναι `%USERPROFILE%\Downloads\<name>`, με default `package.zip`.
  - Το `File::create` κάνει truncate σε υπάρχον αρχείο.
  - Σε αποτυχία ή cancel το αρχείο διαγράφεται, και επειδή μπαίνει στο `Cleanup` διαγράφεται και κατά το κλείσιμο του MYLE.
- **Fix:**
```rust
let target = work_dir.join(&resolved.file_name);
let file = if tokio::fs::try_exists(&target).await.unwrap_or(true) {
    let p = Path::new(&resolved.file_name);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..1000).map(|n| work_dir.join(format!("{stem} ({n}){ext}"))).find(|c| !c.exists())
        .ok_or("Downloads already holds too many copies of this file.")?
} else { target };
cleanup.add(file.clone());
```

**INS-04 [MEDIUM] Το XZ checksum του payload του setup δεν ελέγχεται ποτέ** (S4-05) · ✔ (διαδρομή)
- **Location:** `backend/installer/src/engine.rs:207-228`, `:318-337`· `payload.rs:60`.
- **Root cause:** το `copy_files` διαβάζει ακριβώς τα δηλωμένα bytes και δεν φτάνει ποτέ στο EOF. Το `lzma-rust2` ελέγχει CRC64, index και footer μόνο στο read που επιστρέφει 0, και το payload είναι ένα μόνο block. Ένα αλλοιωμένο `setup.exe` εγκαθιστά χαλασμένα binaries και σβήνει την παλιά, λειτουργική έκδοση.
- **Fix:**
```rust
let placed = copy_files(dir, &header, &mut reader, report)?;
let mut tail = [0u8; 1];
match reader.read(&mut tail) {          // το EOF ελέγχει CRC64, index και footer
    Ok(0) => {}
    other => { roll_back(placed);
        return Err(format!("The setup file is damaged: {}",
            other.err().map_or("trailing data".into(), |e| e.to_string()))); }
}
```

**INS-05 [LOW] Το escaping single quote του PowerShell δεν πιάνει τα τυπογραφικά εισαγωγικά** (S2-13, S3-13, S4-06) · C
- **Location:**
  - `backend/src/apps/process.rs:99-101`
  - `maintenance/runner.rs:319-321`, `game_saves/commands.rs:1820-1822` (με χρήση στο `:1718` για το exe path του scheduled task)
  - `apps/custom.rs:677`, `apps/creative.rs:532-533`, `apps/winget.rs:181`
  - `spotify_hub/actions.rs:908-910`, `installer/src/webview2.rs:66`, `:103`
- **Root cause:** το PowerShell τερματίζει τα single-quoted strings και στα U+2018 έως U+201B. Ένα profile όπως `C:\Users\O’Brien` σπάει τα scripts, και ένα path που ελέγχει επιτιθέμενος σημαίνει injection.
- **Fix:** ένα κοινό module, μοιρασμένο με το pattern `#[path]`:
```rust
pub(crate) fn ps_quote(s: &str) -> String {
    let mut out = String::from('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') { out.push(c); }
        out.push(c);
    }
    out.push('\''); out
}
```

**INS-06 [LOW] Τα arguments με κενά σπάνε στα δύο κατά το elevation** (S4-07) · C
- **Location:** `apps/process.rs:39-46`, `:69-76`· επηρεάζει το `custom.rs:739-745`.
- **Root cause:** το `Start-Process -ArgumentList 'a','b c'` ενώνει τα στοιχεία με απλά κενά, χωρίς εισαγωγικά.
- **Fix:**
```rust
fn win_quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) { return a.into(); }
    let (mut out, mut bs) = (String::from('"'), 0usize);
    for c in a.chars() { match c {
        '\\' => bs += 1,
        '"' => { out.push_str(&"\\".repeat(bs * 2 + 1)); out.push('"'); bs = 0; }
        _ => { out.push_str(&"\\".repeat(bs)); bs = 0; out.push(c); } } }
    out.push_str(&"\\".repeat(bs * 2)); out.push('"'); out
}
// -ArgumentList {ps_quote(&args.iter().map(|a| win_quote(a)).collect::<Vec<_>>().join(" "))}
```

**INS-07 [LOW] Τα προγράμματα που καλούνται με γυμνό όνομα αναζητούνται πρώτα στον φάκελο του app, που είναι εγγράψιμος από τον χρήστη** (S4-08, S2) · C
- **Location:**
  - `winget.rs:214`, `:571` (`winget`)· `custom.rs:728` (`msiexec.exe`)
  - `spotify_hub/actions.rs:600` (`powershell.exe`)· `creative.rs:267` (`taskkill`)
  - `run_elevated("powershell.exe")` στα `winget.rs:133`, `creative.rs:549`, `actions.rs:906`
  - `cleaner/health/sys.rs:507` (`reg.exe`), `:516` (`tasklist.exe`)
  - `cleaner/health/mod.rs:1122`, `:1127`, `:1186`, `:1279` (`schtasks`/`reg`), `:1175` (`sc.exe` μέσω `run_elevated`)
  - `debloat/mod.rs:373` (`winget`)
- **Root cause:** το `Command` του Rust ψάχνει πρώτα στον φάκελο του exe. Ένα φυτεμένο exe μπορεί έτσι να πάρει και UAC prompt. Αυτό έρχεται σε αντίθεση με τον κανόνα στο `process.rs:10-16`.
- **Fix:** `process::system32("…")` / `process::powershell()`, και το winget από `%LOCALAPPDATA%\Microsoft\WindowsApps\winget.exe` (όπως ήδη κάνει το `github_releases/commands.rs:2082`).

**INS-08 [LOW] Το rollback του installer δοκιμάζει κάθε αρχείο μόνο μία φορά** (S4-09) · P
- **Location:** `installer/src/engine.rs:414-421`· sweep στα `:439-455` (καλείται στο `:205`)· `updater.rs:440-473`.
- **Root cause:** ένας antivirus που κρατά ένα αρχείο αφήνει την εγκατάσταση με μείγμα εκδόσεων, και στην επόμενη εκκίνηση το `*.myle-old` διαγράφεται.
- **Fix:** `rename_patiently` και `remove_patiently` μέσα στο `roll_back`.

**INS-09 [LOW] Races στο cancel του Install Apps, και kill με PID αντί για Job Object** (S4-10) · C
- **Location:** `winget.rs:481-503`, `:566-600`· `custom.rs:732-737`· `jobs.rs:52-64`· `spotify_hub/runner.rs:63-118`.
- **Root cause:**
  - Ένα cancel ανάμεσα σε attempts χάνεται.
  - Ένα σφάλμα ανάγνωσης επιστρέφει πριν από το `wait`, χωρίς `kill_on_drop`, και αφήνει orphan process.
  - Το `taskkill` με PID μπορεί να πετύχει άλλο process αν το PID έχει επαναχρησιμοποιηθεί.
- **Fix:**
```rust
for attempt in &ATTEMPTS {
    if job.is_cancelled() { return Ok(JobOutcome::Cancelled); }
    /* … */
}
let mut child = cmd.kill_on_drop(true).spawn().map_err(spawn_error)?;
let jobobj = JobObject::new().filter(|j| child.raw_handle().is_some_and(|h| j.assign(h as _)));
job.set_pid(child.id());
if job.is_cancelled() { if let Some(j) = &jobobj { j.kill(); } }
```

**INS-10 [LOW] Portable και zip apps τρέχουν χωρίς έλεγχο ακεραιότητας όταν λείπει digest· το activation δεν παρακολουθείται** (S4-11) · C
- **Location:** `custom.rs:571-572` (`IntegrityRequirement::None`), `:497-510`, `:893-934`· `creative.rs:215`, `:274-276`.
- **Root cause:**
  - Το `VencordInstallerCli.exe`, το `bdcli.exe` και το WandEnhancer εκτελούνται χωρίς έλεγχο ακεραιότητας.
  - Το `apps_activate` δεν περνά από το `Jobs`: δεν έχει timeout ούτε cancel.
- **Fix:** υποχρεωτικό digest ή pinned publisher για κάθε εγγραφή που εκτελείται, και `jobs.start(&id)` συν `tokio::time::timeout` στο `run_activation`.

**INS-11 [LOW] Το cancel ενός release-asset upload που έχει κολλήσει δεν κάνει τίποτα** (S4-12) · C
- **Location:** `github_releases/github.rs:540-551`.
- **Fix:**
```rust
let client = reqwest::Client::builder().user_agent(USER_AGENT)
    .connect_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())?;
let result = tokio::select! {
    r = send(/* … */) => r,
    _ = async { while !cancel.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(200)).await } }
        => return Err(CANCELLED.into()),
};
```

**INS-12 [LOW] Τα git hooks και helpers κληρονομούν το GitHub token** (S4-13) · C
- **Location:** `github_releases/git.rs:130-143`, `:167-171`· `ops.rs:51-86`.
- **Root cause:** το `GIT_CONFIG_VALUE_0` (`x-access-token:<PAT>`) βρίσκεται στο environment ολόκληρου του git run. Ένα husky ή pre-push hook του repository μπορεί να το διαβάσει.
- **Fix:** `-c core.hooksPath=NUL` στα quiet fetch, και ένας one-shot credential helper για το push.

**INS-13 [LOW] Τα diffs πηγαίνουν σε τρίτους AI providers χωρίς redaction** (S4-14) · C
- **Location:** `github_releases/commands.rs:861-918`· `ai.rs:105`.
- **Root cause:**
  - Εξαιρούνται μόνο τα lockfiles· τα `.env`, keys και certificates στέλνονται στο Groq, στο Gemini κ.λπ.
  - Το «Nothing leaves this PC» για το Ollama είναι ψευδές όταν το URL δείχνει σε άλλο host.
- **Fix:**
  - pathspecs `":(exclude)**/.env*"`, `":(exclude)**/*.pem"`, `":(exclude)**/*.key"`, `":(exclude)**/*.pfx"`·
  - redaction των patterns `ghp_|github_pat_|sk-|AKIA`·
  - το κείμενο απορρήτου να εξαρτάται από τον host του Ollama.

**INS-14 [LOW] Τα synced paths του GitHub Releases μπορεί να είναι UNC** (S4-15) · P
- **Location:** `github_releases/sync.rs:22`.
- **Root cause:** το `is_dir()` ή το git σε `\\host\share` μπορεί να διαρρεύσει NTLM hash και να εκτελέσει repo config που ελέγχει επιτιθέμενος.
- **Fix:** άρνηση `Prefix::UNC` και `VerbatimUNC` στο `key()`, ή υποχρεωτική επιβεβαίωση από τον χρήστη.

**INS-15 [LOW] Ένα live update που αποτυγχάνει αφήνει το παλιό app χωρίς single-instance lock** (S4-16) · C
- **Location:** `updater.rs:486-511`.
- **Root cause:** το `single_instance::destroy` γίνεται πριν φανεί ότι η νέα έκδοση άνοιξε. Αν η νέα έκδοση κλείσει (`Exited`), ένα δεύτερο launch δημιουργεί δεύτερο instance στο ίδιο WebView2 profile και στα ίδια αρχεία δεδομένων.
- **Fix:** στο `Exited`, ενημέρωση του χρήστη και `app.exit(0)` ή restart από το παλιό path.

**INS-16 [LOW] Το app και το setup κοιτάζουν διαφορετικούς φακέλους Startup** (S4-17) · C
- **Location:** `startup.rs:70-73` (`%APPDATA%`) έναντι `installer/src/shell.rs:73` (`FOLDERID_Startup`).
- **Fix:** `SHGetKnownFolderPath(FOLDERID_Startup)` και στο `startup.rs`.

**INS-17 [LOW] Το μέγεθος του παραθύρου αγνοεί το ελάχιστο μέγεθος και το DPI** (S4-18) · C
- **Location:** `window_sizing.rs:32-47` με `lib.rs:93` (`min_inner_size(800,500)` σε logical pixels).
- **Root cause:** σε 1280×720 με 150% ή 1080p με 200%, το παράθυρο ξεπερνά την περιοχή εργασίας και μπαίνει κάτω από τη γραμμή εργασιών.
- **Fix:** clamp ανάμεσα σε `800/500 × scale_factor` και στο work area, και κεντράρισμα με βάση το πραγματικό μέγεθος.

**INS-18 [LOW] Το uninstall με «remove data» αφήνει το `%LOCALAPPDATA%\MakeYourLifeEasier`** (S4-19) · C
- **Location:** `installer/src/engine.rs:636-653`· ο φάκελος γράφεται από τα `apps/mod.rs:84-90` και `spotify_hub/actions.rs:106-111`.
- **Fix:** `folders.push(local.join(product::LEGACY_BINARY));`, με guard `!shell::same_path(&folder, dir)`.

**INS-19 [LOW] Τα helper processes δεν έχουν timeout** (S4-20) · C
- **Location:** `spotify_hub/detection.rs:221-234` (`spicetify -v`)· `github_releases/commands.rs:1988-1998` (`gh auth token`)· `winget.rs:213-226` (`list`, `search`, `show`)· `custom.rs:916-921`.
- **Fix:** `tokio::time::timeout(..)` μαζί με `kill_on_drop(true)`.

**INS-20 [LOW] Ο updater δεν λέει στο installer ποιον φάκελο να ενημερώσει** (S8-05) · C
- **Location:** `updater.rs:301`, `:366` (`/S /UPDATE /LIVE`), `:331` (`/P /UPDATE /R`)· `installer/src/lib.rs:86-90` (`registry::install_dir()`).
- **Root cause:** με δύο εγκαταστάσεις ενημερώνεται ο φάκελος του registry, ενώ γίνεται restart το παλιό αντίγραφο με `--just-updated`. Ο χρήστης δεν μαθαίνει ποτέ ότι το update δεν εφαρμόστηκε εκεί που έτρεχε.
- **Fix:** `.args(["/S", "/UPDATE", "/LIVE"]).arg(format!("/D={}", exe.parent().ok_or("no install folder")?.display()))`, ως τελευταίο argument.

**INS-21 [LOW] Το `owned_setup_exit` κάνει loop για πάντα σε επαναλαμβανόμενα σφάλματα του wait, κρατώντας το exclusive job slot** (S4) · C
- **Location:** `updater.rs:389-406`.
- **Fix:** όριο στις επαναλήψεις, και fallback σε polling με `try_wait` συν έλεγχο ότι το process υπάρχει ακόμη.

**INS-22 [LOW] Κάθε τοπικό window παίρνει όλα τα commands· η απομόνωση των Steam webviews εξαρτάται από εσωτερικά του Tauri** (S8-07) · C
- **Location:** `backend/build.rs:3` (χωρίς `AppManifest`)· `backend/capabilities/default.json:5-14`· `tauri-2.11.6/src/webview/mod.rs:1823` (`|| !is_local`)· `Cargo.toml:23` (`tauri = "2"`).
- **Root cause:**
  - Τα windows splash και notice μπορούν να καλέσουν και τα 272 commands, μαζί με το `passwords_reveal`.
  - Το `core:default` δίνει `core:image:allow-from-path`, `core:menu`, `core:tray:allow-new` και `core:event:allow-emit`, που το UI δεν χρησιμοποιεί.
  - Η απομόνωση των remote Steam webviews στηρίζεται σε μια λεπτομέρεια του κλειδωμένου Tauri 2.11.6.
- **Fix:**
  - `tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(&[…])))`, με capabilities ανά window: το main παίρνει όλα, το splash μόνο τα δικά του, το notice τίποτα·
  - συγκεκριμένα core permissions στη θέση του `core:default`·
  - `tauri = ">=2.11.6, <3"`.

### 3.6 Frontend / Svelte 5

**UI-01 [MEDIUM] Το `operationGate` απελευθερώνεται πρόωρα ενώ τρέχει debloat** (S6-02) · ✔
- **Location:**
  - `frontend/lib/operation-gate.svelte.ts:5-12` (ένας owner, χωρίς μέτρηση holds)
  - `windows-optimization/state.svelte.ts:140-147` (το `applySnapshot` καλεί `end()` χωρίς όρο σε κάθε `load()`), `:34-35`
  - `windows-optimization/debloat/state.svelte.ts:79`, `WindowsOptimization.svelte:27-30`
  - backend: `debloat/mod.rs:247`, `:396` (lock μόνο στα run/undo)· όχι στα `:73` (`debloat_restore_point`) και `:487-535` (Start Menu)
- **Root cause:** δύο ανεξάρτητα κομμάτια μοιράζονται τον owner `"windows-optimization"`. Παράδειγμα:
  1. «Create a restore point» (UAC, λεπτά).
  2. Ο χρήστης πηγαίνει στο Install Apps και επιστρέφει: το gate απελευθερώνεται.
  3. Ξεκινά εγκατάσταση εφαρμογής.
  4. Το `debloat api.run` απορρίπτεται, αφού το restore point έχει ήδη γίνει.
- **Fix:** token ανά holder:
```ts
class OperationGate {
  owner = $state<OperationOwner | null>(null);
  #holds = new Set<symbol>();
  begin(owner: OperationOwner): symbol | null {
    if (this.owner !== null && this.owner !== owner) return null;
    const t = Symbol(owner); this.#holds.add(t); this.owner = owner; return t;
  }
  end(token: symbol | null | undefined) {
    if (token && this.#holds.delete(token) && this.#holds.size === 0) this.owner = null;
  }
  lockedFor(owner: OperationOwner) { return this.owner !== null && this.owner !== owner; }
}
// windows-optimization/state.svelte.ts
#backendHold: symbol | null = null;
private applySnapshot(s: WindowsOptimizationSnapshot) {
  this.snapshot = s;
  if (isAutoLogonActive(s.autoLogon) || s.firmwareRestart.active) {
    this.#backendHold ??= operationGate.begin("windows-optimization"); this.watchBackendOperation();
  } else { operationGate.end(this.#backendHold); this.#backendHold = null; }
}
readonly locked = $derived(this.ownBusy || debloat.busy || debloat.startMenuBusy || this.externallyLocked);
```
Το ίδιο pattern χρειάζεται και το Spotify Hub (`spotify-hub/state.svelte.ts:125`, `:129`, `:155`). Στο backend: exclusive lock και στο `debloat_restore_point` και στις εντολές του Start Menu.

**UI-02 [MEDIUM] Η σελίδα GitHub Releases κρασάρει όταν ένα workflow αποτύχει με δύο ίδια errors** (S6-03) · ✔
- **Location:** `frontend/app/pages/github-releases/ReleaseTab.svelte:657` (`{#each f.errors.slice(0, 5) as e (e)}`)· `backend/src/github_releases/github.rs:682-692` (`job_annotations`, χωρίς αφαίρεση διπλοτύπων).
- **Fix:** `{#each f.errors.slice(0, 5) as e, i (i)}`, και dedupe στο `job_annotations`.

**UI-03 [MEDIUM] Η λίστα του vault ξαναχτίζεται και ταξινομείται ολόκληρη σε κάθε πλήκτρο, χωρίς windowing** (S6-04) · P (δεν έγινε profiling)
- **Location:** `password-manager/state.svelte.ts:16-34`, `:117-135`· `VaultView.svelte:291-357`.
- **Root cause:**
  - Σε κάθε πλήκτρο ενώνονται όλα τα πεδία κάθε εγγραφής και γίνεται lowercase.
  - Το `localeCompare(…, {…})` καλείται περίπου n·log n φορές, φτιάχνοντας κάθε φορά νέο αντικείμενο σύγκρισης.
  - Όλες οι γραμμές γίνονται render.
  - Με 1–3 χιλιάδες logins (σύνηθες μετά από import) το πληκτρολόγιο κολλάει.
- **Fix:**
```ts
const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
#haystack = $derived(new Map(this.entries.map((e) => [e.id, [e.title, e.username, e.folder, e.notes, ...e.urls,
  ...e.apps.flatMap((a) => [a.name, a.exe]), ...e.passkeys.flatMap((k) => [k.rpId, k.userName])].join("\n").toLowerCase()])));
#sorted = $derived([...this.entries].sort((a, b) => this.sort === "recent"
  ? b.updatedAt - a.updatedAt || collator.compare(a.title, b.title)
  : Number(b.favorite) - Number(a.favorite) || collator.compare(a.title, b.title) || collator.compare(a.username, b.username)));
readonly visible = $derived.by(() => { const w = searchWords(this.query);
  return this.#sorted.filter((e) => this.#passesFilter(e) && w.every((x) => this.#haystack.get(e.id)!.includes(x))); });
```
Μετά, windowing με το `visibleRange` του `disk-analyzer/rows.ts`.

**UI-04 [LOW] Το Treemap στέλνει 2–3 περιττά requests για στατιστικά επεκτάσεων σε κάθε zoom** (S6-05) · C
- **Location:** `disk-analyzer/Treemap.svelte:402-404`· `disk-analyzer/state.svelte.ts:412-434`, `:487-498`.
- **Fix:**
```ts
$effect(() => { if (d.treemap && d.extensions === null) untrack(() => void d.loadExtensions()); });
// + στο loadExtensions: συγχώνευση πανομοιότυπων requests σε εξέλιξη, με key `${scanId}:${folder}:${sizeMode}:${generation}`
```

**UI-05 [LOW] Διπλό κλικ στο Install του installer δείχνει ψευδές σφάλμα** (S6-06) · C
- **Location:** `frontend/installer/Setup.svelte:247-265`, `:353`· backend `installer/src/ui.rs:180-191`.
- **Fix:** flag `starting` που ελέγχεται πριν από κάθε `await`, συν `disabled={starting}` στο κουμπί.

**UI-06 [LOW] Ο background έλεγχος του Game Saves «τρώει» σιωπηλά το κλικ του Restore** (S6-07) · C
- **Location:** `game-saves/state.svelte.ts:511-528`, `:605-612`, `:627`, `:638-649`, `:667-677`.
- **Fix:**
```ts
// #backgroundCheck
if (this.busy || this.discovering || this.settingsBusy || confirmState.current || this.restoreDialogOpen || this.customDialogOpen) return;
// μετά από κάθε confirm
if (this.locked) { toast.info("Game Saves is busy for a moment. Try again when the check finishes."); return; }
```

**UI-07 [LOW] Μια παλιά απάντηση του `windowsMatches()` μπορεί να αντικαταστήσει τη νεότερη** (S6-08) · C
- **Location:** `password-manager/WindowsFill.svelte:21-30`.
- **Fix:**
```ts
$effect(() => {
  const target = p.windowsTarget; void p.entries;
  if (!target) { matches = []; linking = false; return; }
  let live = true;
  api.windowsMatches().then((f) => { if (live) matches = f; }, () => { if (live) matches = []; });
  return () => { live = false; };
});
```

**UI-08 [LOW] Η κονσόλα του Spotify ξανασχεδιάζει όλο το `<pre>` σε κάθε γραμμή** (S6-09) · C
- **Location:** `spotify-hub/state.svelte.ts:293-317`· `LiveConsole.svelte:15-17`, `:29-32`.
- **Fix:** batching ανά frame, με το ίδιο pattern `#queue`/`#flush` που έχει το `system-maintenance/state.svelte.ts:231-252`.

**UI-09 [LOW] Τα state modules όλων των σελίδων φορτώνονται eagerly στην εκκίνηση** (S6-10) · C
- **Location:** `app/account/snapshot.ts:3-11`, `app/preload.ts:4-11`, `App.svelte:16-17`.
- **Root cause:** μπαίνουν στο entry bundle μαζί με το owner-only GitHub Releases.
- **Fix:** `await import(...)` μέσα στα `apply()`/`collect()`, ή κάθε σελίδα να δηλώνει δικό της reload hook.

**UI-10 [LOW] Επικαλυπτόμενοι έλεγχοι του `ghStatus()` μπορεί να καλέσουν δύο φορές το `connectGh()`** (S6-11) · P
- **Location:** `github-releases/SettingsDialog.svelte:66-79` (`setInterval(async …)`).
- **Fix:** αλυσίδα `setTimeout` που ξαναπρογραμματίζεται μόνη της, ή flag `inFlight`.

**UI-11 [LOW] Το owner change detection δεν σταματά μετά το sign-out** (S6-12) · C
- **Location:** `App.svelte:27-32`· `project-backups/state.svelte.ts:213-229`.
- **Fix:** `projectBackupsState.stopDetection()`, που καθαρίζει timer και listener και επαναφέρει το `#detecting`, κλήση όταν το `account.owner` γίνει false.

**UI-12 [LOW] Unhandled rejection μέσα σε `finally` στο Creative Hub** (S6-13) · C
- **Location:** `creative-hub/state.svelte.ts:110-121`.
- **Fix:** `.catch(() => false)` και στις δύο κλήσεις `invoke`.

**UI-13 [LOW] Το splash μένει στο «Starting…» αν αποτύχει το `finishStartup`** (S6-14) · P
- **Location:** `splash/Splash.svelte:74-98`.
- **Fix:** `await updater.finishStartup().catch((e) => showError("Couldn't open MYLE", e));` και κουμπί retry.

**UI-14 [LOW] Ο χειρισμός σφαλμάτων στο UI βασίζεται σε ταίριασμα του κειμένου τους** (S8-09) · C

| UI | Rust |
|---|---|
| `password-manager/state.svelte.ts:346` `.includes("locked")` | `vault/src/vault.rs:543…` |
| `debloat/state.svelte.ts:210` `.includes("declined")` | `elevated_pipe.rs:32` |
| `project-backups/ExclusionsPanel.svelte:53`, `ProjectEditor.svelte:83` `=== "Cancelled."` | `project_backups/mod.rs:18` |
| `steam-helper/state.svelte.ts:75` `.includes("sign in")` | `steam_automation/commands.rs:91`, `library.rs:20` |
| `mobile/updater.svelte.ts:74` `"allow-installs"` | `MyleMobilePlugin.kt:151` |

- **Root cause:** σήμερα ταιριάζουν όλα, αλλά μια αλλαγή διατύπωσης στο Rust σπάει το UI χωρίς κανένα compile error. Επιπλέον, το os error 33 περιέχει τη λέξη «locked» και θα έδειχνε το vault κλειδωμένο.
- **Fix:** `#[derive(Serialize)] struct CmdError { code: &'static str, message: String }` και switch στο `code` από το TypeScript.

**UI-15 [LOW] Keyed `{#each}` πάνω σε strings από τον χρήστη ή από απομακρυσμένα δεδομένα: η ίδια κατηγορία crash με τα BUG-05 και UI-02** (S6) · C
- **Location:** `spotify-hub/PurgeDialog.svelte:96`, `:105`, `:112`· `github-releases/SettingsDialog.svelte:309`, `:434`· `GithubReleases.svelte:237`· `ProjectPanel.svelte:245`· `installer/Setup.svelte:510`.
- **Fix:** key με id ή index. Καλό θα ήταν ένα lint script ή test που κάνει grep για `as (\w+) \(\1\)`.

**UI-16 [LOW] Το Auto Clicker χρησιμοποιεί getters αντί για `$derived`** (S6) · C
- **Location:** `auto-clicker/state.svelte.ts:27-29` (`active`, `busy`, `validation`).
- **Root cause:** το `validateConfig` ξανατρέχει σε κάθε ανάγνωση.
- **Fix:** `readonly validation = $derived(validateConfig(this.config));`.

**UI-17 [LOW] Το page state του GitHub Releases και του Project Backups δεν μηδενίζεται στο sign-out** (S6) · C
- **Root cause:** paths και status projects του owner μένουν στη μνήμη. Ο κίνδυνος είναι μικρός, αφού το backend αρνείται τα commands.
- **Fix:** `reset()` ανά σελίδα, που καλείται από το account state.

### 3.7 Extension / Mobile

**MOB-01 [MEDIUM] Στο iOS το quick unlock ανοίγει και με τον κωδικό της συσκευής, και επιβιώνει όταν αλλάξουν τα βιομετρικά** (S5-02) · C
- **Location:** `backend/mobile/src/unlock.rs:7`, `:91`, `:118`· `tauri-plugin-biometry` 0.2.8 `ios/Sources/BiometryPlugin.swift:192-199` (`.userPresence`).
- **Root cause:**
  - Με `.userPresence`, όποιος ξέρει τον κωδικό του iPhone ακυρώνει το Face ID, βάζει τον κωδικό και ανοίγει το vault.
  - Επειδή δεν υπάρχει `.biometryCurrentSet`, ακόμη και ένα πρόσωπο που γράφτηκε μετά την ενεργοποίηση ανοίγει το vault.
  - Το `confirm` του MYLE έχει `allow_device_credential: Some(false)` (`unlock.rs:156`), άρα η πρόθεση ήταν σαφώς μόνο βιομετρικά.
- **Fix:** δική του αποθήκευση στο Keychain μέσα στο `MyleMobilePlugin.swift`:
```swift
@objc public func putUnlockKey(_ invoke: Invoke) throws {
  let a = try invoke.parseArgs(SetDataArgs.self)
  guard let access = SecAccessControlCreateWithFlags(nil,
        kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, .biometryCurrentSet, nil) else {
    return invoke.reject("Access control failed") }
  let base: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
    kSecAttrService as String: a.domain, kSecAttrAccount as String: a.name]
  SecItemDelete(base as CFDictionary)
  var add = base; add[kSecValueData as String] = Data(a.data.utf8); add[kSecAttrAccessControl as String] = access
  let s = SecItemAdd(add as CFDictionary, nil)
  s == errSecSuccess ? invoke.resolve() : invoke.reject("Keychain error \(s)")
}
```

**MOB-02 [MEDIUM] Ο mobile updater δεν ελέγχει το package name και τον υπογράφοντα του APK** (S5-03) · C
- **Location:** `backend/mobile/src/update.rs:22`, `:127-138`, `:146`, `:200-204`, `:222-231`· `MyleMobilePlugin.kt:132-167`.
- **Root cause:** το feed, το APK και το SHA-256 έρχονται από τον ίδιο host. Ένα look-alike package (`uk.thomast.myle.passwordz`) εγκαθίσταται ως **νέα** εφαρμογή, χωρίς σύγκρουση υπογραφής, και ζητά τον master password.
- **Fix:** Ed25519-signed feed (όπως στο SEC-02), συν:
```kotlin
val pm = activity.packageManager
val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES
            else @Suppress("DEPRECATION") PackageManager.GET_SIGNATURES
val apk = pm.getPackageArchiveInfo(file.path, flags)
if (apk?.packageName != activity.packageName) { invoke.reject("The download is not MYLE Passwords."); return@runOnUiThread }
if (Build.VERSION.SDK_INT >= 28) {
  val mine = pm.getPackageInfo(activity.packageName, flags).signingInfo?.apkContentsSigners?.toSet()
  val theirs = apk.signingInfo?.apkContentsSigners?.toSet()
  if (mine.isNullOrEmpty() || mine != theirs) { invoke.reject("The download is not signed by MYLE."); return@runOnUiThread }
}
```

**MOB-03 [MEDIUM] Το clipboard του mobile δεν σημαδεύει τους κωδικούς ως ευαίσθητους, και το καθάρισμα στα 30 s δεν τρέχει όσο το app είναι στο background** (S5-04) · C / P
- **Location:** `backend/mobile/src/clipboard.rs:19-37`· plugin `ClipboardPlugin.swift:21-24`, `ClipboardPlugin.kt:95-105`.
- **Root cause:**
  - Στο iOS λείπουν `.localOnly` και `.expirationDate`, οπότε ο κωδικός πηγαίνει σε Mac και iPad μέσω Universal Clipboard.
  - Στο Android λείπει το `EXTRA_IS_SENSITIVE`, οπότε φαίνεται στο preview της αντιγραφής και στο ιστορικό του Gboard.
  - Το `tokio::sleep` δεν τρέχει όσο το process είναι suspended.
  - Το καθάρισμα γράφει `""` αντί για `clear()`.
- **Fix:**
```kotlin
@Command fun copySecret(invoke: Invoke) {
  val text = invoke.parseArgs(CopyArgs::class.java).text
  val clip = ClipData.newPlainText("", text)
  clip.description.extras = PersistableBundle().apply {
    putBoolean(if (Build.VERSION.SDK_INT >= 33) ClipDescription.EXTRA_IS_SENSITIVE else "android.content.extra.IS_SENSITIVE", true) }
  (activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
  invoke.resolve()
}
```
```swift
@objc public func copySecret(_ invoke: Invoke) throws {
  let a = try invoke.parseArgs(CopyArgs.self)
  UIPasteboard.general.setItems([["public.utf8-plain-text": a.text]],
    options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(30)])
  invoke.resolve()
}
```

**MOB-04 [MEDIUM] Δεν υπάρχει FLAG_SECURE ή native κάλυμμα για τα screenshots του app switcher** (S5-05) · P (δεν μετρήθηκε σε συσκευή το πότε γίνεται το snapshot)
- **Location:** `frontend/mobile/MobileApp.svelte:25-26`, `:56-60`, `:151`· `MyleMobilePlugin.kt:45-49`· το Swift plugin δεν έχει lifecycle hook.
- **Root cause:**
  - Υπάρχει μόνο ένα CSS blur στο `visibilitychange`, που συμβαίνει **μετά** το snapshot των Recents.
  - Λείπει το `FLAG_SECURE`, οπότε screen recording και casting καταγράφουν κωδικούς που φαίνονται.
  - Το blur καλύπτει μόνο το `main`, όχι τα SettingsSheet, ConfirmHost και Toaster.
- **Fix:**
```kotlin
activity.runOnUiThread {
  if (Build.VERSION.SDK_INT >= 33) activity.setRecentsScreenshotEnabled(false)
  else activity.window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
}
```
```swift
override func load(webview: WKWebView) {
  NotificationCenter.default.addObserver(forName: UIApplication.willResignActiveNotification, object: nil, queue: .main) { _ in
    guard let w = keyWindow(), w.viewWithTag(0x4D594C45) == nil else { return }
    let v = UIVisualEffectView(effect: UIBlurEffect(style: .dark)); v.frame = w.bounds; v.tag = 0x4D594C45; w.addSubview(v) }
  NotificationCenter.default.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) { _ in
    keyWindow()?.viewWithTag(0x4D594C45)?.removeFromSuperview() }
}
```

**MOB-05 [MEDIUM] Τα session tokens και το vault μεταφέρονται σε device-to-device transfer και σε τοπικά backups** (S5-06) · P (το `gen/` δεν είναι committed)
- **Location:** `backend/mobile/plugin/android/src/main/AndroidManifest.xml:7`· biometry plugin `AndroidManifest.xml:4-5` και `res/xml/backup_rules.xml:5`, `:9`· `backend/mobile/src/account.rs:26`, `:87-91`, `:147-157`· `.github/workflows/mobile.yml:148-151`, `:191-194`.
- **Root cause:**
  - Με targetSdk ≥ 31, το `allowBackup=false` δεν απενεργοποιεί το D2D transfer.
  - Τα `dataExtractionRules` του plugin εξαιρούν λάθος domain.
  - Το `account.json` (Supabase tokens σε **plaintext JSON**), το `passwords.vault` και το `passwords-biometry.json` μεταφέρονται.
  - Στο iOS ο φάκελος Application Support μπαίνει στα backups, και ένα μη κρυπτογραφημένο Finder backup περιέχει το refresh token.
- **Fix:**
```xml
<application tools:replace="android:dataExtractionRules,android:fullBackupContent"
  android:dataExtractionRules="@xml/myle_backup" android:fullBackupContent="false">
<!-- res/xml/myle_backup.xml -->
<data-extraction-rules>
  <cloud-backup><exclude domain="root" path="."/><exclude domain="file" path="."/></cloud-backup>
  <device-transfer><exclude domain="root" path="."/><exclude domain="file" path="."/></device-transfer>
</data-extraction-rules>
```
Στο iOS: `isExcludedFromBackup = true` στον φάκελο δεδομένων. Ακόμη καλύτερα, τα tokens στο Keychain ή Keystore.

**MOB-06 [LOW-MEDIUM] Όταν αλλάξουν τα βιομετρικά στο Android, η ακύρωση του κλειδιού δεν χειρίζεται** (S5-07) · C
- **Location:** `backend/mobile/src/unlock.rs:61-73`, `:111-126`, `:142-149`· `BiometryPlugin.kt:185`, `:330-338`, `:362-380`, `:537-539`.
- **Root cause:**
  - Το `KeyPermanentlyInvalidatedException` εμφανίζεται ως γενικό σφάλμα, ενώ το `hasData` λέει ακόμη `enabled`, οπότε το quick unlock αποτυγχάνει σε κάθε άνοιγμα.
  - Το `status` αναφέρει διαθέσιμο ακόμη και με `BIOMETRIC_WEAK`, αλλά το κλειδί απαιτεί STRONG.
- **Fix:**
```rust
let text = match blocking(move || store::take(&a, &i, "Open your password vault")).await {
    Ok(t) => t,
    Err(e) if ["permanently invalidated", "No key pair found", "-25300"].iter().any(|m| e.contains(m)) => {
        let _ = crate::account::remove_private(&file);
        let _ = blocking(move || store::remove(&app, &id)).await;
        return Err("Your fingerprints or face changed. Use the master password, then turn this on again.".into());
    }
    Err(e) => return Err(e),
};
```

**MOB-07 [LOW] Το CSV σε απλό κείμενο για το Apple Passwords μπορεί να μείνει πίσω** (S5-08) · C
- **Location:** `backend/mobile/src/passwords.rs:457-479`· `lib.rs:33-42`.
- **Root cause:** αν το app σκοτωθεί όσο είναι ανοιχτό το share sheet, το αρχείο μένει στο `Caches/export`. Στο Android γράφεται ολόκληρο και μετά αποτυγχάνει.
- **Fix:** `remove_dir_all(app_cache_dir/export)` στο setup, και guard `cfg!(target_os = "ios")` στην πρώτη γραμμή.

**MOB-08 [LOW] Ο έλεγχος του path της εγκατάστασης update είναι λεξικός, και το `Update` που δίνει η σελίδα δεν ξαναελέγχεται** (S5-09) · C
- **Location:** `update.rs:144-152`, `:162-163`, `:211-215`.
- **Fix:**
```rust
let real = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
let inside = std::fs::canonicalize(update_dir(&app)?).map_err(|e| e.to_string())?;
if !real.starts_with(&inside) || apk_version(&real).is_none() { return Err("That is not a downloaded update.".into()); }
if !update.url.starts_with("https://") || expected.as_deref().is_none_or(str::is_empty) {
    return Err("The update has no checksum.".into());
}
```
Επιπλέον, όριο μεγέθους ίσο με `update.size` συν ένα περιθώριο.

**MOB-09 [LOW] Το sign-out σταματά εντελώς αν αποτύχει η αφαίρεση του βιομετρικού κλειδιού** (S5-10) · C
- **Location:** `backend/mobile/src/account.rs:358`· `unlock.rs:98-109`, `:187-197`.
- **Fix:**
```rust
if let Err(e) = crate::unlock::passwords_hello_disable(app.clone(), passwords.clone()).await {
    eprintln!("biometric cleanup failed: {e}");
    remove_private(&state.dir.join("passwords-biometry.json"))?;
}
```

**MOB-10 [LOW] Οποιαδήποτε εφαρμογή μπορεί να στείλει το OAuth callback: DoS του sign-in και κείμενο σφάλματος της επιλογής της** (S5-11) · C
- **Location:** `backend/mobile/src/account.rs:197-236`, `:246-261`· `Info.ios.plist:5-15`· `frontend/mobile/account.svelte.ts:57`.
- **Root cause:** το `finish()` καταναλώνει τον verifier πριν κάνει parse το callback. Το PKCE εξακολουθεί να αποτρέπει την κλοπή του code.
- **Fix:**
```rust
let code = match callback(url) { Callback::Code(c) => c,
    Callback::Error(_) => return Err("The sign-in did not finish. Try again.".into()) };
let pending = state.peek_pending().ok_or("That sign-in is over. Tap Sign in again.")?;
let session = supabase::exchange_code(&client()?, &code, &pending.verifier).await?;
state.drop_pending();   // μόνο μετά την επιτυχία
```
Αφαιρέστε επίσης το `CFBundleURLTypes` στο iOS (το `ASWebAuthenticationSession` δεν το χρειάζεται).

**MOB-11 [LOW] Στο iOS, αντικατάσταση ή ακύρωση ενός sign-in session μπορεί να αφήσει μια κλήση να περιμένει για πάντα** (S5-12) · P
- **Location:** `MyleMobilePlugin.swift:29-53`, `:85-91`· `account.rs:317-319`.
- **Fix:**
```swift
private var pending: Invoke?
// signIn, πριν από το νέο session:
self.pending?.reject("cancelled"); self.session?.cancel()
let session = ASWebAuthenticationSession(url: url, callbackURLScheme: args.callbackScheme) { [weak self] cb, err in
  guard let self = self, self.pending === invoke else { return }   // παλιό session: αγνοείται
  self.session = nil; self.pending = nil
  /* resolve/reject όπως πριν */ }
self.pending = invoke
// cancelSignIn: self.pending?.reject("cancelled"); self.pending = nil; self.session?.cancel(); self.session = nil
```

**MOB-12 [LOW] Το scan QR συνεχίζει ενώ κλειδώνει το vault** (S5-13) · C
- **Location:** `frontend/mobile/camera.svelte.ts:19-32`· `MobileApp.svelte:56-73`· `mobile.css:162-169`.
- **Fix:** `if (scanner.active) scanner.stop();` στο `p.lock()` και στο event `passwords-locked`.

**MOB-13 [LOW] Το wrap του device key δεν δεσμεύει το `vault_id` στο AAD, παρότι η τεκμηρίωση λέει ότι το κάνει** (S5) · C
- **Location:** `backend/src/passwords/hello.rs:18` (`"myle-vault|hello"`)· `backend/mobile/src/unlock.rs:21` (`"myle-biometry-v1"`)· `docs/android-credentials.md:22`.
- **Fix:** κοινό `wrap_device_key(prefix, vault_id, …)` με AAD της μορφής `"{prefix}|{vault_id}"`, και migration κατά το επόμενο unlock.

**MOB-14 [LOW] Μικρότερα ζητήματα του mobile** (S5) · C
- Το `mobile_leave` (`lib.rs:16-18`) καλεί `app.exit(0)` χωρίς να κλειδώσει πρώτα, οπότε τα zeroizing drops δεν τρέχουν.
- Το `unlock.rs:47-51`, `:63` χρησιμοποιεί `cell.with`, που ανανεώνει το idle timer· πρέπει `with_quiet`.
- Το `write_private` (`account.rs:147-166`) δεν κάνει fsync πριν από το rename.
- Το `docs/android-credentials.md` περιγράφει providers που έχουν αφαιρεθεί.
- **Fix:** lock πριν από το exit· `with_quiet`· `sync_all()` πριν από το rename· ενημέρωση ή αφαίρεση του doc.

### 3.8 CI/CD / Build / Scripts

**CI-01 [MEDIUM] Το `contents: write` ισχύει για όλο το workflow και το token μένει στο `.git/config`** (S7-03) · C
- **Location:** `release.yml:24-25` με `:41`· `mobile.yml:37-38` με `:60`, `:111`, `:218`, `:310`.
- **Root cause:** το gradle, το brew, το npm και το cargo μπορούν να διαβάσουν το token και να κάνουν push στο `main`.
- **Fix:**
```yaml
permissions: { contents: read }
jobs:
  publish:
    permissions: { contents: write }
    steps:
      - uses: actions/checkout@<sha>
        with: { persist-credentials: false }
```

**CI-02 [MEDIUM] Race μεταξύ releases: το feed μπορεί να γυρίσει σε παλιότερη έκδοση** (S7-04) · ✔
- **Location:** `release.yml:27-29` (`group: release-${{ github.ref }}`), `:173-195`, `:186-187` (mutable `MYLE.exe`), `:225`.
- **Root cause:** αν το `v11.0.2` και το `v11.0.1` τρέχουν παράλληλα και το δεύτερο τελειώσει τελευταίο, το R2 και το «latest» του GitHub δείχνουν την παλιότερη έκδοση. Το ίδιο κάνει και ένα re-run ενός παλιού tag.
- **Fix:**
```yaml
concurrency: { group: release-publish, cancel-in-progress: false }
# πριν από τα uploads:
$live = try { Invoke-RestMethod $env:FEED_URL -Headers @{'Cache-Control'='no-cache'} } catch { $null }
if ($live -and [version]$live.version -gt [version]$env:VERSION) { throw "R2 serves $($live.version) > $env:VERSION" }
Publish $setup.FullName "MYLE-$env:VERSION.exe" $exe   # immutable, versioned
```
Το ίδιο global group και στο `mobile.yml:40-42`.

**CI-03 [MEDIUM] Τα releases δεν τρέχουν tests** (S7-05) · C
- **Location:** `release.yml:79-82` (μόνο `npm run check`)· το `mobile.yml` δεν έχει tests.
- **Fix:**
```yaml
- name: Tests
  if: env.SKIP_CHECKS != 'true'
  working-directory: backend
  run: cargo test --locked --workspace
- if: env.SKIP_CHECKS != 'true'
  run: node --test "scripts/tests/*.test.mjs"
```
Εναλλακτικά, gate στα check-runs του CI για το SHA του tag.

**CI-04 [MEDIUM] Το `update-sidestore.mjs` έχει αποκλίνει από το `docs/sidestore.json` και το επόμενο mobile release θα αναιρέσει μια χειροκίνητη διόρθωση** (S7-06) · C
- **Location:** `scripts/update-sidestore.mjs:7-21`· `docs/sidestore.json:21-22`· `mobile.yml:408-411`· commit `7886e9c`.
- **Fix:**
```js
import { readFileSync, writeFileSync } from "node:fs";
const fresh = JSON.parse(readFileSync(process.argv[2], "utf8")).apps[0].versions[0];
if (!fresh?.sha256 || !Number.isInteger(fresh.size)) throw new Error("bad generated feed");
const file = "docs/sidestore.json";
const source = JSON.parse(readFileSync(file, "utf8"));
source.apps[0].versions = [fresh];               // versioned URL του R2 + sha256, χωρίς παλιά πεδία
writeFileSync(file, JSON.stringify(source, null, 2) + "\n");
```
Προσθέστε και test.

**CI-05 [MEDIUM] Τα actions είναι pinned σε tag και το toolchain είναι unpinned** (S7-07) · C
- **Location:** `ci.yml:24`, `:26`, `:31`, `:35`, `:100`· `release.yml:41`, `:43`, `:48`, `:50`, `:138`· `mobile.yml:60`, `:111`, `:113`, `:118`, `:123`, `:127`, `:207`, `:218`, `:232`, `:237`, `:241`, `:246` (brew xcodegen), `:297`, `:310`, `:312`.
- **Root cause:** το `dtolnay/rust-toolchain@stable` είναι branch και δεν υπάρχει `rust-toolchain.toml`. Ένα νέο clippy lint μαζί με `-D warnings` (`ci.yml:58`) μπορεί να σπάσει ένα release χωρίς καμία αλλαγή στον κώδικα.
- **Fix:** SHA pins, `backend/rust-toolchain.toml` με `channel = "1.xx.0"`, και Dependabot για `github-actions`.

**CI-06 [LOW] Το «Checks: skip» διαβάζεται από το μήνυμα του commit αντί για το annotation του tag** (S7-08) · P
- **Location:** `release.yml:73-76`, `mobile.yml:100-103`.
- **Fix:** `git fetch --force --depth=1 origin "refs/tags/$TAG:refs/tags/$TAG"` πριν από την ανάγνωση.

**CI-07 [LOW] Ο έλεγχος της υπογραφής του installer είναι ελλιπής** (S7-09) · ✔
- **Location:** `release.yml:129-133`.
- **Fix:** όπως στο SEC-02: `Status -eq 'Valid'` συν timestamp και thumbprint.

**CI-08 [LOW] Το keepalive χρησιμοποιεί το service-role key και διαβάζει γραμμές χρηστών** (S7-10) · C
- **Location:** `.github/workflows/keepalive.yml:1-15`, `:28-29`, `:47-50`.
- **Root cause:** το service-role key παρακάμπτει το RLS. Επιπλέον, το GitHub απενεργοποιεί τα cron workflows μετά από 60 ημέρες χωρίς δραστηριότητα στο repository.
- **Fix:** `permissions: {}`, anon key και ένα `SECURITY DEFINER` RPC `keepalive()`· αφαίρεση του GET στο `user_settings`.

**CI-09 [LOW] Headers του site** (S7-11) · C / P
- **Location:** `site/_headers:1-11`.
- **Root cause:** το `Cache-Control` συγχωνεύεται από επικαλυπτόμενους κανόνες, και λείπει το HSTS (ίσως ορίζεται στο zone).
- **Fix:** `! Cache-Control` στο `/assets/*`, και `Strict-Transport-Security: max-age=31536000; includeSubDomains`.

**CI-10 [LOW] Ένα mobile release εμφανίζεται ως «Current release» στο site** (S7-12) · C
- **Location:** `site/assets/releases.js:57-58`, `:62`.
- **Fix:** `data.filter((r) => /^v\d/.test(r.tag_name) && !r.draft && !r.prerelease && r.published_at)`.

**CI-11 [LOW] Το CI ακυρώνει runs στο `main`, δεν έχει `timeout-minutes` και κανένα audit dependencies** (S7-14) · C
- **Location:** `ci.yml:16-18`· `smoke-test-setup.ps1:208` (`Start-Process -Wait`).
- **Fix:** `cancel-in-progress: ${{ github.event_name == 'pull_request' }}`, `timeout-minutes` σε κάθε job, και `cargo deny`/`cargo audit`/`npm audit`.

**CI-12 [LOW] Ο mobile κώδικας για Android και iOS κάνει compile μόνο σε release** (S7-15) · C
- **Location:** `ci.yml:56-62`· `backend/mobile/Cargo.toml:260-265`· `mobile.yml:159-162`.
- **Fix:** ένα Linux job με `cargo check --locked -p myle-passwords --target aarch64-linux-android` (ίσως χρειαστεί NDK `cc`).

**CI-13 [LOW] Ένα placeholder HTML μπορεί να καταλήξει σε τοπικό release build** (S7-16) · C
- **Location:** `backend/installer/build.rs:129-143`· `backend/mobile/build.rs:9-19`. Το pipeline είναι ασφαλές (`build-setup.ps1:104`).
- **Fix:** το placeholder να γράφεται στο `OUT_DIR`, ή ο έλεγχος του release να ψάχνει marker του Vite (`assets/`).

**CI-14 [LOW] Ζητήματα στα scripts** (S7-17) · C
- `scripts/sign.ps1:3-4`, `:16-21`: το σχόλιο είναι παλιό (δεν υπάρχει `signCommand`) και η παράκαμψη του Ludusavi είναι dead code. Επιπλέον, το password περνά στο command line του signtool (`:36`) και φαίνεται στη λίστα processes.
- `scripts/bootstrap-game-saves.ps1:96`, `:101`: αναθέσεις στα automatic variables `$matches`/`$input`· μετονομασία σε `$entries`/`$source`.
- `scripts/tests/passkey-rp-interoperability.test.mjs:9-14`: περνά όταν τα vectors είναι `[]`· χρειάζεται `assert.ok(vectors.length >= 1)`.
- `scripts/prepare-extension.mjs:215-219`: ο έλεγχος είναι ταυτολογικός· πρέπει να συγκρίνει με το source manifest.

### 3.9 Cross-cutting

**XB-01 [MEDIUM] Το `panic = "abort"` κάνει κάθε panic να ρίχνει όλο το process και να μην τρέχει κανένα Drop rollback** (S7-13, S8-11, σημειώσεις των S3/S4) · ✔
- **Location:** `backend/Cargo.toml:104-109`, που ισχύει για όλο το workspace: app, native host, helpers, mobile και installer.
- **Root cause:**
  - Τα 42 `impl Drop` guards δεν τρέχουν, π.χ. το rollback του `DirectorySwap` στο `spotify_hub/filesystem.rs:290-296` ή το `roll_back` του installer.
  - Τα `spawn_blocking(...).await.unwrap_or_else(|_| failed)` (`browser.rs:941-943`) και τα `unwrap_or_else(|p| p.into_inner())` είναι dead code.
  - Τα BUG-01, AUT-01, AUT-16, DSK-07 και DSK-01 γίνονται έτσι crash ολόκληρης της εφαρμογής.
- **Fix:**
  - `panic = "unwind"` τουλάχιστον για το app και το `myle-setup` (μικρό κόστος σε μέγεθος), ή αλλιώς `std::panic::set_hook` που γράφει log πριν από το abort·
  - `#![deny(clippy::indexing_slicing, clippy::string_slice)]` στα modules που κάνουν parse εξωτερικά δεδομένα.

---
## 4. Deep Performance & Memory Optimization Plan

### 4.1 Rust backend

**A. Διαχωρισμός async και blocking (`spawn_blocking`)**

| Σημείο | Πρόβλημα | Ενέργεια |
|---|---|---|
| `game_saves/commands.rs:1249`, `:869`, `:900`, `:974`, `:875`, `:931`, `:883-889`, `:570` | scan, verify, safety copy και scan cache γίνονται πάνω στα tokio workers | `spawn_blocking` (AUT-04) |
| `cleaner/health/mod.rs:617-735`, `:908-1133`, `:826-839` | `SHFileOperationW`, `reg.exe`, `schtasks`, `tasklist` στα workers | `spawn_blocking`· χρειάζεται και για το STA COM του BUG-03 |
| `passwords/browser.rs:1508-1621` (`passkey_reply`), `passwords/mod.rs:115` (lock watcher) | std mutex και file I/O στα workers· περιμένουν όσο τρέχει το Argon2 (~0,5 s) | `spawn_blocking`, ή `try_lock` στο watcher |
| `backend/mobile/src/update.rs:148-158`, `:173-196`· `passwords.rs:470-473` | `std::fs` και SHA-256 ενός APK ~30 MB σε worker | `tokio::fs`, ή hashing σε `spawn_blocking` |
| `backend/mobile/src/passwords.rs:64-72`, `:125` | το vault mutex μένει κλειδωμένο σε όλο το Argon2· τα async commands και το watch loop μπλοκάρουν | derive έξω από το lock (vault crate)· `try_lock` στο loop |
| `steam_automation/engine.rs:523-533` | το `keep_going` διαβάζει όλα τα `Apps\*` registry keys πριν από κάθε listing | cache για λίγα δευτερόλεπτα, ή ανάγνωση μόνο των `Running=1` |
| `apps/mod.rs:177-180`, `spotify_hub/mod.rs:174-182` | blocking `taskkill` μέσα σε `#[tauri::command(async)]` | `spawn_blocking` / `tokio::process` |

**B. Allocations, αντιγραφές και I/O buffering**

| Σημείο | Πρόβλημα | Αναμενόμενο κέρδος |
|---|---|---|
| `cloud/online_only.rs:787`, `:864` | 64 KB μηδενισμένο buffer σε κάθε κλήση `Cf*`, περίπου 4 κλήσεις ανά αρχείο (≈25 GB κίνηση μνήμης ανά 100k αρχεία) | επαναχρησιμοποιούμενο buffer ~4,2 KB ή `CF_PLACEHOLDER_INFO_BASIC`, και cache του registration ανά φάκελο: **πολλαπλάσια ταχύτερο walk** |
| `disk_analyzer/mft.rs:394` + `mod.rs:285-293` | payload, snapshot και tree ζωντανά ταυτόχρονα: RAM 2–3× το snapshot | decode από `Vec<u8>` by value (`split_off`) και drop των κομματιών κατά το χτίσιμο του δέντρου: **−50–60% peak RAM** |
| `disk_analyzer/mft.rs:601-624` | κάθε chunk 4 MB αντιγράφεται στο `pending` και μετά γίνεται drain: ένα επιπλέον memcpy όλου του `$MFT` (1–4 GB) | ανάγνωση απευθείας στην ουρά του `pending` με offset, και read-ahead thread: **περίπου ½ wall time σε NVMe** |
| `cleaner/elevated.rs:238` | μηδενισμός 64 KB σε κάθε listing | heap buffer μία φορά ανά φάκελο (λύνει και το DSK-01) |
| `cleaner/targets.rs:276-279` | `canonicalize` για κάθε αρχείο του `%TEMP%` | canonicalize του γονικού φακέλου μία φορά |
| `cleaner/health/scans.rs:1411` | ένα spawn του `tasklist.exe` ανά site ειδοποιήσεων του browser (5–10 s για 100 sites) | μία φορά ανά browser, πριν από το loop |
| `cleaner/health/scans.rs:334` | `$n += …` στο PowerShell: τετραγωνικό κόστος | `[Collections.Generic.List[string]]` |
| `disk_analyzer/mod.rs:728-733` | `Vec::contains` σε έως 10k ids (~50M συγκρίσεις) | το `HashSet` που ήδη χτίζεται δύο γραμμές πιο κάτω |
| `steam_automation/library.rs:175` | `extend` με `contains`: O(n·m) σε 10k+ licences | το υπάρχον `HashSet`: ~50 ms → <1 ms |
| `project_backups/compare.rs:467-482`, `:750-765` | κάθε ανάγνωση ξανανοίγει το zip και ξανακάνει parse το central directory | ένα ανοιχτό `ZipArchive` ανά πλευρά |
| `project_backups/incremental.rs:313` | ξαναδιαβάζει το listing του base zip σε κάθε increment | επαναχρησιμοποίηση του cache `changes::LISTINGS` |
| `project_backups/changes.rs:286` | κάθε έλεγχος κάνει walk όλου του project για fingerprint | παράλειψη του walk όταν ο watcher δεν είδε events: **~0 CPU για projects σε αδράνεια** |
| `vault/src/vault.rs:599-626` (`summaries()`), `browser.rs:1321-1324` | κάθε browser request υπολογίζει strength και reuse για **όλες** τις εγγραφές (το Save δύο φορές) | `entry_meta(id)` / `logins_for(host)`: O(1) αντί για O(n) |
| `vault/src/vault.rs:1253-1259` | `to_vec_pretty` ολόκληρου του vault σε κάθε save | `to_vec`: −20–30% μέγεθος και χρόνος |
| `game_saves/covers.rs:25-55` | έως 5.000 covers ως base64 `data:` URLs σε ένα IPC reply (δεκάδες MB JSON) | file paths μέσω asset protocol (`convertFileSrc`) |
| `backend/mobile/src/update.rs:103-105` | `format!` ανά byte στο `hex()` | `write!` σε ένα `String`, ή `hex::encode` |

**C. Locks και contention**
- Το `VaultCell` mutex μένει κλειδωμένο σε όλο το Argon2 (desktop και mobile). Αυτό, μαζί με το SEC-03 (απεριόριστες παράμετροι), είναι ο χειρότερος συνδυασμός: όλο το browser filling και το sync παγώνουν.
- `elevated_pipe.rs:298-304`: idle exit χωρίς lock (WIN-08).
- `steam_helper/picks.rs:107-128`: RMW χωρίς lock (AUT-13).
- `disk_analyzer/elevated.rs:38-58`: το session mutex μένει κλειδωμένο όσο διαρκεί το UAC και η ανάγνωση, και το cancel ελέγχεται μόνο στο `into_tree`. Ελέγξτε το flag αμέσως μετά το `ask`.

**D. Processes**
- `extension/background.js:48`: το `sendNativeMessage` ξεκινά **νέο** `MYLE.exe` σε κάθε request, μαζί με Toolhelp snapshot (`browser.rs:327-355`) και σύνδεση στο pipe. Με polling κάθε 1,5 s (`background.js:349-372`) και αναμονή έως 180 s (`content.js:1301-1335`), ένα passkey sign-in σημαίνει δεκάδες έως εκατοντάδες εκκινήσεις. Ένα μόνιμο `runtime.connectNative` port με reconnect γλιτώνει περίπου 50–150 ms και ένα process ανά request.
- `installer/src/processes.rs:197-199`: Toolhelp snapshot κάθε 250 ms για έως 20 s. Καλύτερα handles και `WaitForMultipleObjects`.
- `installer/src/ui.rs:146-159`: το `folder_size` περνά όλο το `EBWebView`. Καλύτερα άθροισμα από το `install.json`.
- `autoclicker/native.rs:231-257`: polling κάθε 10 ms. Καλύτερα `MsgWaitForMultipleObjects`.

**E. Μέγεθος binary και ταχύτητα compile**
```toml
# backend/Cargo.toml — ταχύτερο KDF και hashing με μικρό κόστος σε μέγεθος (εκτίμηση: 10–30% λιγότερη CPU στο unlock)
[profile.release.package.argon2]
opt-level = 3
[profile.release.package.blake2]
opt-level = 3
[profile.release.package.sha2]
opt-level = 3
```
- **Διπλά RustCrypto crates:**
  - `argon2 0.5` φέρνει `digest 0.10`, ενώ το `sha2 0.11` χρησιμοποιεί `digest 0.11`·
  - `chacha20poly1305 0.10` φέρνει `cipher 0.4`, δίπλα στο `cipher 0.5`.
  
  Η μετάβαση σε `argon2 0.6` και `chacha20poly1305 0.11` θα τα ενοποιήσει. Τα `windows-sys` 0.59/0.60/0.61 έρχονται από dependencies.
- **Features:**
  - `zip/aes-crypto` (aes, pbkdf2, hmac, sha1): μόνο αν υπάρχουν κρυπτογραφημένα Creative archives·
  - `notify/macos_fsevent`: δεν κάνει τίποτα στα Windows.
- **Fonts:** 6 αρχεία `.woff` (132 KB) ενσωματώνονται δίπλα στα `.woff2`. Δικά σας `@font-face` μόνο με woff2.
- **`frontend/public/README.md` και `sample-mark.svg`:** μπαίνουν στο bundle χωρίς λόγο.
- **CI:**
  - `shared-key: windows-release` στο rust-cache (αλλιώς το release πιάνει το debug cache)·
  - pinned toolchain·
  - το `setup` job με fat LTO να μην τρέχει σε κάθε PR.

### 4.2 Frontend & WebView2

| Περιοχή | Ενέργεια | Κέρδος |
|---|---|---|
| Λίστα vault (`VaultView.svelte`, `password-manager/state.svelte.ts`) | `Intl.Collator`, προϋπολογισμένο haystack και sort σε `$derived`, windowing με `rows.ts` (UI-03) | ομαλό πληκτρολόγιο σε 1k+ εγγραφές |
| Build log (`github-releases/state.svelte.ts:107-118`, `BuildLog.svelte:27`, `:117`) | ring buffer αντί για `slice()` 20k γραμμών ανά 120 ms, και windowing | λιγότερη πίεση στο GC και ~20k λιγότεροι DOM κόμβοι |
| Κονσόλες | batching ανά frame στο Spotify (UI-08)· cache ανά γραμμή στο `system-maintenance/output.ts:24-29`· μία κοινή κλάση console buffer | λιγότερα layouts |
| Treemap | `untrack` και συγχώνευση requests (UI-04) | ÷2–3 υπολογισμοί στατιστικών στο backend |
| Progress bar του System Cleaner (`system-cleaner/state.svelte.ts:347-353`) | CSS `transition: width` αντί για `$state` σε κάθε rAF | 0 reactive writes ανά frame |
| Startup bundle | lazy `import()` των state modules (UI-09) | μικρότερο entry chunk και λιγότερη δουλειά στο startup |
| IPC payloads | covers μέσω asset protocol (§4.1.B)· τα website icons σε `Map` χωρίς reactivity συν μετρητή έκδοσης αντί για βαθύ reactive record (`password-manager/state.svelte.ts:96`) | λιγότερη μνήμη WebView2 και λιγότερο proxy overhead |
| CSS | αφαίρεση του περιττού `filter: blur(52px)` 580×440 (`UnlockVault.svelte:242-256`)· το `App.svelte:108` να μην κάνει animate το `grid-template-columns`, αλλά transform ή clip | λιγότερα GPU και layout passes, κυρίως σε software rendering |
| Clocks | ~12 components με δικό τους `setInterval`: ένα κοινό module `clock` που χτυπά κάθε 30 s | λιγότερα timers |
| WebView2 στο tray | `ICoreWebView2_3::TrySuspend` μετά από ~30 s κρυφό όταν Jobs, Spotify και GitHub είναι σε αδράνεια, `Resume` στο show (`lib.rs:374-381`, `webview_memory.rs`) | ~0 CPU στον renderer και λιγότερο working set |
| Startup του app | έλεγχος single-instance **πριν** από τα `storage::prepare`, `note_previous_install` και το cache rename (`lib.rs:245-252`)· το `remove_dir_all(update_dir)` στο thread του sweep (`lib.rs:328`) | λιγότερο I/O σε κάθε δεύτερο κλικ |
| Exit | `Cleanup::run` στο `RunEvent::Exit` με μία μόνο προσπάθεια (`apps/mod.rs:57-65`, `:92-108`) | έως 1,2 s λιγότερο ανά κλειδωμένο αρχείο στο κλείσιμο |
| Mobile | αδιαφανές κάλυμμα αντί για `blur(18px)` στο `.away main` | χωρίς repaint μεγάλων λιστών |

---

## 5. Architecture, Code Quality & Refactoring Opportunities

### 5.1 Διπλοτυπίες που μπορεί να αποκλίνουν (και ήδη αποκλίνουν)

| Λογική | Αντίγραφα | Πρόταση |
|---|---|---|
| PowerShell quoting | `apps/process.rs:99`, `maintenance/runner.rs:319`, `game_saves/commands.rs:1820`, `custom.rs:677`, `creative.rs:532`, `winget.rs:181`, `spotify_hub/actions.rs:908`, `installer/webview2.rs:66`, `:103` | ένα module μοιρασμένο μέσω `#[path]` (INS-05) |
| PowerShell/process runners | `health/sys.rs::powershell` (χωρίς timeout), `debloat/helper.rs::run_script` (pipe χωρίς drain), `apps/process.rs::run_elevated` (encoded command line), winget `run_streaming`, Spotify `run_program` (kill με PID), git `stream_child` και build runner (Job Object) | ένας `ChildRunner` με Job Object, readers σε threads, timeout, cancel και `kill_on_drop` (λύνει τα WIN-03, DSK-05, INS-09 και INS-19) |
| Recycle | `disk_analyzer/delete.rs` (με προστασία) έναντι `cleaner/health/sys.rs::recycle` (χωρίς) | μία υλοποίηση (BUG-03) |
| Site και host matching | `browser.rs::site` (PSL), `extension/background.js::siteOf`, `vault/src/sites.rs`, `passkeys.rs:116-132` | `myle_vault::sites` και κοινά JSON test vectors (VLT-03, VLT-17) |
| Σύγκριση paths | `walk::same_path`, `store::inside`, `game_saves::settings::{overlaps, same_location, paths_equal}`, `parser::within`, `installer/shell.rs:406-438`, `spotify_hub/filesystem.rs:209-218`, github `store::same_path`, με διαφορετικούς κανόνες για κεφαλαία | ένα helper για Windows (`CompareStringOrdinal(…, TRUE)` σε canonical paths) |
| `SHGetKnownFolderPath` | `protect.rs:157`, `health/sys.rs:371`, `debloat/system.rs:531`, `cleaner/elevated.rs:195`, `cloud/detection.rs:277`, `cleaner/downloads.rs:54` | ένα wrapper |
| Helper sessions («ask και restart») | `disk_analyzer/elevated.rs:38`, `cleaner/elevated.rs:81`, `debloat/mod.rs:24` | ένα generic `HelperSession<Req, Resp>` στο `elevated_pipe` |
| `system32`, `CREATE_NO_WINDOW`, IShellLink, `JUST_UPDATED_ARG`, registry keys | `installer/shell.rs:547-564` έναντι `apps/process.rs:5-17`· `startup.rs:136-166` έναντι `installer/shell.rs:278-295`· `updater.rs:101` έναντι `installer/shell.rs:524`· `taskbar.rs:46` έναντι `startup.rs:21-23` | κοινά modules μέσω `#[path]`, όπως το `shell_identity.rs` |
| Commands των passwords (desktop και mobile) | `passwords_lock`, `set_auto_lock`, `copy`, `set_website_icons`, `history`, `totp`, `totp_check`, `generate`, `strength`, `save`, σχεδόν ίδια γραμμή προς γραμμή | συναρτήσεις του `myle_vault` πάνω σε `&VaultCell`, με λεπτά wrappers `#[tauri::command]` |
| Clipboard policy | `backend/src/passwords/clipboard.rs` (thread sleep) έναντι `backend/mobile/src/clipboard.rs` (generation counter) | ένα crate-level policy |
| OAuth callback parsing | `account/oauth.rs:78` έναντι `mobile/src/account.rs:246` | στο `myle_vault::account` |
| Wrap του device key | `hello.rs:18` έναντι `unlock.rs:21` (κανένα δεν δεσμεύει το `vault_id`) | `wrap_device_key` (MOB-13) |
| `process_path` / `program_of` | `browser.rs:299-311` έναντι `hello.rs:53-68` | μία συνάρτηση |
| Frontend `channel()` | `disk-analyzer/api.ts:147`, `github-releases/api.ts:488`, `system-cleaner/api.ts:50`, `system-maintenance/api.ts:36`, `install-apps/api.ts:54`, `installer/api.ts:57` | `lib/ipc.ts` |
| Frontend `message(error)` | ~20 αρχεία | `lib/ipc.ts` |
| Console buffers | `spotify-hub/state.svelte.ts:305-317`, `system-maintenance/state.svelte.ts:231-252` + `output.ts`, `github-releases/state.svelte.ts:558-565` | μία κλάση (caps, `\r`, batching ανά frame) |
| Locks του UI | `#releaseGate` στο Install Apps, `startingAction` στο Spotify, δύο state machines στο Windows Optimization με κοινό owner | token-based gate (UI-01) και node test |
| `atomic::write` και `free_space` | `game_saves::atomic` χρησιμοποιείται από Project Backups, Steam και Auto Clicker· `archive.rs:495` και `safety.rs:121` | μεταφορά στο `crate::storage` |

### 5.2 Error handling και IPC contract

- **Structured errors:**
  - Όλα τα commands επιστρέφουν `Result<T, String>`, και το GitHub Releases βάζει ένα `Problem` σε JSON μέσα σε `String` (`github_releases/mod.rs:134-138`).
  - Πρόταση: ένα `thiserror` enum με `Serialize` → `{ code, message }`, κοινό σε desktop, mobile και native bridge. Καταργεί το ταίριασμα κειμένου του UI-14.
- **Παραγωγή τύπων:**
  - `tauri-specta` ή `ts-rs` για commands, events και payloads. Σήμερα συμφωνούν και τα 287 invokes, τα 16 events και τα 51 channels, αλλά τίποτα δεν το εγγυάται.
  - Ένα CI check (~40 γραμμές) για commands που είναι registered αλλά δεν καλούνται και αντίστροφα. Σήμερα υπάρχει ένα dead command: `cleaner_health_apply`.
- **Πρωτόκολλα με version:**
  - native bridge: `protocol`, `features`, `unknownType`, pairing (VLT-04, VLT-16)·
  - elevated pipe: `{v, build}` (WIN-09)·
  - binary snapshot του fast scan.
- **`panic = "abort"`:** επανεξέταση (XB-01). Σε κάθε περίπτωση, clippy lints για slicing πάνω σε εξωτερικά δεδομένα και `args_os()`.
- **Περιορισμοί του server και στα clients:** όριο ciphertext, DDL και RLS του `user_settings` στο repo, CAS στα settings.

### 5.3 Dead code, μέγεθος αρχείων, docs

- **Dead code:**
  - `Vault::for_app` και `matches_name` (`vault.rs:1052-1071`, `:1395-1411`, με `#[allow(dead_code)]`)·
  - `cleaner_health_apply`·
  - το Ludusavi skip στο `sign.ps1`.
- **Πολύ μεγάλα αρχεία:**
  - `github_releases/commands.rs` (2.338 γραμμές), `release.rs` (2.224)·
  - `windows_optimization/autologon.rs` (2.279)·
  - `passwords/browser.rs` (2.196)·
  - `game_saves/commands.rs` (2.171)·
  - `cleaner/health/scans.rs` (2.088).
  
  Διάσπαση ανά feature, π.χ. Game Saves σε `restore.rs`, `schedule.rs`, `scan.rs`, ώστε το restore να μπορεί να γίνει test χωρίς Tauri.
- **Docs:**
  - collapsed module docs `// !` αντί για `//!` (`browser.rs:1`, `hello.rs:1`, `session.rs:1`, `windows_fill.rs:1`, `clipboard.rs:1`)·
  - λάθος σχόλιο για τα tombstones (`password-manager.sql:22-23`, `vault.rs:95-96`)·
  - παλιό `docs/android-credentials.md`.
- **Τεστ με το πιο αξιόλογο κόστος/όφελος:**
  1. golden αρχείο vault από παλιότερο release με τις παραμέτρους production του Argon2·
  2. contract test ανάμεσα στο `latest.json` που παράγει το `release.yml` και στο `Feed` του `updater.rs:119` (μεταφορά της παραγωγής σε `.mjs`)·
  3. test μήκους command line για τα elevated actions·
  4. test για βαθύ δέντρο στον elevated cleaner·
  5. test ότι το Health δεν διαγράφει ποτέ οριστικά·
  6. test για names με πολυbyte χαρακτήρες στο naming του Project Backups·
  7. tests για `unlock.rs`, `clipboard.rs` και `session.rs`·
  8. DACL test και test με ξένο client στο elevated pipe·
  9. node test για το operation gate·
  10. lint για keyed `{#each}` πάνω σε strings.

---

## 6. Prioritized Action Roadmap

### Phase 1: Immediate Hotfixes (ασφάλεια και αποτροπή απώλειας δεδομένων)

Μικρές και τοπικές αλλαγές, που προτείνεται να βγουν σε ένα patch release (11.0.2):

1. **BUG-01**: `strip_zip` με έλεγχο char boundary, συν test. *(1 συνάρτηση)*
2. **BUG-02**: Restore με `list_backups_checked` και `checked_chain`.
3. **BUG-03**: το Health recycle μέσω της προστατευμένης υλοποίησης `IFileOperation`· journal μετά την επιτυχία (DSK-06).
4. **BUG-04**: το `run_elevated` μέσω stdin, συν test για το μήκος του command line.
5. **BUG-05 / UI-02 / UI-15**: keys με index σε όλα τα `{#each}` πάνω σε strings· dedupe στα `clean_list` και `job_annotations`.
6. **SEC-03**: `check_kdf` σε derive, `take_header` και import· μετά, σφράγισμα του header (χρειάζεται migration στο Supabase).
7. **SEC-04**: wall clock στο away/idle lock του mobile.
8. **SEC-01 + SEC-02 (pipeline)**: secrets σε βήματα που τα κρατούν μόνο όσο τρέχουν, `environment:` με reviewers, άρνηση unsigned release, πλήρης έλεγχος Authenticode, `permissions` με ελάχιστα δικαιώματα (CI-01), global release concurrency (CI-02).
9. **SEC-02 (app)**: Ed25519 υπογραφή του feed και pinned prefix του GitHub· το ίδιο για το mobile feed (MOB-02), μαζί με τον έλεγχο package και signer.
10. **VLT-03** (sites για IPs), **VLT-05** (`logout_local`), **VLT-02** (όριο ciphertext και συνέχεια του sync μετά από μια απόρριψη).
11. **INS-03** (Downloads του Creative), **INS-04** (XZ EOF), **DSK-01** (heap buffer), **AUT-01** (`parts.get`).
12. **MOB-01** (`.biometryCurrentSet`), **MOB-05** (backup και D2D rules), **MOB-04** (`FLAG_SECURE`).

### Phase 2: Stability & Concurrency (deadlocks, races, leaks)

1. **XB-01**: απόφαση για `panic = "unwind"` (τουλάχιστον app και installer) ή `set_hook` με log, συν clippy lints για slicing.
2. Ένας **`ChildRunner`** (Job Object, drained pipes, timeout, cancel) για winget, Spotify, Health, debloat και maintenance: λύνει τα WIN-03, DSK-05, INS-09, INS-19 και INS-11.
3. **UI-01**: token-based operation gate, και exclusive lock στο backend για το restore point και το Start Menu.
4. **VLT-04 + VLT-16**: pairing token και versioned πρωτόκολλο στο native bridge· `connectNative` port.
5. **WIN-04 / WIN-08 / WIN-09**: SDDL στο pipe, έλεγχος elevation του client, idle exit κάτω από lock, envelope `{v, build}`, ένα runas primitive.
6. **WIN-02 / INS-01 / DSK-02**: κλείσιμο των TOCTOU (log handle, χωρίς recursive takeown/icacls, canonical έλεγχος του parent).
7. **VLT-06, VLT-07, VLT-08, VLT-10, VLT-12**: σωστή σήμανση header synced, υπογραφή passkey μέσα στο lock, καθάρισμα clipboard στο exit και στο lock, CAS στα settings, conflicts με το σφραγισμένο `updated_at`.
8. **AUT-02, AUT-03, AUT-06, AUT-08, AUT-09**: registry στο safety copy, ανοχή σε αρχεία που εξαφανίζονται, guard στο pagination, σφάλμα αντί για reset στο store, validation στο relocate.
9. **INS-02, INS-08, INS-15, INS-20, INS-21**: kill μόνο των δικών μας exes, rollback με επαναλήψεις, single-instance lock μόνο μετά την επιτυχία, `/D=`, όριο στο `owned_setup_exit`.
10. **WIN-01, WIN-05, WIN-06**: αυτο-καθαριζόμενο Start policy, undo για όλους τους τύπους registry, καταγραφή της προηγούμενης κατάστασης στο Recommended.
11. **MOB-06, MOB-09, MOB-10, MOB-11, MOB-12**: χειρισμός ακύρωσης κλειδιού, best-effort sign-out, σειρά στο OAuth callback, pending invoke στο iOS, stop του scanner στο lock.
12. **CI-03, CI-04, CI-05, CI-11, CI-12**: tests στο release, διόρθωση του sidestore, SHA pins και toolchain, timeouts και audit, cross-check για Android.

### Phase 3: Performance & UX polish

1. **§4.1**: `spawn_blocking` παντού (AUT-04 και ο πίνακας 4.1.A)· buffers του online-only· peak RAM και memcpy του MFT· `summaries()` O(1)· `to_vec`· covers μέσω asset protocol.
2. **§4.2**: windowing και collator στη λίστα του vault, ring buffer στο build log, batching στις κονσόλες, `untrack` στο Treemap, lazy state modules, CSS cleanup, `TrySuspend` στο tray.
3. **`opt-level = 3`** για argon2, blake2 και sha2· ενοποίηση RustCrypto· μόνο woff2· features του zip.
4. **INS-16, INS-17, INS-18, DSK-03, DSK-08, DSK-09, AUT-10 έως AUT-14, UI-04 έως UI-13**: μικρές διορθώσεις στο UX και την ακρίβεια.
5. **§5**: ενοποίηση διπλοτυπιών, structured errors, παραγωγή τύπων, διάσπαση των μεγάλων αρχείων, ενημέρωση των docs, τα 10 προτεινόμενα tests.

---

### Παράρτημα: σύνοψη αριθμών

| Κατηγορία | Πλήθος |
|---|---|
| P0/P1 (HIGH) | 9 (SEC-01…04, BUG-01…05) |
| MEDIUM | 32 |
| LOW-MEDIUM | 5 |
| LOW (μερικά συγκεντρώνουν πολλά μικρά ευρήματα του ίδιου είδους) | 89 |
| Ευρήματα που επαλήθευσα ο ίδιος στον κώδικα (✔) | 19 |
| Ευρήματα των subagents που ενώθηκαν επειδή ήταν διπλά | 9 (update signing, ps_quote ×3, oversized entry, logout scope, settings CAS, panic=abort, native bridge, bare program names) |
| JS tests | 526/526 πέρασαν |
| IPC: commands που καλούνται αλλά δεν είναι registered / mismatches ονομάτων arguments / events χωρίς αντίστοιχο | 0 / 0 / 0 |
