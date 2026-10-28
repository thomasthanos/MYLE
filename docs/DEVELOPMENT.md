# MYLE · development guide

Everything about building, the structure, each page, the updater and the setup.
For what the app does, see the <a href="../README.md"><kbd>README</kbd></a>.

**Contents**

- <a href="#1-get-started"><kbd>1 · Get started</kbd></a> <a href="#commands"><kbd>commands</kbd></a> <a href="#ci"><kbd>CI</kbd></a> <a href="#where-builds-land"><kbd>where builds land</kbd></a>
- <a href="#2-repository-map"><kbd>2 · Repository map</kbd></a> <a href="#not-in-git"><kbd>not in git</kbd></a> <a href="#versions-that-must-agree"><kbd>versions</kbd></a>
- <a href="#3-frontend"><kbd>3 · Frontend</kbd></a> <a href="#add-a-page"><kbd>add a page</kbd></a> <a href="#frontend-conventions"><kbd>conventions</kbd></a>
- <a href="#4-backend"><kbd>4 · Backend</kbd></a> <a href="#start-up-and-the-hand-over"><kbd>start-up</kbd></a> <a href="#shared-backend-modules"><kbd>shared modules</kbd></a>
- <a href="#5-the-pages"><kbd>5 · The pages</kbd></a> <a href="#install-apps"><kbd>Install Apps</kbd></a> <a href="#creative-suite"><kbd>Creative Suite</kbd></a> <a href="#spotify-hub"><kbd>Spotify Hub</kbd></a> <a href="#game-saves"><kbd>Game Saves</kbd></a> <a href="#project-backups"><kbd>Project Backups</kbd></a> <a href="#github-releases"><kbd>GitHub Releases</kbd></a> <a href="#windows-optimization"><kbd>Windows Optimization</kbd></a> <a href="#system-cleaner"><kbd>System Cleaner</kbd></a> <a href="#system-maintenance"><kbd>System Maintenance</kbd></a> <a href="#disk-analyzer"><kbd>Disk Analyzer</kbd></a> <a href="#auto-clicker--macros"><kbd>Auto Clicker</kbd></a> <a href="#password-manager"><kbd>Password Manager</kbd></a> <a href="#myle-passwords-android-and-iphone"><kbd>MYLE Passwords</kbd></a> <a href="#settings"><kbd>Settings</kbd></a>
- <a href="#6-owner-only-pages"><kbd>6 · Owner-only pages</kbd></a>
- <a href="#7-where-things-live-on-disk"><kbd>7 · Where things live on disk</kbd></a>
- <a href="#8-updates-and-releases"><kbd>8 · Updates and releases</kbd></a> <a href="#the-cloudflare-pages-site"><kbd>the site</kbd></a>
- <a href="#9-setup-and-uninstall"><kbd>9 · Setup and uninstall</kbd></a>
- [10. Tests](#10-tests)
- [11. Conventions](#11-conventions)

---

## 1. Get started

### Requirements

| | |
|---|---|
| Node.js | 20 or newer |
| Rust | stable, MSVC toolchain (`winget install Rustlang.Rustup`) |
| Visual Studio | 2022 Build Tools, `Desktop development with C++` |
| Windows | 10 or 11, with the WebView2 Runtime |

### Commands

~~~powershell
npm install
npm run dev       # the app, with hot reload
npm run build     # the whole Windows setup
npm run check     # site check, svelte-check and TypeScript
npm test          # Rust tests (workspace), then the script tests
npm run lint      # cargo clippy -D warnings
~~~

`npm run check` is `node scripts/check-site.mjs && svelte-check --tsconfig ./tsconfig.app.json && tsc -p tsconfig.node.json`.
`npm test` is `cargo test --workspace` followed by `node --test "scripts/tests/*.test.mjs"`.

Everything else runs directly:

| Command | What it does |
|---|---|
| `npx tauri dev` | The app with hot reload (what `npm run dev` runs) |
| `npx tauri build --no-bundle` | Release exe, no setup |
| `npx vite` | The app's pages in a browser (no Rust behind them); `http://localhost:1420` |
| `npx vite build` | The app's two windows → `backend/target/web/` |
| `npx vite build --mode setup` | The setup and uninstaller window → `backend/target/web-setup/` |
| `npm run web:mobile` | MYLE Passwords' page (`vite build --mode mobile`) → `backend/target/web-mobile/`; `npx vite --mode mobile` serves it at `/mobile.html` |
| `cargo run -p myle-passwords` | MYLE Passwords on Windows, to try it (no Face ID, sign-in or camera there) |
| `node scripts/prepare-extension.mjs` | Copies the extension's shared files into `extension/firefox/` and packs the store zips |
| `node scripts/update-psl.mjs` | Writes `extension/psl.js` from the `psl` crate the app uses |
| `scripts/bootstrap-game-saves.ps1` | Downloads and SHA-256-verifies the pinned Game Saves resources |
| `scripts/verify-install.ps1` | Checks an install; `-Removed` checks an uninstall |
| `npm test -- --test-only` | Not a thing: the script tests take a glob, so run `node --test "scripts/tests/*.test.mjs"` |

Tauri's `beforeDevCommand` and `beforeBuildCommand` (in `backend/tauri.conf.json`) run the Game Saves bootstrap and the extension preparation, so `npm run dev` and `npm run build` need nothing else.

### CI

`.github/workflows/`:

| File | Runs on | Does |
|---|---|---|
| `ci.yml` | every push and pull request | frontend checks, Rust lint and tests, a Windows install smoke test. Each run's setup is kept as an artifact for 7 days |
| `release.yml` | a `v*` tag, or by hand | the signed release (see [Updates and releases](#8-updates-and-releases)) |
| `mobile.yml` | a `mobile/…` branch, a tag, or by hand | the Android and iOS builds |
| `keepalive.yml` | a schedule | keeps the workflows' storage alive |

### Where builds land

Everything goes under `backend/target/`, which is ignored by git:

| Build | Output |
|---|---|
| Rust | `backend/target/debug/`, `backend/target/release/` |
| `vite build` | `backend/target/web/` |
| `vite build --mode setup` | `backend/target/web-setup/` |
| `vite build --mode mobile` | `backend/target/web-mobile/` |
| `npm run build` | `backend/target/release/bundle/setup/` |

---

## 2. Repository map

```
.github/workflows/            ci.yml, release.yml, mobile.yml, keepalive.yml
scripts/
  build-setup.ps1             the whole setup (npm run build)
  bootstrap-game-saves.ps1    download and SHA-256 verification of the pinned resources
  sign.ps1                    exe signing (release.yml)
  smoke-test-setup.ps1        silent install → update → uninstall, with checks (CI)
  verify-install.ps1          checks one install, or with -Removed an uninstall
  check-site.mjs              checks site/ (files, links, headers, stale references)
  mobile-feeds.mjs            sidestore.json and mobile-latest.json for R2
  update-psl.mjs              extension/psl.js from the psl crate's list
  update-sidestore.mjs        refreshes the SideStore source
  prepare-extension.mjs       the Firefox mirror, plus the store zips
  tests/                      37 node --test suites for everything outside Rust
site/                         the static site (Cloudflare Pages; wrangler.toml)
frontend/                     Svelte 5 + TypeScript, the Vite root
  index.html                  main window → main.ts → app/
  splash.html                 updater window → splash.ts → splash/
  installer.html              setup/uninstaller window → installer/
  mobile.html                 MYLE Passwords for phones → mobile/
  notice.html                 the Game Saves notice window → notice/
  app/App.svelte              titlebar + sidebar + content
  app/shell/                  Titlebar, Sidebar, ContentArea
  app/pages/registry.ts       ← every page is listed here
  app/pages/<page>/           one folder per page (UI, state, typed IPC)
  app/account/                Discord/Google sign-in and settings sync
  installer/, splash/, mobile/, notice/   the other windows
  lib/                        shared: toast, confirm, tooltip, context menu, updater, brand, markdown, platform, components/, icons/
  styles/                     tokens.css (colours, sizes, motion), glass.css, controls.css, base.css, background.css, fonts.css
  public/                     static files shipped with the app (icons/)
backend/                      Rust + Tauri
  src/lib.rs                  start-up, the splash → main hand-over, the command table
  src/<feature>/              account, apps, autoclicker, cleaner, cloud, debloat, disk_analyzer,
                              game_saves, github_releases, maintenance, passwords,
                              project_backups, spotify_hub, windows_optimization
  src/updater.rs              the updater (feed on R2, GitHub Releases as fallback)
  src/whats_new.rs            the "What's new" notes
  src/storage.rs              where settings and data live (and the one-time folder move)
  src/elevated_pipe.rs        the administrator helper pipe, shared by every feature that needs admin
  installer/                  setup.exe + uninstall.exe (Rust, its own Tauri window)
  vault/                      myle-vault: the Password Manager's vault, sync and account
  mobile/                     MYLE Passwords for Android and iPhone (gen/ is made by CI)
  mobile/plugin/              its native code (Kotlin, Swift)
  catalog/                    creative-apps.json, custom-apps.json (baked into the exe)
  resources/                  what ships next to the exe (Ludusavi, Spicetify)
  icons/                      app-icon.svg (the source) and the PNG/ICO built from it
  tauri.conf.json             name, version, windows, resources
  capabilities/               the Tauri capability files
extension/                    the browser extension (Manifest V3 + the Firefox manifest)
vite.config.ts                one config: `vite build` → web/, `--mode setup` → web-setup/, `--mode mobile` → web-mobile/
wrangler.toml                 the Pages project (pages_build_output_dir = "./site")
docs/                         this guide, release notes, Supabase SQL, the site's assets
```

### Not in git

From `.gitignore`:

| Ignored | Why / what makes it |
|---|---|
| `node_modules/` | npm |
| `backend/target/` | every build (Rust, Vite, the setup) |
| `backend/gen/`, `backend/installer/gen/` | Tauri's generated schema |
| `backend/resources/ludusavi/*` (except its README) | fetched and verified by `scripts/bootstrap-game-saves.ps1` |
| `extension/firefox/*` (except `manifest.json`) | copied by `scripts/prepare-extension.mjs` — the bundle and `cargo` builds need them there |
| `extension/*.zip` | the packed store packages |
| `.preview/` | the local page-preview harness and its verification scripts (see its README) |
| `.claude/`, `*.bak`, `.vscode/*` (except `extensions.json`), `.idea/`, `*.local` | tooling and editor noise |

### Versions that must agree

The app's version lives in **`package.json`** and **`backend/Cargo.toml`**, and `backend/tauri.conf.json` reads it from `package.json` (`"version": "../package.json"`). `release.yml` refuses a tag that disagrees with either.

The phones have their own: `backend/mobile/tauri.conf.json` and `backend/mobile/Cargo.toml`.

---

## 3. Frontend

Svelte 5 (runes: `$state`, `$derived`, `$props`), TypeScript, Vite 8. No SvelteKit.

`frontend/` holds five entry points, each its own window:

| Entry | Window |
|---|---|
| `index.html` → `main.ts` | the main window |
| `splash.html` → `splash.ts` | the updater's splash |
| `installer.html` → `installer/` | setup and uninstaller |
| `mobile.html` → `mobile/` | MYLE Passwords (the Password Manager page in a phone shell) |
| `notice.html` → `notice.ts` | the Game Saves notice pop-up |

### Add a page

1. Create `frontend/app/pages/my-page/MyPage.svelte`, starting with `<PageHeader title="…" subtitle="…" />`.
2. Add one entry to `app/pages/registry.ts`:

```ts
{ id: "my-page", label: "My page", group: "system", icon: SomeIcon, component: MyPage,
  description: "One line for the sidebar's tooltip." }
```

`group` is one of `apps`, `data`, `system`, `owner`, `bottom` (Settings). Use `load: () => import(...)` instead of `component` for a page that should not be in the main bundle (owner-only pages do this). `fill: true` makes the page fill the window and scroll inside itself. `numbered: false` keeps a page out of the Ctrl+number order.

3. Give it an icon in `lib/icons/` and list it in the sidebar (nothing else to touch: `lib/sidebar.ts` derives groups, order and shortcuts from the registry).

Icons come from <a href="https://lucide.dev/icons"><kbd>Lucide</kbd></a>: `import Star from "@lucide/svelte/icons/star"`.

### Frontend conventions

- **Panels.** `.surface` for a card inside a page; `.glass` for the big raised shell panels (always solid). Colours only from `styles/tokens.css`, so they follow the theme (`:root.dark` is the default).
- **State.** A page's state is a class in `state.svelte.ts`, exported as one singleton, because `ContentArea` destroys the page component on navigation and scan results must survive it.
- **IPC.** Each page has an `api.ts` with typed `invoke` wrappers and the shapes Rust sends. The page never builds a path or a command line: it sends ids, and Rust resolves everything else from its own tables.
- **Long lists** are virtualised (`DiskAnalyzer`'s folder list) or drawn to a canvas (`Treemap`).
- **Motion** is a token (`--dur-fast`, `--dur-med`, `--ease-out`), and every animation respects `prefers-reduced-motion`.
- **Text** is sentence case, buttons say what they do, and a destructive action always asks first (`.danger` in the confirm dialog).

---

## 4. Backend

`backend/` is a Cargo workspace:

| Member | What |
|---|---|
| `.` (`make-your-life-easier`) | the app: Tauri commands and every Windows feature |
| `installer` | `setup.exe` and `uninstall.exe` |
| `vault` (`myle-vault`) | the Password Manager's vault, shared with the phone app |
| `mobile` (`myle-passwords`) | MYLE Passwords for Android and iPhone |

### Start-up and the hand-over

Only the splash is created at first (`src/lib.rs`). Once it is on screen it calls `splash_shown`, and the main window starts loading hidden behind it, so the two web views do not compete for the first paint. The main window opens when both the update check is done (`finish_startup`) and the page has drawn (`main_ready`) — there is no minimum splash time. Then `myle-shown` goes out.

What the first look does not need waits for `whenShown()`: Install Apps' winget check, and the preload's second round (other pages' pictures, Game Saves covers, Spotify Hub's online update check). The first round — each page's data read from this PC, and the Install Apps icons — runs while the splash shows. If the hand-over never happens, the window opens after 12 seconds anyway.

The main window is sized for the screen the cursor is on, in physical pixels:

| Screen | Window |
|---|---|
| 4K (2160p) | 2560×1440 |
| 2K (1440p) | 1920×1080 |
| 1080p | 1280×720 |
| smaller | 90% of the usable area |

It never exceeds 95% of the area outside the taskbar (`src/window_sizing.rs`). While it is minimised or hidden in the tray, the web view is asked to keep little in memory (`src/webview_memory.rs`).

### Shared backend modules

| File | What |
|---|---|
| `src/lib.rs` | start-up, the splash hand-over, `owner_gate`, the command table |
| `src/storage.rs` | settings (roaming) and data (local) folders, and the one-time move from the 7.0.x folders |
| `src/elevated_pipe.rs` | the administrator helper pipe: one UAC prompt per session, used by the System Cleaner, the debloater and the Disk Analyzer |
| `src/console.rs` | the console output the maintenance and GitHub pages read |
| `src/download.rs` | the one downloader (progress over a channel, cancellable) |
| `src/updater.rs`, `src/whats_new.rs` | see [Updates and releases](#8-updates-and-releases) |
| `src/startup.rs`, `src/tray.rs`, `src/taskbar.rs`, `src/shortcut_preferences.rs` | Windows start-up, the tray icon, the taskbar pin |
| `src/webview_policy.rs` | the CSP and navigation rules the web views run under |
| `src/apps/` | winget, the job registry (`Jobs`), process control, the catalog |

---

## 5. The pages

### Install Apps

An app store on top of **winget**: `frontend/app/pages/install-apps/` and `backend/src/apps/`.

| File | Content |
|---|---|
| `frontend/app/pages/install-apps/data/apps.json` | the fixed list (winget ids, site, `selfUpdating`, `iconDomain`/`icon`) and the app packs |
| `backend/catalog/custom-apps.json` | the apps outside winget: resolver (`github` / `page` / `static`), install kind (`installer` / `portable` / `zip`), detection, `activate` |
| `categories.ts` | keywords for the automatic categories (override with `"category"` in the JSON) |

- **Status** (green installed, blue update, grey not installed) comes from `winget list`. Special apps are detected from the registry or a file. Anything `selfUpdating` never counts toward updates.
- **Search** looks in the list first; if nothing turns up, it searches winget live ("More from catalog"), and whatever you tick there stays **Pinned**.
- **Install via winget**, with three attempts: `--silent`, then without `--silent`, then `--scope user`.
- **A hash mismatch** asks first; agreeing briefly opens a UAC prompt to set `InstallerHashOverride`, installs, and closes it again.
- **Special apps:** the resolver finds the latest link (6-hour cache). A malformed SHA-256 is always rejected. An executable installer needs either a valid SHA-256 or a valid Authenticode signature from a publisher pinned in the catalog (e.g. NVIDIA Corporation).
- **Safety:** the UI sends ids only. URLs and command lines exist only in the JSON baked into the binary.
- **Vencord / BetterDiscord:** installing downloads the official tool (`VencordInstallerCli.exe` / `bdcli.exe`) and immediately patches Discord Stable. Status comes from the newest `app-*` folder in `%LOCALAPPDATA%\Discord\app-*\resources`; if a Discord update wipes the patch, "Patch Discord" reappears. The two declare `conflicts` with each other, so installing one asks for confirmation while the other is active.

### Creative Suite

Cards for **your own** packages (zip or installer): download with progress, extraction, running the setup, cleaning up temp files. `frontend/app/pages/creative-hub/`, `backend/src/apps/creative.rs`.

The list is only `backend/catalog/creative-apps.json`, baked into the exe: the user cannot change it, so a change needs a new build.

```jsonc
{
  "id": "creative.video",
  "name": "…", "description": "…", "category": "Video",
  "sizeHint": 1500000000,          // optional, for a size estimate
  "digest": "sha256:…",            // optional, checked before setup runs
  "source": { "type": "gdrive", "fileId": "ID_FROM_THE_LINK", "fileName": "suite.zip" },
  "setup": { "type": "zip", "run": "setup.exe", "args": [] }
}
```

- `source`: `gdrive` (a file id **or the whole Drive link**), `static` (any https URL: R2, B2, Dropbox, your own server), `github`, `page`.
- `setup`:
  - `zip` unpacks and runs `run`; without it, it finds a `setup.exe`/`.msi` itself. `"onlyRun": true` extracts **only** that file, for a zip that holds a whole project.
  - `installer` runs a plain exe/msi.
  - `extract` only unpacks, into `to` (default `%USERPROFILE%\Downloads\<name>`).
  - `"password": "…"` for locked zips (AES or classic).
- `icon` (optional): `"/icons/app.svg"` from `frontend/public/icons/`; an `https://…` image; or a local path (read by the backend, never uploaded). Prefer SVG, or 256×256 PNG/WebP with transparency. Accepted: svg, png, webp, jpg, gif, avif, ico, up to 4 MB.
- **Where files go:** the package downloads to `%USERPROFILE%\Downloads\` and unpacks into `%USERPROFILE%\Downloads\<name>\`; the installer runs from there, and asks for admin rights through UAC if it needs them.
- **Cleanup:** the download and the folder unpacked for an installer are deleted **when the app closes**, not sooner, so an installer that restarts itself never loses its files mid-run. An `extract` destination is **never** deleted.
- Progress (percentage) shows for both the download and the extraction, with the file name. If `run` matches nothing in the zip, the error names the `.exe`/`.msi` the package does contain. A malformed JSON shows an error with the line number instead of being ignored.

**Google Drive.** Uses `https://drive.usercontent.google.com/download?id=<ID>&export=download&confirm=t`, which skips the "can't scan this file" page.

- The file must be shared as **"Anyone with the link"**.
- Drive has a **daily per-file download quota**. Once it is hit it returns a web page instead of the file, and the app says so; there is no way around it.
- For large or often-downloaded packages, **Cloudflare R2** (no egress fees), **Backblaze B2** or **GitHub Releases** (2 GB per file) are better, with `"source": { "type": "static", "url": "…" }`.

### Spotify Hub

Two actions on Spotify Desktop, resolved entirely in Rust (`backend/src/spotify_hub/`, page in `frontend/app/pages/spotify-hub/`): **install Spicetify** (with a theme) and **restore Spotify** to stock. A third, **Purge all**, removes Spicetify and its leftovers after a preview.

- The webview picks one of three action ids. Release URLs, process arguments, registry keys and filesystem targets are all resolved from compile-time allowlists (`actions.rs`), so a page cannot name a path.
- Release downloads come from Spicetify's GitHub releases, SHA-256 checked; the install runs the tool with the app closed, writes the placeholder theme from `backend/resources/spicetify/`, and reopens Spotify.
- **Purge** first shows a preview of exactly what it will delete (5-minute grant), then removes it; MYLE closes Spotify first and never touches a user's own Spotify cache outside that list.
- Detection (`detection.rs`, `installed.rs`) reads Spotify's install folders, its version and whether Spicetify is applied; the page's status comes from there, and a Spotify update that wipes the patch brings the install button back.
- Phases are reported over a channel (`Preparing`, `Downloading`, `Verifying`, `Installing`, `Restoring`, `Uninstalling`, `Cleaning`, `Finalizing`, then `Done` / `Cancelled` / `Partial` / `NeedsAdmin`), and the page shows them as a progress line.
- Anything that needs administrator rights goes through `elevated_pipe`.

### Game Saves

Uses a bundled **Ludusavi v0.31.0** with its own config directory, so it never touches a user's own Ludusavi install. `backend/src/game_saves/`, page in `frontend/app/pages/game-saves/`.

- A normal scan is offline (`--no-manifest-update`) and recognises saves from the pinned database plus the user's custom games. `Update database` is the only action that talks to the manifest source; a failed download or validation keeps the previous database atomically.
- Backups are ZIP/Deflate 6, three full snapshots per game, no differential snapshots. Restore runs a fresh preview, asks for confirmation and keeps a safety copy for seven days; `Undo last restore` puts it back.
- The frontend sends opaque ids. Titles reach Ludusavi over stdin, never as arguments.
- Settings are saved atomically in the app data: launcher roots, custom games, exclusions, path mappings, database metadata and the last scheduled result.
- **Daily/Weekly auto-backup** creates a per-user Windows Scheduled Task with the chosen calendar trigger, a logon catch-up trigger, and `StartWhenAvailable` with a calendar end boundary so sleep/resume catches up a missed run. It has no elevation and never wakes the PC. Start-up repairs older or disabled tasks and launches an overdue run. The headless process compares calendar slots in date-aware local time under the operation lock; a PC-local activation timestamp stops a newly chosen schedule from backing up an earlier slot. `Off` and uninstall both remove the task.
- A **scheduled run ends with a notice of MYLE's own** (`game_saves/notice.rs`, window in `frontend/notice/`): the headless backup starts `MYLE.exe --game-saves-notice=<base64 JSON>`, which opens only a small frameless, transparent, always-on-top window at the bottom right of the main screen's work area. It does not take the focus, plays a short chime (Web Audio, no sound file) and closes itself after a few seconds (longer after a failure, and not while the pointer is on it). It has its own WebView2 profile (`webview-notice`). Clicking it runs `MYLE.exe --open-game-saves`: a running MYLE is focused and goes to Game Saves. Preview with `npx vite` at `/notice.html?kind=backedUp` (or `nothingNew`, `failed`).
- A backup folder whose drive is not connected (Google Drive's `G:` while Drive is not running) does not stop a scan: the saves on this PC are listed with an unknown backup status and the page says the backups are out of reach. Backups and restores stop with the same explanation.
- **Cloud backup**: buttons for Dropbox, Google Drive, MEGA and OneDrive. Each creates `<cloud>\Make Your Life Easier\Game Saves Backups` and makes it the backup folder — never the root of the cloud folder. Only each app's local synced folder is used; no cloud APIs, no accounts:
  - **Dropbox**: `info.json` in `%LOCALAPPDATA%` or `%APPDATA%\Dropbox` (personal and business), else `%USERPROFILE%\Dropbox`.
  - **Google Drive**: Drive for desktop's drive (from its registry settings, or a drive named "Google Drive") and its "My Drive" folder in whatever language, or the mirror folder in the profile.
  - **MEGA**: its settings are encrypted, so `MEGA` / `MEGAsync` is looked for in the profile and in Documents.
  - **OneDrive**: the `OneDrive*` environment variables.
  - If a service is not found, its button asks for its folder and creates the backup folder inside it. Earlier backups stay where they are.

The pinned binaries and manifest are ignored by git: a fresh checkout fetches them in `beforeDevCommand`/`beforeBuildCommand` (or with `scripts/bootstrap-game-saves.ps1`). The URLs and SHA-256 values live in that script; the licences ship from `backend/resources/ludusavi/`.

### Project Backups

Zips of project folders in Google Drive or Dropbox (`backend/src/project_backups/`, page in `frontend/app/pages/project-backups/`). It is a port of the standalone `backup_projects` app, writing the same folders and names, so both apps add to one history:

```
<cloud>\Projects Backup\<AppName>\<YYYY-MM Month>\<AppName>_D<day>_V<n>.zip
```

- **Names.** Month folders are named in Greek as `backup_projects` does and read in any language (`2026-10 October` too). The version carries on from the highest `<AppName>_…_V<n>` in any month folder (`_D_V`, `_V_D` and `_V` names, zips and the old folder backups); `__partial__` leftovers and `all - pre release backups` never count.
- **A backup.** The folder is walked with the exclusions (`walk.rs`), the zip is streamed (Deflate 6) into `%LOCALAPPDATA%\…\data\project-backups-staging`, every entry is read back against the size and CRC-32 read from the source, and the folder is walked once more trusting only metadata to make sure no file is missing (`audit`). Only then is it copied next to the backups as `__partial__<name>__<ms>.zip` (SHA-256 while copying), re-read until size and SHA-256 match (stale cloud views are retried for about a minute) and renamed with `MoveFileExW` **without** `MOVEFILE_REPLACE_EXISTING`: a zip of that name saved meanwhile — by `backup_projects`, say — fails the backup with `NAME_TAKEN` and is never overwritten. Without room for staging it is built and checked at the destination. `.backup-info.json` inside each zip has `backup_projects`' fields plus totals, exclusions and the items that cannot be zipped.
- **Reparse points** (OneDrive/Dropbox placeholders, WOF-compressed files) are ordinary files to Rust's std and are backed up; links to files are followed, links to folders and junctions are listed as skipped. An online-only file whose app is not running (os error 362) starts OneDrive, Dropbox or Google Drive and the backup is tried once more.
- **Exclusions.** A global list (`rules.rs`, `DEFAULT_PATTERNS`) of names that are never sources: VCS and agent folders (`.git`, `.agents`, `.claude`, `.codex`), editor caches (`.idea`, `.vs`, `ipch`), JS/TS (`node_modules`, `dist`, `.next`, `.nuxt`, `.svelte-kit`, `.vite`, `.turbo`, `.angular`, `.expo`, `coverage`, `*.tsbuildinfo`, …), Python (`__pycache__`, `.venv`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`, `*.egg-info`, …), `.gradle`, `.dart_tool`, `DerivedData`, `.build`, `xcuserdata`, `.godot`, `cmake-build-*`, `CMakeFiles`, `*.o`, `*.pdb`, `*.ilk`, logs, temp files, archives and OS junk. Each project adds its own and its "always back up" patterns. `name/` is a folder anywhere, `a/b/` is anchored to the project folder, `*.log` a file.
- **Names that are sometimes sources** (`target`, `build`, `bin`, `obj`, `out`, `Debug`, `Release`, `x64`, `packages`, `Pods`, `Library`, `Temp`, `venv`, `env`, `gen`, …) are decided by markers (`detect.rs`), read from the folder's own listing and its parent's: `CACHEDIR.TAG`/`.rustc_info.json` or `Cargo.toml` next to `target` (Rust), `pom.xml` (Maven), Gradle files, `pubspec.yaml` (Flutter), `pyproject.toml`/`setup.py` (Python), `*.xcodeproj`/`Package.swift` (Xcode/SwiftPM), `CMakeCache.txt` or `CMakeLists.txt` (CMake), a `.csproj`/`.fsproj`/`.vbproj` next to `bin`/`obj` (.NET), a `.sln`/`.vcxproj` next to `Debug`/`Release`/`x64` (Visual Studio), `go.mod` next to `bin`, `pyvenv.cfg`/`conda-meta` inside (virtual environments), electron-builder's config next to `release`, `Assets` + `ProjectSettings` next to `Library`/`Temp`/`Obj`/`Logs` (Unity), `project.godot` next to `.import`, a NuGet layout next to a `.sln`, a `Podfile` next to `Pods`, Tauri's `gen/schemas`. Without a marker such a folder is left out only when git tracks nothing in it (`build`, `out`, `target`, `bin`, `obj`, `debug`, `release`, `gen`; the index is read directly in `gitindex.rs`) or a `.gitignore` lists it — and a folder git tracks files in is always kept.
- **Preview.** The editor and the Exclusions panel show what a backup holds before it runs: the project as a tree (folders with file count and size, at most 4,000 lines; folders and what is left out always listed), each left-out item with the rule that leaves it out (a pattern, a detected build folder, `.gitignore`), and a count per rule. A new preview stops the running one, and closing the dialog stops it (`project_backups_cancel_preview`).
- **Destination.** It is made ready before anything else (before closing the program). A failure answers a code the page offers an action for: `NO_PROVIDER` (choose one), `CLOUD_NOT_INSTALLED` (the other cloud, or install Google Drive), `CLOUD_NOT_READY` (start it, try again) or `DESTINATION_MISSING` (choose the folder again) — and the remaining projects are not tried. Every attempt is kept on the project (`lastResult`: when, ok/cancelled, code, message) for the page's status tags. Cloud apps are started detached from MYLE (`DETACHED_PROCESS`, breakaway from its job). Google Drive's drive exists only while `GoogleDriveFS.exe` runs: the newest installed version is started and the drive waited for (2 minutes, cancellable); if it comes up under another letter and only one Drive folder is found, that one is used. Dropbox is found through its `info.json` and `Dropbox.exe` is started if needed. Detection and launching live in `backend/src/cloud/`, shared with Game Saves.
- **Before a backup** an optional program (`MyApp.exe`) is closed (`taskkill`, forced after 1.5 s); MYLE never closes itself. A project whose folder is missing answers `SOURCE_MISSING`: the page opens the editor on the folder and the backup carries on after saving.
- **Compare** two backups, or a backup with the project folder: names are normalised (`\` → `/`, Unicode NFC, a single wrapper folder ignored when that pairs more files, case-insensitive), files are matched by size and CRC-32 (a folder side is hashed), and the same exclusions apply. A deleted file that is still in the project folder is flagged (the newer backup is incomplete). Reporting progress, cancellable also mid-file; only one backup or comparison runs at a time, and closing the dialog cancels only the comparison. Clicking a file shows its preview, read on demand from the zip or folder (`project_backups_file_diff`): text as a side-by-side or unified diff with line numbers (`similar`, up to 2 MB a side and 20,000 rows; UTF-8/UTF-16 BOMs are read; a CRLF/LF-only difference is said so), an added or deleted text file in full, images (png, jpg, gif, webp, ico, bmp, avif, svg up to 8 MB) old next to new with an SVG's source as text, other files by size and SHA-256 (up to 512 MB). The list draws 400 rows at a time; ↑/↓ step through files.
- **Three-day change check.** While MYLE runs, the owner's state compares every saved project afresh every three days and shows a Review notification for pending changes or missing backups, even when the count is unchanged. The last check time is local to this PC (`myle.projectBackups.lastChangeCheck`), so restarting keeps the deadline and an overdue check runs on the next start. Busy operations wait; failed native requests are retried. Unreachable, missing or unreadable folders are reported separately. It never creates backups; the live folder watcher and manual checks still update the badges between reminders.
- **First start.** `backup_projects`' `%APPDATA%\ThomasThanos\Backup-projects\projects.json`, else its copy `Projects Backup\.backup-projects.json`, else the project folders in `Projects Backup` are imported once (by backup name; a program to close is dropped when it is MYLE). **Import** runs it again for new ones. Settings: `%APPDATA%\ThomasThanos\MakeYourLifeEasier\project-backups.json`.
- Tests (`cargo test project_backups`) include a zip made by `backup_projects` 2.0.6 (`project_backups/testdata/`), read, compared and continued.

### GitHub Releases

Commit, build and release git projects to GitHub (`backend/src/github_releases/`, page in `frontend/app/pages/github-releases/`). It replaces the standalone `Github-Build-Release` app, whose last project and DeepSeek key are brought over once on first open (the plain-text key is then removed from its `grm-config.json`, and the page says so).

- **Projects.** Add a repository (a folder inside one adds the repository), or scan a folder (four levels down, not inside repositories). A monorepo (no version at its top, or npm workspaces) shows each app folder with a manifest as its own project, with the id `<repo id>/<folder>`; the backend accepts only folders the repository really has. `/` or Ctrl+F searches. Saved in `github-releases.json` in the roaming folder. The owner account also syncs repository paths through `user_settings.data.githubReleases` in Supabase, separately from the legacy `v7` preferences so older clients cannot erase them. The list has its own change timestamp, so changing the theme after a reinstall cannot overwrite cloud projects with an empty list. First sync uploads existing local paths when the account has no list; missing paths on another PC remain listed. Explicit removals sync too. Imports preserve local ids and build settings for retained paths, reject malformed input before editing, and wait for running operations. Commands, GitHub/AI credentials and build outputs are never included. No new table or RLS policy is needed.
- **Status.** Branch, ahead/behind (a quiet `git fetch` when a project is opened), changed files, the versions of every version file (`package.json`, `package-lock.json`, `Cargo.toml` + `Cargo.lock`, `tauri.conf.json` unless it points at `package.json`, `pyproject.toml`, `.csproj`, an extension's `manifest.json`; in the project folder and in a Tauri folder one level down) with a warning when they disagree, the last tag and GitHub release, the last Actions run, and badges for what the project is built with.
- **Git.** Run with `git -C`, no pager or prompts, `core.quotepath=false`, file paths only after `--` with `--literal-pathspecs`, and checked against the current status. Push and pull first use the user's own credentials; on an authentication failure they retry with the GitHub token as an `http.https://github.com/.extraheader` passed through `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_0`/`GIT_CONFIG_VALUE_0` in the child's environment — never on a command line or in `.git/config`. Failures get codes (no identity, nothing to commit, hook failed, rejected, protected branch, secret blocked by push protection, workflow scope, no upstream, conflict, network, auth) and the page offers the fix. Pull is `--rebase --autostash`; a conflict aborts the rebase.
- **Changes.** Each changed file can be picked (staged) or left out; its diff is Project Backups' `DiffView` (HEAD against the file on disk, `project_backups::compare::bytes_diff`). Commit, Commit & Push (Ctrl+Enter / Ctrl+Shift+Enter), and "Generate with AI" from the staged diff (lock files left out, 14,000 characters at most). With nothing to commit the tab shows the commits since the last release (unpushed ones marked, `github_releases_recent_commits`) and the next steps: pull/push when needed, Build, Release, the folder, GitHub. Alt+1–4 switch tabs, Alt+R refreshes (F5 and Ctrl+R stay blocked app-wide).
- **Build.** Detected commands (a `build-all`/`release`/`dist`/`build` script with the right package manager, `tauri build`, `cargo build --release`, `dotnet publish`, PyInstaller, `go build`, `flutter build windows`) or a typed one. The project's own top-level script is its full build and comes first: its chain (`&&`, `npm run …`) and the named steps of a script file it starts (`Invoke-Step "App" { … }`, `==> App`; MYLE's own `build-setup.ps1`) show as a pipeline that lights up as the log reaches each step (`build/detect.rs`, `pipeline.ts`). `tauri build` is offered on its own only when no script runs it, and says so when `bundle.active` is off (the app alone, no installer); picking or typing anything else than the full build shows that, with a button back. A release built here runs the same command. It runs through `cmd /d /s /c` in a Job Object (cancel kills the whole tree), with colours off; the log streams in batches, is saved to `%LOCALAPPDATA%\…\github-releases\logs` (10 per project), and errors and warnings (rustc, tsc, svelte-check, MSBuild, ESLint, esbuild, Vite/Rollup, electron-builder, NSIS, Python, Go) fill the Problems panel; a problem opens in VS Code or Cursor (`vscode://file/…:line:col`). Progress follows stages and the last build's length. Fresh installers, update files and signatures are listed with their SHA-256.
- **Release.** Patch/minor/major or a typed version; every version file (and the lock files) is set to it with a preview, keeping the files' formatting. Notes are written by hand, by AI from the commits since the last tag (and the diff), or polished. Checks first (version, files agree, GitHub remote, branch up to date, uncommitted changes, tag free locally and on GitHub, account, workflow or build command). Then: version files → build → commit → push → annotated tag → push the tag, then either a draft release with the files uploaded (streamed, 3 tries each) and published, or — **Actions mode**, picked by itself when a workflow runs on the tag — the workflow followed job by job, with its notes written to `docs/release-notes/<version>.md` when the workflow reads that folder. A failure before the push puts the files and commit back; after it, **Resume** carries on (remaining uploads, publishing). Monorepo apps tag as `<folder>-v<version>`; their earlier shared `v…` tags count as their last release (same major, not newer). Actions mode follows the run of the workflow the tag starts, found by its file (a tag can start several: MYLE's also starts MYLE Passwords, done in seconds); without notes, `release.yml` publishes GitHub's generated ones. Building here while a workflow also runs on the tag is a warning. The checks follow the form, fetching at most every 90 seconds, and only the newest answer is shown.
- **Releases (history).** The project's releases with downloads; edit title, notes and pre-release; delete one or several (their tags too, when asked); **combine** several: AI merges their notes into the newest, and one confirmation that names the release getting the notes and every release and tag to go saves the notes, checks GitHub kept them, and only then deletes the others with their tags (on GitHub and here). Nothing is deleted when saving fails; the kept release's tag, and any tag another release still uses, is never deleted; what could not be deleted is listed with **Try again**. **Tags without a release** (a failed or abandoned release leaves one) are listed apart, here and on GitHub, from every page of releases (drafts count); one, several or all are deleted on GitHub and here after a confirmation that names them, and the backend looks again first and refuses a tag a release uses.
- **GitHub account.** The GitHub CLI's sign-in (`gh auth token`, read once and checked with `GET /user`) or a pasted token, sealed with DPAPI in `%LOCALAPPDATA%\…\github-releases\github-token.bin`. MYLE never installs anything by itself: a missing Git or GitHub CLI shows a button that runs `winget install` in a visible window, and "Sign in" opens `gh auth login --web` in its own console. Device-flow sign-in needs a registered OAuth App and is not built.
- **AI.** One OpenAI-compatible client for Groq (first; `openai/gpt-oss-120b`), Google Gemini (`gemini-flash-latest`), OpenRouter's free models (`openrouter/free`), DeepSeek (`deepseek-v4-flash`) and Ollama on this PC; the order and models are the user's. Keys are checked with the provider, then sealed with DPAPI (`ai-<provider>.bin`) and never sent back to the page. A failure (429 with its wait, bad key, no credit, model gone) names the next ready provider and the page offers it; nothing is sent elsewhere unasked.
- Tests (`cargo test github_releases`): porcelain v2 status, remotes, tags and log, failure codes, token environment, version files (a MYLE-like layout, mismatches, format-preserving JSON/TOML edits, bumps), monorepos and their tags, workflows and tag patterns, build detection, the error parsers, the runner (a real `cmd`), artifacts, the AI requests and replies (a mock server with a 429), the GBR import and settings.

### Windows Optimization

Five tabs: **Quick setup**, **Settings**, **Apps**, **Start Menu** and **Tools** (Auto-Logon, restart to BIOS/UEFI). The debloater is MYLE's own (`backend/src/debloat/`, page in `frontend/app/pages/windows-optimization/debloat/`); it replaces the WinUtil and Sparkle launchers, whose downloaded cache (`…\data\windows-optimization`) is deleted on the page's first load.

- **Choose, then apply.** Nothing changes while the user chooses. Quick setup's profiles, the Settings switches (one line per tweak, `SettingRow.svelte`, details on hover or under the line) and the Apps ticks only record choices (`desired` and `removing` in `state.svelte.ts`, kept in `myle.debloat.pending`; an older version's ticked apps are carried over). The bar under those tabs shows how many there are; **Review & apply** lists them (`ReviewPanel.svelte`), where any can be left out, then makes one restore point and does them all. The logic is plain TypeScript in `selection.ts`, tested by `scripts/tests/debloat-selection.test.mjs`.
- **Profiles.** Every tweak and app has a `level` in `catalog.rs`: **Light** (privacy and promoted apps; nothing looks different), **Recommended** (adds what nearly everyone is better off with) and **Maximum** (the rest, with small trade-offs). Each profile takes in the ones before it and only ever turns things on. `None` is outside every profile: personal taste (dark mode, the taskbar on the left), Edge, and the apps most people use. A tweak's `note` says what to know before turning it on; an app's `about` says what it is and `keep` why someone might keep it.
- **The tables.** `catalog.rs` holds every tweak (registry values, service start types, scheduled tasks, Store apps, the 24-hour clock, Edge, parts of Windows) and every app it may remove, fixed at compile time; the page sends only ids. Values follow WinUtil, Win11Debloat and Sparkle without their known mistakes: WinUtil's `wermgr` is not a service; `SearchboxTaskbarMode` has four values, so the user's own is kept; Widgets are hidden with the `Dsh\AllowNewsAndInterests` policy because the UserChoice Protection Driver blocks `TaskbarDa`, and Windows 10's `ShellFeedsTaskbarViewMode` is best effort too (24H2 answers "access denied"); `SharedAccess` is left alone (Mobile Hotspot needs it). A service changes only from the start types Windows may have (`from`), so one the user set by hand stays. `NEVER` and `removable()` keep Windows' and MYLE's own packages out of reach whatever the table says: the Store, Terminal, winget, Xbox sign-in, frameworks, codecs, and WebView2, which MYLE runs on. Names match exactly or by publisher suffix, never with wildcards.
- **Where it stands.** `detect.rs` reads each tweak's state from the registry, services (`QueryServiceConfig`), scheduled tasks (Task Scheduler COM), the locale's time format and the installed packages (the per-user AppModel repository in the registry, not PowerShell) — without administrator rights, in well under a second.
- **Undo.** Before each change `undo.rs` keeps what was there: the value or its absence and the first key the write created, the start type, the task's state, the time format. Only the first original of an operation is kept, so applying twice cannot overwrite it. Undo puts it back exactly and removes the keys it created while empty. The record is `debloat-undo.json` in the local data folder; Edge's undo installs it again with winget, and removed apps are offered again from the Microsoft Store. A tweak that was on before MYLE ran is turned off with the Windows defaults (`detect::default_before`): no policy, the value Windows ships with, and for a service the start type in the catalog's `windows`, never one Windows keeps off itself (Remote Registry stays disabled).
- **Who does what.** The user's own settings (HKCU, the time format through `SetLocaleInfoW`) are changed by the app itself, as the user, so an elevation with another administrator account cannot land them in that account's profile; Explorer is restarted by the app for the same reason. Machine-wide changes, app removal for every user and new ones (`Remove-AppxPackage -AllUsers` and the provisioned package), Edge and restore points go through an administrator helper: this same program started with `--debloat-elevated-helper` over `elevated_pipe.rs` (shared with the System Cleaner; one UAC prompt per session, the helper serves only the app's pipe and the app answers only this same program). The helper checks every undo record against the catalog: only the tweak's own operations, strings from `allowed_machine_strings`, and keys on the way to the value.
- **Windows features.** Parts of Windows few people use (Steps Recorder, Math Input Panel, WordPad, XPS Viewer, Windows Media Player Legacy, PowerShell ISE, Internet Explorer mode, Fax and Scan) are tweaks of the `features` category, shown on the Apps tab. Whether one is installed is read without administrator rights from the servicing store: `HKLM\…\Component Based Servicing\Packages`, the neutral packages of its name, `CurrentState` 0x70 (installed). A Windows without the package shows nothing. The helper removes it with `Remove-WindowsCapability` (matching the catalog's capability name, never one it is sent) and Undo adds it back with `Add-WindowsCapability`, which downloads it from Windows Update. Optional features (`Enable-WindowsOptionalFeature`) are left out: their state is not in the registry, and the ones worth turning off (SMB1, PowerShell 2.0, XPS Services) already are on Windows 11 24H2.
- **Edge.** `setup.exe --uninstall --force-uninstall` from Edge's own uninstall entry, accepted only from Edge's folder, with `EdgeUpdateDev\AllowUninstall` and the old-Edge stub in place for the run (both removed after). EdgeUpdate stays: it also updates WebView2.
- **Restore point.** Made before every Review & apply that turns something on or removes an app (`Checkpoint-Computer`, with Windows' once-a-day limit lifted for the call and put back). System Protection is read from `SPP\Clients`; if it is off the page offers to turn it on for the Windows drive, or to go on without.
- **Start Menu.** `start_menu.rs`: layout, alignment, the All Apps view, the Recommended section's parts and the folders next to Power are the user's own (HKCU, set by the app). Hiding Recommended and the pinned-apps presets are machine policies set by the helper; pins come only from `PIN_CATALOG` (the helper checks the JSON again), `start2.bin` is backed up before the first preset, and the `ConfigureStartPins` policy is removed again after the Start menu has read it, so pins stay the user's to change.
- **Tests on a real Windows.** `sandbox_tests.rs` applies every tweak for real, applies it again (nothing new is kept), undoes it and compares every value, key, service, task and the time format with before; it also turns tweaks off from the Windows defaults, sets and clears the Start Menu policies, and makes a restore point. They run only in Windows Sandbox (`MYLE_DEBLOAT_SANDBOX=1`, account `WDAGUtilityAccount`) or a VM (`MYLE_DEBLOAT_VM=1`): copy the test executable (`cargo test --lib --no-run`) and `vcruntime140*.dll` into the sandbox and run `myle_lib-<hash>.exe sandbox_ --ignored --test-threads=1 --nocapture`. Edge's removal needs a VM: Windows Sandbox shares Edge's files with the host, read-only, so its uninstaller cannot delete them.

### System Cleaner

Measures and removes the usual Windows leftovers (`backend/src/cleaner/`, page in `frontend/app/pages/system-cleaner/`). The page sends category ids only; paths, patterns and the elevated script all come from the fixed table in `targets.rs`.

- **Categories:** Temporary files (`temp`), Windows Update leftovers, Thumbnails, Prefetch, the Recycle Bin, and Windows Error Reporting (`errors`). Each has a title, a description, a hint, an icon and `may_need_admin` (set when at least one of its folders is readable only as administrator).
- **Measuring.** A scan is a read-only walk with a size and file count per category, reported over a channel; it can be cancelled. Files that are in use are counted and reported honestly ("Freed X · Y in use") instead of pretending they went.
- **Removing.** Categories the user picked are deleted; Recycle Bin contents go through the shell's own API (`recycle_bin.rs`), never a hand-rolled delete. Anything outside the current user's reach goes through the administrator helper (`elevated.rs`, `--cleaner-elevated-helper` over `elevated_pipe.rs`); a file the helper could not delete is counted as skipped (`skip()`), not as freed.
- **Downloads** (`downloads.rs`): the Downloads folder is listed with each file's size and date for the page to show, and deletion is by id with a re-check, never by a path the page names.
- **Software health** (`health/`): a separate panel that inspects installed programs (from the Uninstall registry keys), finds leftovers of programs that are gone, and reports what it finds. `install_locations` from here is reused by the Disk Analyzer's protection rules.
- A run holds the job registry (`Jobs::start("system-cleaner")`), so a delete cannot overlap with the Disk Analyzer's or a backup's.

### System Maintenance

Windows diagnostics and repairs, one at a time (`backend/src/maintenance/`, page in `frontend/app/pages/system-maintenance/`). The page sends an action id; the command, its arguments and the elevated script all come from the fixed table in `tasks.rs`.

| Section | Actions |
|---|---|
| **Network** | Flush DNS, renew the IP, restart the audio service, Network Reset |
| **Health** | `sfc /scannow` (System Files), `DISM /RestoreHealth` (Disk image) |
| **Software** | Update all applications (winget `upgrade --all`) |

- Every action runs one at a time (the console shows its output live, `Console.svelte`), can be cancelled, and reports its exit code. `3010` means "done, restart Windows"; winget's `0x8A15_002B` means nothing had an update — both are shown as success with the right wording, not as errors.
- Network Reset and the DISM repair need administrator rights and go through `elevated_pipe`.
- The output is parsed into sections and bullet points (`output.ts`) rather than shown as raw log lines where a known tool produced it.

### Disk Analyzer

What is filling a drive or a folder (`backend/src/disk_analyzer/`, page in `frontend/app/pages/disk-analyzer/`), written from scratch: a treemap, the folder tree, the largest files, file types, moving items to the Recycle Bin — and, below those, **Cloud Storage**. No administrator rights, except one prompt when a fast scan starts its helper.

- **Scanning** (`scan.rs`). The page sends one path: a drive chip's root or a folder from the picker. Each folder is opened once (`CreateFileW` with `FILE_LIST_DIRECTORY`, `\\?\` paths) and read in 128 KB batches with `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` (`FileIdBothDirectoryInfo`, then `FileFullDirectoryInfo`, where a file system refuses it): size, size on disk, attributes, reparse tag and date for many entries per call, and no file is ever opened. A `std::thread::scope` pool (the logical CPUs, at most 8) shares a stack of folders to read; each finished listing goes to the one thread that owns the results. Junctions, symbolic links, mount points and app execution aliases are listed as links of size 0 and never followed; a OneDrive folder (`IO_REPARSE_TAG_CLOUD*`) is walked like any folder. Cloud placeholders (`RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN`, `OFFLINE`, cloud tags) count their size, take the size on disk as reported (often 0) and are never opened. A folder that cannot be read is counted (`inaccessible`) and the walk goes on. Hard links are not known without opening files, so on the Windows drive a hard-linked file (WinSxS) counts once per link; the page says so (the fast scan counts it once). Progress goes out at most 10 times a second (`Throttle`, tested with a fake clock); a new scan, `disk_analyzer_cancel` or closing the app stops a running one.
- **Results** (`tree.rs`). One flat list of 40-byte nodes (sizes, parent, the name's place in one shared buffer, extension, kind, flags, date); a folder's children are added together, so a folder keeps only where they start and how many. Totals are summed bottom-up in one pass from the end. The page never gets the whole tree: it asks for a page of a folder's children (sorted by size, name, files or date; at most 500, plus what is left as "N smaller items"), a treemap of at most 20,000 nodes as flat arrays (big folders are opened first; entries below `minBytes`, about 9 pixels, are merged per folder), the 1,000 largest files (worked out once, or filtered by name), and file types (the scan's 20 largest extensions, the rest as "Other types"). Every answer carries the scan's id, and an id from an earlier scan is refused.
- **Recycle Bin** (`delete.rs`, `protect.rs`). By ids only, while no System Cleaner run holds the job registry (`Jobs::start("disk-analyzer-delete")`). Each item is checked again just before it moves: still there, inside the scanned folder (compared by path parts), not a link, the same kind as in the scan, and not protected. Protected, whatever the page sends: drives, the scanned folder, Windows and everything in it, `Program Files` / `Program Files (x86)` / `ProgramData` themselves (what is inside goes with a warning), `System Volume Information`, `$Recycle.Bin`, `$WinREAgent`, `Recovery`, `Boot`, `EFI`, the page/hibernation/swap files, `DumpStack.log*` and every `$` name at a drive's root, system files directly under a root, `Users`, every profile and its main folders (and the current user's known folders wherever they were moved), MYLE's install and data folders, the folder of every installed program that says where it lives (`InstallLocation` of the Uninstall keys, through `cleaner::health::install_locations`: "uninstall it from Apps instead"), every running program's file and the folder it runs from (a Toolhelp snapshot and `elevated_pipe::process_path`: "close it first"), and any folder that holds one of those. A program or game that names a drive, a profile, Documents or a folder holding one of those keeps that place's usual rule, so a bad registry entry cannot claim a whole folder. Some things go only after a warning that names them, shown with each item in the question before anything moves: what is inside an installed program (the closest one names it) or the folder of a running one, browser data in every profile (Chrome, Edge, Brave, Vivaldi, Chromium, Yandex, Opera, Opera GX, Firefox, LibreWolf, Waterfox, and Thunderbird's mail) and the folders that hold it, the save folders Game Saves found in its last scan (`game_saves::known_save_folders`) and the folders that hold them, and what is inside `Program Files` or `ProgramData`. The confirmation and the move each take the job lock, so the two cannot interleave.
- **The page.** `fill: true`: drive chips with a used/free bar, Choose folder…, Size / Size on disk (on disk by default, kept and synced with the other view choices), Scan / Cancel. The folder list draws only the rows on screen (a folder of 100,000 entries scrolls like a short one); ↑/↓, →/←, Enter (show in the treemap), Space and Del work there, and every action is in the context menu too. The treemap (`treemap.ts`, the squarified layout of Bruls, Huijsen and van Wijk, pure functions tested in Node) is drawn on a canvas, again only when the data or the size changes; hover, selection and the kind in focus go on a second canvas, and the tooltip (`lib/tooltip.svelte.ts`) and the right-click menu (`lib/context-menu.ts`) find the item in the layout's flat arrays. Double-click opens a folder, Backspace or Alt+← goes up. Colours go by kind of file, not by extension (`palette.ts`: video, images, audio, archives and disk images, apps and games, documents, code and data, system, other, from a fixed extension table), muted, one set per theme; the treemap, File types and Largest files share them. The legend under the map has one chip per kind with its size; a chip dims everything else. The three tabs are **Folders**, **Largest files** and **File types** (arrow keys move between them).
- **Recycle failures.** The action never falls back to permanent deletion by itself. When Windows cannot recycle an item, the result includes its node id and `canDeletePermanently`, and the page offers a separate **Delete permanently** with another danger confirmation naming the items and their logical size (including online-only files). `disk_analyzer_delete_permanently` repeats the same id, path, link, protection and job checks; only its shell mode differs.
- **Deletion progress** forwards the shell's `UpdateProgress` work estimates, throttled to ten a second. The bar includes work inside the current folder and sweeps while Windows has not provided a total. After a scan the drive cards collapse behind **Change drive**; the results keep a minimum height and the page scrolls in short windows so the map and its legend stay reachable. Folder frames use five stable muted accents, separate from the file-type colours.
- **Fast scan** (`mft.rs`, `elevated.rs`). For a whole fixed NTFS drive (`mftPossible`) with the fast scan on (the default; kept per PC as `myle.diskAnalyzer.fast`, not synced), `disk_analyzer_scan` gets `fast: true` and reads the drive's file table instead of walking it, as WizTree does — written from the public NTFS layout, not from any other program. Opening `\\.\C:` needs an administrator, so it goes through the elevated helper on the `elevated_pipe` pattern (`--disk-analyzer-elevated-helper`, one UAC prompt, then reused until idle for half an hour). The app asks by drive letter only (`{"op":"scan","drive":"C"}`); the helper checks the drive is fixed and NTFS and answers with a JSON line followed by bytes (`serve_with_payload` / `Session::ask_with_payload`: an 8-byte length, then the payload) — a compact snapshot of the files in use (record number, folder or not, attributes, reparse tag, date, size, size on disk) and their names with their folders, never the volume handle. Reading: the boot sector gives the sector, cluster and record sizes and where the table starts; the table's own record 0 gives its data runs (and, through its attribute list, the extension records holding more of them when it is very fragmented); the table is then read in 4 MB chunks, and each record has its update sequence checked and put back, then `$STANDARD_INFORMATION`, `$FILE_NAME` (every name but the 8.3 ones, each with its folder's reference), `$DATA` (the main stream's size; on disk, the clusters every stream's data runs really point at, so a hole takes nothing, a cloud placeholder takes nothing, a WOF-compressed file takes its compressed stream, and `$BadClus` takes nothing), `$INDEX_ALLOCATION` and `$REPARSE_POINT`. Extension records add to their base record.
- **Cloud Storage** (`backend/src/cloud/online_only.rs`, page in `disk-analyzer/cloud-storage/`). Below the map: make a cloud app's files **online-only**, so they stop taking space on this PC while staying in the cloud. Google Drive, MEGA, Dropbox and OneDrive are listed with their synced folders and a `mode`:
  - `native` — MYLE can do it itself by setting the file's cloud attribute (Google Drive's Online only, and any other app Windows exposes a sync root for).
  - `guided` — the app needs its own controls, so the page shows what to press.
  - `unavailable` — nothing found on this PC.
  The run reports `visited`, `requested`, `alreadyOnline`, `pendingUploads`, `skipped`, `failed`, `foldersRequested`, `foldersAlreadyOnline` and the file it is on, over a channel, and can be cancelled. Google Drive's cache is cleared through Drive's own storage window (the page waits for it), and the answer says how much was cleared, that it was already empty, or that the window was not opened.
- Tests: `cargo test disk_analyzer` (a temp tree with an empty folder, a path past 260 characters, Greek and emoji names, a junction that must not be followed, a folder an ACL locks, cancelling; totals, pages, top files, file types, removing; every protected rule; the re-checks and a real move to the Recycle Bin, which is taken back out of it; the file table on synthetic records and a small volume image whose table is in two pieces: boot sector, update sequence, data runs, an extension record, a hard link, a junction, a WOF file, a deleted record and a reused folder; the snapshot's bytes; the helper over its pipe) and `scripts/tests/disk-analyzer.test.mjs` (the layout, the colours, the rows, the page state). `cargo test measure_a_whole_drive -- --ignored --nocapture` scans `C:\` (or `MYLE_SCAN_ROOT`) and prints the numbers; from an elevated terminal, `cargo test mft_matches_the_walk -- --ignored --nocapture` reads `C:`'s file table (or `MYLE_MFT_DRIVE`'s), walks the same drive and prints both.

### Auto Clicker & Macros

Repeat clicks, or a sequence of recorded points (`backend/src/autoclicker/`, page in `frontend/app/pages/auto-clicker/`). It sends real input through Win32 `SendInput`, so it works in any program.

- **Two modes.** *Simple clicker* clicks wherever the cursor is; *multi-point sequence* moves to each saved screen coordinate and performs its configured clicks there. Negative coordinates are allowed, for a monitor to the left of or above the main one.
- **Timing.** Millisecond intervals internally (10 ms minimum, 24 hours maximum); the simple editor also shows seconds. In a sequence, the interval separates clicks and the transition delay runs after each step, including before a repeated pass. A click count counts one single or double action, not the down/up pair.
- **Repeat.** A finite count must be positive; unlimited runs until Stop or F6. Simple mode takes an optional finite click count through the same control.
- **Countdown.** A 3-second start countdown, cancellable, leaves time to move away from the Start button. Every wait (countdown, interval, transition) is interruptible, and Stop never leaves a mouse button held down.
- **F6.** Registered with `MOD_NOREPEAT` when the page is first opened and kept while MYLE runs, also after navigating away. It starts the last valid profile or stops a running or picking operation. A conflicting registration is reported on the page; nothing is silently replaced. A profile never starts by itself when the app opens.
- **Pick a location.** Temporarily hooks physical left clicks outside MYLE and consumes the chosen down/up pair, so the target is not activated; injected clicks are ignored. It cleans up on success, on Stop/F6/Escape, on a 30-second timeout and on exit. Clicks inside MYLE's window still reach it, so Cancel and moving the window work.
- **Profiles.** Versioned JSON in the roaming folder, with the active selection, validated bounds and unique ids (100 at most). Writes are atomic and keep the previous file on error; a malformed file is reported, never silently overwritten.
- **Code.** `model.rs` validates, `engine.rs` runs the schedule (driven by a fake driver in tests, so no real clicks), `native.rs` is the Win32 input, `store.rs` persists, and `mod.rs` orchestrates and emits `autoclicker-status`. The page is `api.ts` + `model.ts` + `state.svelte.ts` + `AutoClicker.svelte` + `StepEditor.svelte`.
- Tests: `cargo test autoclicker` (malformed and oversized configs, finite/infinite boundaries, click and step counts, cancellation during countdown, interval and transition, worker exclusion, profile round-trips, the native event builder's paired events) and `scripts/tests/auto-clicker.test.mjs`, `auto-clicker-state.test.mjs`.

### Password Manager

An end-to-end encrypted vault (`backend/src/passwords/`, page in `frontend/app/pages/password-manager/`). The vault itself — encryption, entries, 2FA codes, passkeys, imports, website icons, sync and the account's Supabase sign-in — is the `myle-vault` crate in `backend/vault/`, shared with MYLE Passwords for phones. `backend/src/passwords/` keeps what only Windows does (Windows Hello, the clipboard, the browser extension's host, filling Windows programs, reading pictures with WIC) and the commands.

- **Keys.** The master password gives a master key through Argon2id (64 MiB, 3 passes). A random vault key encrypts each entry on its own with XChaCha20-Poly1305. The associated data is `entry id | revision`, so a ciphertext cannot be moved to another entry or rolled back unnoticed. The vault key is stored only wrapped: by the master key, and by a 52-character recovery code shown once. Changing the master password re-wraps it; entries are not encrypted again.
- **On this PC.** `%APPDATA%\ThomasThanos\MakeYourLifeEasier\passwords.vault` holds only ciphertext (titles, user names and addresses are encrypted too). Keys and decrypted entries live only in Rust and are wiped (`zeroize`). The page never gets the vault key; a password reaches it only when revealed. Copying goes from Rust to the clipboard, marked to stay out of clipboard history and cloud clipboard, and is cleared after 30 seconds. The vault locks after the chosen idle time (5 minutes by default), whenever Windows locks and after the PC wakes. Only what the user does counts as use: the page, and filling, saving or a new password from the extension at the user's click; look-ups from the browser and the sync never keep it open.
- **Windows Hello.** Optional, per PC (`passwords/hello.rs`); opening the page asks it at once if the window is in front (never when the vault locks while the user is on the page). Turning it on makes a Windows Hello key (`KeyCredentialManager`, in the TPM where there is one) and has it sign a random challenge; that signature (RSA PKCS#1 v1.5, the same every time for the same data) is hashed into a key that wraps the vault key. Only the wrapped key and the challenge are stored, sealed with DPAPI in `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data\passwords-hello.bin`, never synced. Opening the vault this way has Windows Hello sign again, which it does only after the user's PIN, fingerprint or face. A changed master password keeps it working (the vault key is the same); another vault, or a Hello reset, turns it off and the master password opens the vault.
- **Generator.** `backend/vault/src/generator.rs`: random characters (8–128, every chosen kind at least once, no modulo bias) or passphrases from the EFF's large wordlist (`eff_large_wordlist.txt`, CC BY 3.0 US; 7,775 words, about 12.9 bits each; 3–12 words, a separator of up to 3 symbols, capitals and a digit on request). `Options` takes both, so a page that sends only the old fields still gets a password.
- **Guarding it.** After three wrong master passwords (or recovery codes) each try waits longer, doubling up to a minute. Exporting a backup asks for the master password again even while the vault is open, and an import reads only the file just picked in its dialog, never a path the page names. An entry that does not open with the vault key (damaged, or not made with it) is left out and counted instead of keeping the whole vault shut; the page says how many. A shown password, and the password history, hide again after 30 seconds.
- **Sync.** Two Supabase tables with row security per user: `password_vault` (header) and `password_items` (one sealed row per entry, deletions as tombstones). Create them with `docs/supabase/password-manager.sql`. Every change is sent with compare-and-swap on the revision the PC last saw. An entry changed on two PCs is merged once unlocked: the newer change wins and the other password goes into the entry's history. Deletions are sealed with the vault key too (`myle-tombstone|id|revision`), and nothing from the account is taken in while the vault is locked: every row must open with the key first, so someone with the account alone can neither add nor delete entries (a row that does not open is refused, and this PC's version goes back over it one revision up). A second PC takes the account's vault and opens it with the same master password.
- **Website icons.** The list shows each website's icon (`vault/src/icons.rs`, `Favicon.svelte`; on by default, off from ⋯, which deletes them). The app asks each website itself, so no icon service learns the vault's sites: only public names over https, never an IP address, `localhost` or a local, `.arpa` or `.onion` name, following redirects itself and turning `http://` ones into `https://`. It reads the home page up to `</head>` for `<link rel="icon">` and Apple touch icons (or a `<meta http-equiv="refresh">` page), then `/favicon.ico`, and keeps an answer only if its first bytes are an image's (PNG, ICO, GIF, JPEG, WebP or SVG, 200 KB at most, drawn by `<img>` so an SVG's scripts never run). Icons are kept in `passwords-icons.bin` next to the vault, sealed with the vault key, read when the vault opens and fetched again after a month (a site without one after a week). Sites that block background clients use the icons bundled in `backend/vault/src/site-icons/` (see its README), for those exact registered domains and their subdomains only. The extension gets a site's icons from that cache with its answer; the browser never fetches them.
- **2FA codes.** An entry can keep a TOTP key (`vault/src/totp.rs`, RFC 6238: SHA-1/256/512, 6–8 digits, 10 s–5 min), sealed in the entry as an `otpauth://totp/…` link, so it syncs and backs up with it. The editor takes the key or its link, or reads a QR code (`vault/src/qr.rs`, `rqrr`) from a snip on the clipboard (`CF_DIB`) or a picture file (Windows' own decoders, WIC); the page only ever gets the codes (`passwords_totp`, which never counts as using the vault) and whose key it is. The extension fills a code into a site's 2FA field (`autocomplete="one-time-code"`, a name like `otp`/`2fa`/`verification-code`, or a row of one-digit boxes) for a login saved for that site, by the same rules as a password. Entries keep fields a newer MYLE adds (`Entry.extra`), so an edit on an older copy never drops them; copies up to 9.1.1 still drop the 2FA key and passkeys of an entry they edit.
- **Browser filling.** `extension/` is the Manifest V3 extension for the Chromium browsers (Chrome, Edge, Brave, Vivaldi, Opera, Arc, Thorium); `extension/firefox/` has the manifest for Firefox and its forks (Zen, LibreWolf, Waterfox, Floorp). The shared files are copied next to it by `node scripts/prepare-extension.mjs` (they are not committed, and the bundle and `cargo` builds need them there). Both background scripts load `psl.js`, the Public Suffix List, so the extension counts `a.github.io` and `b.github.io` as two sites as the app does; after a `psl` crate update, run `node scripts/update-psl.mjs`. Chromium uses `background.service_worker`, Firefox `background.scripts`. The extension talks to a native messaging host (`com.thomasthanos.myle`): this same program, started by the browser with the extension's id (`run_native_host` in `passwords/browser.rs`). The host checks the id and that a browser started it, then passes each request over a per-user named pipe to the running app, which answers only a host that is the same program. The app answers only while "Browser filling" is on and the vault is unlocked, only for https pages (or `localhost`), and a password only for an entry saved for that site (same host, or same registrable domain by the Public Suffix List). Nothing is filled without a click. Browser filling is on by default (`Prefs.browser_filling`; a vault made before that had it turned on once, `filling_default_applied`, and the user's choice stands after that). While it is on, the app writes the host manifests to `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data\native-messaging` and registers them under `HKCU\Software\{Google\Chrome, Microsoft\Edge, BraveSoftware\Brave-Browser, Chromium, Mozilla}\NativeMessagingHosts`, with Vivaldi, Opera and Arc using Chrome's registration, Thorium Chromium's and the Firefox forks Mozilla's (the browsers the host accepts are `BROWSERS` in `browser.rs`, by program file name), and `ensure_registered` checks that on every start. The store listing is `https://chromewebstore.google.com/detail/mifjffbnaeeljjfboiglbcoaokgdilca` (`package.json` → `myle.extensionUrl`); the packed zips keep the manifest free of `key`, which is what makes the store id differ from a folder install's id.
- **Locked or closed.** When a passkey request arrives at a locked vault, or MYLE is closed on a site known to have a passkey, the prompt offers to unlock or open MYLE (`open`) and then waits for the vault (`waitUnlocked`, ended by `stopWaiting`), carrying on by itself. A status answer that names no vault state ends the wait at once and says the copy of MYLE is older than the extension, instead of polling for two minutes and then blaming the vault.
- **The extension against the page.** The page's scripts share the document, so the menu (`extension/content.js`) assumes they are hostile. It counts only real input (`isTrusted`); it lives in a closed shadow root under a random tag name, in the browser's top layer (a manual popover, shown again above anything the page put there), with its host's look pinned by `:host { … !important }` rules; a change to its element closes it. A click on it counts only after it has been on screen, still and untouched, for half a second, never as the second click of a double click, only where it is the element under the pointer, and in Chromium only while IntersectionObserver v2 sees nothing drawn over it (otherwise it says so). It never fills hidden fields or a form that sends over plain http (checked before the password is asked for). A sign-in form embedded from another site is filled only from the toolbar popup, where the user sees whose it is. A login is offered for saving only if the user typed it (or picked the suggested password): the next page gets a random nonce and the user name, never the password, and "Save" works only with that nonce. On a password step, the account the page already shows (Google's, say) comes first. The app answers the host only so many requests a minute (20 fills, checks or saves; 120 look-ups), and a page may change only the login saved for its own host (the old password goes into the history).
- **Offering to save.** A login the user typed is offered after the form is sent: on the next page, or on the same one when a web app signs in without leaving it (the form gone, or the address changed). While the vault is locked it is offered anyway: Save brings MYLE forward and saves once the vault opens, unless the vault turns out to have it. A 2FA setup page's key, written out or as its `otpauth://` link (only on a page that talks about authenticators, 2FA or a setup key), is offered to keep with the site's login (which one, when there are several), or as a new login; the toolbar popup's "2FA from a QR code" reads a QR code shown in the tab (a screenshot, `captureVisibleTab`, read by the app) the same way. New passkeys are offered in MYLE's prompt.
- **Windows programs.** A login links to a program by its full path (`AppLink.exe`; Browse… in the editor); a program that updates into a new version folder (`app-1.0.9172`, `2.4.1`, `v2.5`) stays linked (`same_program` in `passwords/windows_fill.rs`). The hotkey (Ctrl+Shift+L, or Ctrl+Alt+Shift+L when another program holds that) remembers the program in front and its focused field (Store apps are found inside `ApplicationFrameHost.exe`); MYLE then shows its linked logins: Fill both (user name, Tab, password), or one of them, typed as Unicode keystrokes after checking the window and, where Windows can tell, the field again. A program that runs as administrator gets Copy buttons, since Windows drops keys typed into it. A program with no login gets "New login for it" and "Link a login…"; a login linked by file name only (older links) is linked in one click.
- **Passkeys.** `extension/passkeys.js` runs in the page's own world at `document_start` and takes `navigator.credentials.create/get` calls that carry `publicKey` to `content.js`, which asks the user in MYLE's prompt (the same closed shadow root as the menu); "Another device", or no passkey of MYLE's for the site, calls the browser's own as before. The app (`vault/src/passkeys.rs`) is the authenticator: ES256 keys (P-256, PKCS#8 in the entry), attestation "none", MYLE's AAGUID, backup flags set and the counter always 0 (the passkey lives on every PC of the vault). It makes `clientDataJSON` itself from the address the browser reports, takes only the page's own host or a domain above it that is not a public suffix (`rp_id_for`), and signs only after the click; when the site asks for user verification, Windows Hello confirms it (`hello::verify`, owned by the window in front), or, on a PC without it and a site that insists, the master password typed in MYLE (`ask_master`, `VerifyPasskey.svelte`). A sign-in form that offers passkeys as you type (`mediation: "conditional"`) gets MYLE's in the menu on its user name field, while the browser's own request runs alongside. A new passkey goes into the login of that account on the site, or a new one, and replaces one for the same account. In a frame of exactly the page's origin (an empty helper frame, as Google's sign-in uses; `match_about_blank`, `match_origin_as_fallback`) it hands `publicKey` calls to the top page's wrapper; frames of other origins keep the browser's own. Credential ids are compared as bytes (`same_credential`: base64url with or without padding, or plain base64) and relying parties without regard to case (`same_rp`). Which sites have passkeys comes from `passkeySites` (rp ids only, while unlocked, at most every five minutes). Registration details are in `docs/passkey-provider/`.
- **Tray.** Only with "Keep running in the tray" on (Settings; `KeepInTray` in `HKCU\Software\ThomasThanos\MakeYourLifeEasier`, off when unset — the app never stays behind unless the user chose it), closing the window hides the app next to the clock (`src/tray.rs`), so Ctrl+Shift+L and browser filling keep working; the icon's menu opens the app or the vault, locks the vault, or quits. Started by Windows with "Start minimized", the app starts there (minimised in the taskbar with the tray off). With "Start with Windows" off (`StartupShortcut` 0), a Startup shortcut of ours found at start is removed, and a start from one ends at once (`startup::respect_choice`), so an older setup that put it back cannot bring the app up at sign-in. Because a closed window only hides, the setup first sets the event `Local\MakeYourLifeEasier-Quit`, on which the app exits cleanly, before it asks the windows to close (`installer/src/processes.rs`).

### MYLE Passwords (Android and iPhone)

The Password Manager on phones: see, copy and edit the logins, with their 2FA codes, in sync with MYLE on the PC. It does not fill other apps. It is distributed outside the stores: an APK for Android, and an IPA that <a href="https://sidestore.io"><kbd>SideStore</kbd></a> installs with the user's own (free) Apple ID.

- **Code.** `backend/vault` (`myle-vault`) is the vault itself, used by both apps: encryption, entries, 2FA codes, passkeys, imports, website icons, the sync and the account's Supabase sign-in (`account.rs`, `Cloud`). It knows nothing of Tauri or Windows; TLS is native-tls on Windows and rustls on phones (Mozilla's root certificates on Android, `http.rs`). `backend/mobile` answers the page with the same command names as the Windows app (`passwords_*`, `account_*`), so `frontend/app/pages/password-manager/` serves both; `frontend/lib/platform.ts` (`MOBILE`, from `vite build --mode mobile`) leaves out what only Windows does (browser filling, Windows programs, import and export, passkey prompts) and words things for a phone. `frontend/mobile/` is the phone's shell: the bar, the welcome screen, the account sheet, the camera's QR frame, the back button.
- **Account.** Sign-in shows Discord's or Google's page inside the app (`backend/mobile/plugin`, `tauri-plugin-myle-mobile`): iOS's sign-in sheet (`ASWebAuthenticationSession`, sharing Safari's sign-ins), and on Android a Custom Tab over the app (Google refuses sign-in inside an app's own webview). Both come back to `uk.thomast.myle.passwords://auth-callback` (a Redirect URL of the Supabase project, Authentication → URL Configuration): iOS hands the address straight back; on Android it arrives as a link, and a return to the app without it means the tab was closed (the sign-in is cancelled). The PKCE verifier waits in `sign-in.json` for 10 minutes, so a return that starts the app again still finishes. The session is kept in `account.json` in the app's own folder, which no other app can read.
- **Unlocking.** Face ID / Touch ID / fingerprint (`mobile/src/unlock.rs`, behind the `passwords_hello_*` commands): a random key in the Keychain or Keystore (`tauri-plugin-biometry`, pinned), which the phone hands back only after the user's face or finger, wraps the vault key; only the wrapped copy is kept, in `passwords-biometry.json`. The vault locks after the chosen idle time, and when the user comes back after a minute away (`passwords_app_hidden`); the page is blurred while the app is out of sight. Copied passwords leave the clipboard after 30 seconds.
- **Builds.** `.github/workflows/mobile.yml` makes everything on GitHub's runners: `tauri android init/build` (aarch64 + armv7, then `zipalign` and `apksigner` with the key in the `ANDROID_KEYSTORE_*` secrets) and `tauri ios init/build --no-sign` on macOS with Xcode 26. A push to a `mobile/…` branch, or a run by hand, keeps the APK and IPA as artifacts. A release tag `vX.Y.Z` builds them too when `backend/mobile/tauri.conf.json` and `Cargo.toml` have that version, and they go into the same GitHub release as `MYLE.exe` (`release.yml` and `mobile.yml` each create it or add to it, whichever finishes first; the Windows app's run marks it "latest"). A `mobile-v*` tag is a phone release of its own that never becomes "latest". Either way they go to R2 with `sidestore.json` and `mobile-latest.json` (`scripts/mobile-feeds.mjs`). The Android app checks `mobile-latest.json` (`mobile/src/update.rs`), downloads the new APK into its own cache folder (each version has its own address, `MYLE-Passwords-<version>.apk`, so no cache can hand out the previous one), checks its SHA-256 and opens Android's installer (the plugin; the first time, Android's "install unknown apps" setting for the app). Android installs it over the old one only because it is signed with the same key, so that key (backed up outside the repo) must never be lost. The plugin is an ordinary Tauri plugin crate: its Android library (`REQUEST_INSTALL_PACKAGES`, `androidx.browser`) and Swift package are built into the projects `tauri android/ios init` makes; the workflow only raises Kotlin to 2.1 (for the biometrics plugin) and puts the iOS icon's light, dark and tinted looks (`backend/mobile/ios-icon`, `backend/icons/app-icon-*.png`) into the asset catalog. Android credentials are described in `docs/android-credentials.md`.
- **Installing.** Android: open `https://downloads.thomast.uk/MYLE-Passwords.apk` on the phone and allow installing from the browser. iPhone: in SideStore, add the source `https://downloads.thomast.uk/sidestore.json` and install MYLE Passwords; a free Apple ID re-signs it every 7 days (SideStore does it in the background) and allows 3 such apps, SideStore included.

### Settings

`frontend/app/pages/settings/`, `group: "bottom"`. Four sections, searched by one box (Ctrl+F or `/`):

| Section | What |
|---|---|
| **Account & sync** | Discord or Google sign-in, "Sync now", sign out, and what is stored in the account versus what stays on this PC |
| **Appearance** | The theme (default, matte), previewed as a small mock of the shell |
| **Startup & tray** | Start with Windows, start minimised, keep running in the tray |
| **Updates** | Check now, the update's progress, and what the last check found |
| **About** | Version, the GitHub link, "What's new" |

The choices are written to `HKCU\Software\ThomasThanos\MakeYourLifeEasier` (start-up, tray) or to the settings file (`theme`), and the setup keeps its own remembered shortcut state in step (`src/startup.rs`, `src/shortcut_preferences.rs`).

---

## 6. Owner-only pages

GitHub Releases and Project Backups are for the app's owner only (`backend/src/account/owner.rs`, the address in `OWNER_EMAIL`). The backend decides, never the page:

- **Who.** After sign-in, and again when the window comes back after 10 minutes, the backend asks Supabase for the user (`GET /auth/v1/user` with the session's access token, so Supabase checks the token) and admits only an account whose email is the owner's (any case), confirmed, signed in with Discord (`app_metadata.provider`), with a Discord identity of that email marked verified. Nothing the webview sends counts: the page can only ask for a recheck (`account_access`).
- **Kept.** The verdict is tied to the user id, kept in memory and sealed with DPAPI in `account-access.bin` (roaming folder), so the pages work offline for 30 days after the last check. Signing out, another account or an expired session clears it.
- **Enforced.** `owner_gate` in `lib.rs` wraps the invoke handler: every command named `github_releases_*` or `project_backups_*` is refused unless the verdict admits the session's user. A test reads `lib.rs` and fails if one of those modules' commands is registered under another name.
- **Live.** The backend sends `account-access` (true/false) on every change. On false the sidebar hides both pages (`ownerOnly` in `registry.ts`), an open one falls back to the first page (deep links and the start page too), and running builds, releases, backups and previews are cancelled.

---

## 7. Where things live on disk

| What | Where |
|---|---|
| Program | `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\MYLE.exe` (existing installs keep this folder) |
| Settings, account, Game Saves, Project Backups, GitHub Releases, the vault | `%APPDATA%\ThomasThanos\MakeYourLifeEasier` |
| Cache, WebView2 (localStorage), JSON stores, undo records | `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data` |
| Desktop shortcut | the Windows Desktop known folder (may redirect to OneDrive) · `MYLE.lnk` |
| Start menu shortcut | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\MYLE.lnk` |
| Launch with Windows (opt-in) | `…\Start Menu\Programs\Startup\MYLE.lnk` (with `--autostart`) |
| Uninstall entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MakeYourLifeEasier` |
| Game Saves scheduled task | `MakeYourLifeEasier Game Saves Backup` (legacy name, kept for existing schedules) |

Install is all-or-nothing: each file is written next to the old one, and if something fails partway through the previous version is restored. `install.json` in the folder records which files the setup put there, so update and uninstall only remove those.

Versions up to 7.0.x kept their data in `com.thomasthanos.makeyourlifeeasier` folders. The app moves them once, whole, with a single rename (`src/storage.rs`). If a folder is in use (during a live update the old version is still running) it uses the old one that time and retries on the next launch — a half-finished move never happens, since that would corrupt the WebView2 profile.

---

## 8. Updates and releases

The old Electron app (v4.x) lives on the `old` branch and no longer receives updates; the current line starts at **7.0.0** and is at **10.1.3**. The old `latest.yml` on R2 stays untouched, so old installs simply see "up to date".

### Release

1. Bump the version in **both** `package.json` and `backend/Cargo.toml`, and commit.
2. `git tag v10.2.0 && git push origin main v10.2.0`
3. `.github/workflows/release.yml`:
   - checks the tag and the versions agree, runs `svelte-check`, and builds (downloading Ludusavi too)
   - **signs** the exe, installer and uninstaller with the certificate from `WIN_CSC_LINK` / `WIN_CSC_KEY_PASSWORD` (`scripts/sign.ps1`)
   - uploads `MYLE.exe` to **Cloudflare R2** (`downloads.thomast.uk`, `R2_*` secrets), keeps the old `MakeYourLifeEasier-installer.exe` URL alive for existing links, publishes `latest.json`, and checks R2 serves the exact same bytes
   - publishes the GitHub release too

To test without releasing: Actions → Release → **Run workflow**. It builds and signs the same way, keeps the installer as an artifact, and uploads and publishes nothing.

### On every launch

The splash:

- reads `https://downloads.thomast.uk/latest.json` (`UPDATE_FEED` in `src/updater.rs`); if that does not answer it asks `api.github.com/repos/thomasthanos/MYLE/releases/latest`
- if a newer version is found, downloads the installer (only from `downloads.thomast.uk` or `github.com`) and checks its SHA-256 (from the feed or GitHub's digest; with no hash it will not install)
- runs the setup silently with `/S /UPDATE /LIVE` **while the app is still open**: files are swapped in with a rename (Windows allows this even for a running exe), the old ones stay as `*.myle-old`, and the new version deletes them once it starts
- opens the new version with `--just-updated` (it does not ask the network again and shows "Updated to v…"), and closes as soon as its window appears; before that it drops the single-instance lock, otherwise the new copy would hand its arguments to the old one and quit
- if the live update fails (or the app is not running from the install folder), it falls back to the setup's progress window (`/P /UPDATE /R`), which waits for the app to close and opens the new version half a second before closing itself
- the installer reopens the new version

With no network the app opens normally after about 2 seconds (the bar counts down).

> The SHA-256 check catches files that were corrupted or changed along the way. It does not protect against the GitHub or Cloudflare account itself being compromised.

### What's new

`src/whats_new.rs`, `frontend/lib/components/WhatsNewHost.svelte`: on the first start of a new version the app shows the notes of every version since the one it last showed (kept in `whats-new.json` in the settings folder), newest first. They are the GitHub release bodies, asked for by Rust without a sign-in (the web view's CSP only reaches the app) and cached in `whats-new-cache.json`; offline or rate-limited, the cache, then this version's `docs/release-notes/<version>.md` (bundled by `backend/build.rs`), then a line with a link stand in. Markdown is read by `frontend/lib/markdown.ts` into plain data rendered as text: raw HTML is reduced to its text, pictures are left out, and only http(s) links are kept, opening in the browser. Versions up to 9.8 never wrote `whats-new.json`; when it is missing but MYLE ran on the PC before (its settings folder has files, or its web view profile exists), this version's notes are shown, and on a first install nothing is. Settings → About → **What's new** shows them again. For everyone, not only the owner.

### The Cloudflare Pages site

`make-your-life-easier.pages.dev` is a static download and marketing site and does not serve the Tauri webview. Its files live in `site/`, and the root `wrangler.toml` declares `pages_build_output_dir = "./site"`. The Pages build command stays empty; it must not point at the Vite build or run `npx vite build`.

- Main download: `https://downloads.thomast.uk/MakeYourLifeEasier-installer.exe`
- GitHub fallback: the repository's latest release
- `/installer.html` redirects to `/` for compatibility with old links
- `scripts/check-site.mjs` checks required files, internal links, security headers, and stale Electron/Portable references (run by `npm run check`)
- The R2 objects, `latest.json` and the release workflow are independent of the Pages deployment

---

## 9. Setup and uninstall

The setup is our own: the `backend/installer` crate (Rust) with a Svelte window (`frontend/installer/`, `frontend/installer.html`) in the same dark style as the app. It builds two programs.

**`setup.exe`** carries the app as one solid XZ payload (`installer/src/payload.rs`). It shows the install folder, toggles for Desktop / Start menu (All apps) / taskbar pin / launch with Windows / open when finished, per-file progress, and a completion screen. On the same version it plainly says "Reinstall" and keeps settings and data. It repairs the app's own recognised shortcuts and checks their targets, working folders and stable `MYLE.ico` icon; shortcuts and the process share an AppUserModelID. The Startup shortcut launches with `--autostart`. The choices are remembered in `HKCU\Software\ThomasThanos\MakeYourLifeEasier` (`DesktopShortcut`, `StartMenuShortcut`, `StartupShortcut`) and offered again next time. Missing selected links are repaired on updates; explicit opt-outs in the setup or in Settings remain off. `TaskbarShortcut` stores the taskbar checkbox separately from `TaskbarPinPending`. Only an interactive setup request passes `--pin-taskbar` to the foreground app to show Windows' confirmation; passive updates keep both values and never auto-prompt, and a denied pin is not reported as success. A shortcut Windows refuses (retried a few times, since the shell and virus scanners open new `.lnk` files at once) does not fail the install: the app is installed and the completion screen lists what is missing. A dead link, or one to an older copy of the app, under our shortcut name is taken over rather than becoming a second entry beside it. If the app is running, it asks you to close it before changing files. With "Open when finished" checked it opens the app and closes the setup once the install succeeds.

**`uninstall.exe`** sits next to the app and is what Windows runs from "Installed apps". It asks whether to also remove settings and data (`%APPDATA%\ThomasThanos\MakeYourLifeEasier` / `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data`, cache), and cleans up the older `com.thomasthanos.makeyourlifeeasier` folders. Game Saves backups are never touched. Because a program cannot delete its own file while running, `uninstall.exe` (like NSIS's) copies itself to a new folder in `%TEMP%` and runs from there (`installer/src/relocate.rs`): the copy does the work and, once the original has exited, deletes it and the folder too. The `/S` exit code still reaches whoever ran it, and old copies in `%TEMP%` are swept up on the next uninstall. Uninstall also removes the Game Saves scheduled task, and only the shortcuts that point to our own app.

Command line (the same as NSIS's, which the updater already speaks):

| Flag | What it does |
|---|---|
| `/S` | no window |
| `/P` | progress only, starts at once and closes itself (the updater runs `/P /UPDATE /R`) |
| `/UPDATE` | shortcuts stay as the user left them |
| `/R` | opens the app afterwards |
| `/NS` | no shortcuts |
| `/LIVE` | (with `/S /UPDATE`) update while the app is running; the app reopens itself |
| `/D=<folder>` | a different folder (last, unquoted); testing only, the window does not change the folder |
| `/PURGE` | (uninstall) also removes settings and data |

For each shortcut the setup remembers whether it is wanted and whether it was made (`0` off, `1` wanted but not made yet, `2` made). `/UPDATE` (and the in-app update) retries a wanted one that never got made; one that was made and has since gone was deleted by the user, so it is not brought back and is saved as off (the setup then offers it unticked). Updates refresh the others of ours and remove none. Uninstall turns `2` back into `1`, so a reinstall offers the same shortcuts. The first update over an install from before any of this was remembered adds the Start menu entry and turns the old app's `Run` value into a Startup shortcut.

- Starting with Windows is off unless chosen: in the setup ("Start with Windows", off for a new install and for `/S`) or in Settings, which adds or removes the same Startup shortcut and keeps the setup's remembered state in step (`src/startup.rs`). Game Saves' scheduled backups do not need it: they run from their own scheduled task (`--game-saves-auto-backup`, no window).
- When the app opens from Startup (`--autostart`), the main window starts minimised to the taskbar unless "Start minimized" is off. The choice is `StartMinimized` in `HKCU\Software\ThomasThanos\MakeYourLifeEasier`, read in `finish_startup` (`src/lib.rs`); unset means minimised.
- Existing installs may keep a `MakeYourLifeEasier.exe` compatibility copy, which hands normal launches to `MYLE.exe` so an old updater or scheduled backup keeps working through the transition. New installs contain only `MYLE.exe`.
- After an install or uninstall, `scripts/verify-install.ps1` (or `-Removed`) checks all of the above.

Preview the window in a browser: `npx vite` and `http://localhost:1420/installer.html?demo=install` (or `=update`, `=reinstall`, `=uninstall`, `=running`, `=error`, `=launch-fail`, `=passive`).

---

## 10. Tests

| Where | What |
|---|---|
| `cargo test --workspace` | everything in Rust, including every feature's unit and integration tests |
| `node --test "scripts/tests/*.test.mjs"` | everything outside Rust: the extension's sites and manifest, the pages' pure logic, the setup's version order, the site check |
| `scripts/smoke-test-setup.ps1` | a silent install, an update and an uninstall, with checks (CI) |
| `scripts/verify-install.ps1` | one install, or with `-Removed` one uninstall |
| `cargo test <feature>` | one feature's tests, e.g. `cargo test project_backups` |

Some suites only make sense on real Windows and are `#[ignore]`d or gated:

- the debloater's `sandbox_tests.rs` (Windows Sandbox or a VM, see [Windows Optimization](#windows-optimization))
- `cargo test measure_a_whole_drive -- --ignored` and `cargo test mft_matches_the_walk -- --ignored` (a real drive, see [Disk Analyzer](#disk-analyzer))

`npm run check` catches what the tests do not: svelte-check for every component, `tsc` for the Vite config and the scripts, and the site check.

---

## 11. Conventions

- **The page asks, Rust decides.** A page sends ids, never paths, URLs or command lines. Every id is looked up in a table that is fixed at compile time (`targets.rs`, `tasks.rs`, `catalog.rs`, `actions.rs`, `custom-apps.json`), and the administrator helper re-checks whatever it is given against the same table.
- **Nothing needs administrator rights until it truly does**, and then it is one UAC prompt through `elevated_pipe`, with the same program on both ends.
- **A long operation is cancellable** and reports progress over a channel; a second one waits (the `Jobs` registry).
- **Destructive actions ask first**, name what they will touch, and never fall back to something worse on their own.
- **Files are written atomically** (a temporary file next to the target, then a rename), and a malformed file is reported rather than silently replaced.
- **Secrets never reach the page**: API tokens, the GitHub token, the AI keys and the vault key stay in Rust, sealed with DPAPI or the vault key.
- **Comments explain why**, not what. A refactor is formatting only when it changes no compiled output — `.preview/` has the checks used to prove that (`style-identical`, `only-whitespace`, `blank-lines-only`, `comments-same`, `build-identical`).
