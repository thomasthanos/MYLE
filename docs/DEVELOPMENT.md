# Make Your Life Easier · for developers

Everything about the build, structure, pages, updater and setup. For the app itself, see the [README](../README.md).

## Development

### Requirements

- Node.js 20+
- Rust stable with MSVC: `winget install Rustlang.Rustup`
- Visual Studio 2022 Build Tools with the `Desktop development with C++` workload
- Windows 10/11 and the WebView2 Runtime

### Core commands

~~~powershell
npm install
npm run dev       # app with hot reload
npm run build     # the full Windows setup
npm run check     # svelte-check, TypeScript and site checks
npm test          # Rust tests for the app and the setup, then the script tests
npm run lint      # cargo clippy -D warnings
~~~

The rest runs directly:

| Command | What it does |
| --- | --- |
| <code>npx tauri build --no-bundle</code> | Release exe of the app, without the setup |
| <code>npx vite</code> / <code>npx vite build</code> | Vite for the app; <code>npx vite</code> also serves the pages to a browser |
| <code>npx vite build --mode setup</code> | Builds the setup/uninstaller UI |
| <code>npm run web:mobile</code> | Builds MYLE Passwords' page (<code>vite build --mode mobile</code>); <code>npx vite --mode mobile</code> serves it at <code>/mobile.html</code> |
| <code>cargo run -p myle-passwords</code> | MYLE Passwords on Windows, to try it (no Face ID, sign-in or camera there) |
| <code>scripts/verify-install.ps1</code> | Checks an install, or with <code>-Removed</code>, an uninstall |
| <code>scripts/bootstrap-game-saves.ps1</code> | Downloads and verifies the pinned Game Saves resources |
| <code>node scripts/prepare-extension.mjs</code> | Copies the extension's shared files into the Firefox folder and packs the store zips |
| <code>node scripts/update-psl.mjs</code> | Writes the extension's Public Suffix List (<code>extension/psl.js</code>) from the <code>psl</code> crate the app uses |
| <code>node --test "scripts/tests/*.test.mjs"</code> | Tests for code outside Rust: the extension's sites, the setup's version order |

Every push to main and every pull request runs through .github/workflows/ci.yml: frontend checks, Rust lint/tests and a Windows install smoke test. Each CI run's setup is kept as an artifact for 7 days.

Tauri's `beforeDevCommand` and `beforeBuildCommand` (in `backend/tauri.conf.json`) run the Game Saves bootstrap and the extension preparation, so `npm run dev` and `npm run build` need nothing else. To preview the installer, run `npx vite` and open `http://localhost:1420/installer.html?demo=reinstall`.

The main icon lives at `backend/icons/app-icon.svg`. The app logo and splash share the shapes from `frontend/lib/brand.ts`.

## Structure

```
.github/workflows/            ci.yml (every push/PR), release.yml (tag v*), mobile.yml (tag mobile-v*), keepalive.yml
scripts/
  build-setup.ps1             the whole setup (npm run build)
  bootstrap-game-saves.ps1    download and SHA-256 verification of the pinned resources
  sign.ps1                    exe signing (release.yml)
  smoke-test-setup.ps1        silent install → update → uninstall with checks (CI)
  verify-install.ps1          checks a single install
  check-site.mjs              checks site/
  update-psl.mjs              extension/psl.js from the psl crate's list
  tests/                      node --test tests for code outside Rust
site/                         static download site (Cloudflare Pages, wrangler.toml)
frontend/                     frontend (Svelte 5 + TS), Vite root
  index.html                  main window → main.ts → app/
  splash.html                 updater window → splash.ts → splash/
  installer.html              setup/uninstaller window → installer/
  mobile.html                 MYLE Passwords for phones → mobile/ (the Password Manager page in its own shell)
  app/App.svelte              layout: titlebar + sidebar + content
  app/shell/                  Titlebar, Sidebar, ContentArea
  app/pages/registry.ts       ← add pages here
  app/pages/<page>/           one page per folder (UI, state, typed IPC)
  app/account/                Discord/Google sign-in and settings sync
  installer/                  the setup/uninstaller window (Svelte)
  splash/                     updater screen (animated logo, status, progress)
  lib/                        shared: toast, confirm, updater, brand, components/
  styles/                     tokens.css (colors, sizes, motion, the dark theme), glass.css (raised panels)
  public/                     static files shipped with the app (icons/)
backend/                      backend (Rust + Tauri)
  src/lib.rs                  startup, splash → main
  src/<feature>/               apps, game_saves, project_backups, github_releases, cloud, cleaner, spotify_hub, account, …
  src/updater.rs               updater (feed on R2, GitHub Releases as a fallback)
  installer/                  setup.exe + uninstall.exe (Rust, its own Tauri window)
  vault/                      myle-vault: the Password Manager's vault, sync and account, shared with mobile/
  mobile/                     MYLE Passwords for Android and iPhone (Tauri; gen/ is made by CI)
  mobile/plugin/              its native code (Kotlin, Swift): sign-in inside the app, installing updates, fitting the screen
  icons/                      app-icon.svg (the source) and the PNG/ICO built from it
  resources/                  what ships next to the exe (Ludusavi, Spicetify)
  tauri.conf.json             name, version, windows, resources (and for the setup)
vite.config.ts                one config for everything: `vite build` → backend/target/web/, `--mode setup` → backend/target/web-setup/, `--mode mobile` → backend/target/web-mobile/
```

Not in git: `node_modules/` (npm) and `backend/target/`, where **every** build goes: Rust, Vite (`web/`, `web-setup/`) and the setup (`release/bundle/setup/`).

### A new page

1. Create `frontend/app/pages/MyPage.svelte` and start it with `<PageHeader title="…" />`.
2. Add one line to `registry.ts`: `{ id: "my-page", label: "My page", icon: SomeIcon, component: MyPage }`.

Icons come from [Lucide](https://lucide.dev/icons), e.g. `import Star from "@lucide/svelte/icons/star"`.
For cards inside a page, use the `.surface` class; `.glass` is for the big raised panels (always solid). Colours come from `tokens.css`, so they follow the dark theme (`:root.dark`).

## The "Install Apps" page

An app store on top of **winget**, in `frontend/app/pages/install-apps/` (frontend) and `backend/src/apps/` (Rust).

| File | Content |
|---|---|
| `data/apps.json` | The fixed list (winget IDs, site, `selfUpdating`, `iconDomain`/`icon`) and the **app packs** |
| `backend/catalog/custom-apps.json` | The "special" apps outside winget: resolver (`github` / `page` / `static`), install kind (`installer` / `portable` / `zip`), detection, `activate` |
| `categories.ts` | Keywords for the automatic categories (override with `"category"` in the JSON) |

- Status (green = installed, blue = update, grey = not installed) comes from `winget list`. Special apps are detected from the registry or a file.
- Anything `selfUpdating` never counts toward updates.
- **Search:**
  - looks in the list first
  - if nothing turns up, searches winget live ("More from catalog")
  - whatever you check there stays "Pinned"
- **Install via winget**, with three attempts:
  1. `--silent`
  2. without `--silent`
  3. `--scope user`
- **Hash mismatch:**
  - the app asks first
  - if you agree, a UAC prompt briefly opens `InstallerHashOverride`, installs, and closes it again
- **Special apps:** the resolver finds the latest link (6-hour cache). A malformed SHA-256 is always rejected. Executable installers need either a valid SHA-256 or a valid Authenticode signature from a publisher pinned in the catalog (e.g. NVIDIA Corporation).
- **Safety:** the UI only sends IDs. URLs and commands only exist in the JSON baked into the binary.
- **Vencord / BetterDiscord:**
  - Install downloads the official tool (`VencordInstallerCli.exe` / `bdcli.exe`) and immediately patches Discord Stable.
  - Status comes from the files in `%LOCALAPPDATA%\Discord\app-*\resources` (the most recent `app-*`).
  - If a Discord update wipes the patch, the "Patch Discord" button reappears.
  - The two declare `conflicts` with each other, so installing one asks for confirmation while the other is active.

## The "Creative Suite" page

Cards for **your own** packages (zip or installer): download with progress, extraction, running the setup, and cleaning up temp files.
Frontend: `frontend/app/pages/creative-hub/`. Backend: `backend/src/apps/creative.rs`.

The list is **only** `backend/catalog/creative-apps.json`, baked into the exe at build time. The user can't change it: any change needs a new build.

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

- `source`: `gdrive` (a file id **or the whole** Drive link), `static` (any https URL: R2, B2, Dropbox, your own server), `github`, `page`.
- `setup`:
  - `zip`: unpacks and runs `run` (missing, it finds a `setup.exe`/`.msi` on its own). With `"onlyRun": true` it extracts **only** that file from the zip, useful when the zip holds a whole project.
  - `installer`: runs a plain exe/msi.
  - `extract`: only unpacks, into `to` (default `%USERPROFILE%\Downloads\<name>`).
  - `"password": "…"` for locked zips (AES or classic).
- `icon` (optional):
  - `"/icons/app.svg"` for a file inside `frontend/public/icons/`, shipped with the app
  - `"https://…"` for an image from the web
  - a file path, e.g. `%USERPROFILE%\Pictures\logo.png`, read by the backend (local only)
  - Prefer **SVG** for logos, or 256×256 **PNG/WebP** with transparency. Accepted: svg, png, webp, jpg, gif, avif, ico, up to 4 MB.
- **Where files go:** the package downloads to `%USERPROFILE%\Downloads\` and unpacks into `%USERPROFILE%\Downloads\<name>\`. The installer runs from there, and if it asks for admin rights, UAC appears.
- **Cleanup:** the downloaded file and the folder unpacked for an installer are deleted **when the app closes**, not sooner — so an installer that restarts itself never loses its files mid-run. The `extract` destination (e.g. photos) is **never** deleted.
- Percentage progress shows for both the download and the extraction, along with the file name.
- If `run` doesn't match any file in the zip, the error message shows which `.exe`/`.msi` the package actually contains. A malformed JSON shows an error with the line, instead of being silently ignored.

### Google Drive

Uses the direct endpoint `https://drive.usercontent.google.com/download?id=<ID>&export=download&confirm=t`,
which skips the "can't scan this file" page.

- The file must be shared as **"Anyone with the link"**.
- Google Drive has a **daily per-file download quota**. Once it's hit, it returns a web page instead of the file, and the app shows that as a clear error. The app can't work around it.
- For large or often-downloaded packages, **Cloudflare R2** (no egress fees), **Backblaze B2**, or **GitHub Releases** (up to 2 GB/file) are better, with `"source": { "type": "static", "url": "…" }`.

## The "Game Saves" page

The page sits after Install Apps and uses a bundled **Ludusavi v0.31.0** with its own config directory, so it never touches a user's personal Ludusavi install.

- A normal scan is offline (`--no-manifest-update`) and recognizes saves from the pinned database along with the user's custom games.
- `Update database` is the only explicit action that talks to the manifest source. If the download or validation fails, the previous database is kept atomically.
- Backups are ZIP/Deflate level 6, with three full snapshots per game and no differential snapshots.
- Restore runs a fresh preview, asks for confirmation, and keeps a safety copy for seven days. The last restore can be undone with `Undo last restore`.
- The frontend sends opaque IDs. Titles reach Ludusavi over stdin, never as arbitrary command arguments.
- Settings are saved atomically in the app data, along with launcher roots, custom games, exclusions, path mappings, database metadata, and the last scheduled result.
- Daily/Weekly auto-backup creates a per-user Windows Scheduled Task with `StartWhenAvailable`, no elevation and no wake-from-sleep. `Off` and uninstall both remove the task.
- A scheduled run ends with a notice of MYLE's own (`game_saves/notice.rs`, page `frontend/notice/`): the headless backup starts `MYLE.exe --game-saves-notice=<base64 JSON>`, a run of the app with only a small frameless, transparent, always-on-top window at the bottom right of the main screen's work area, which does not take the focus, plays a short chime (Web Audio, no sound file) and closes itself after a few seconds (longer after a failure, and not while the pointer is on it). It has its own WebView2 profile (`webview-notice`). Clicking it starts `MYLE.exe --open-game-saves`: a running MYLE is focused and goes to Game Saves. Preview it with `npx vite` at `/notice.html?kind=backedUp` (or `nothingNew`, `failed`).
- A backup folder whose drive is not connected (Google Drive's G: while Google Drive is not running) does not stop a scan: the saves on this PC are listed with an unknown backup status and the page says the backups are out of reach. Backups and restores stop with the same explanation.
- **Cloud backup**: buttons for Dropbox, Google Drive, MEGA and OneDrive. Clicking one creates `<cloud>\Make Your Life Easier\Game Saves Backups` and makes it the backup folder (never the root of the cloud folder). Only each app's local synced folder is used, with no cloud APIs or accounts:
  - **Dropbox**: `info.json` in `%LOCALAPPDATA%` or `%APPDATA%\Dropbox` (personal and business), else `%USERPROFILE%\Dropbox`.
  - **Google Drive**: Drive for desktop's drive (from its registry settings or a drive named "Google Drive") and its "My Drive" folder in whatever language, or the mirror folder in the profile.
  - **MEGA**: its settings are encrypted, so `MEGA` / `MEGAsync` is looked for in the profile and in Documents.
  - **OneDrive**: the Windows `OneDrive*` environment variables.
  - If a service isn't found, its button asks for its folder and creates the backup folder inside it. Earlier backups stay in the old folder.

Generated binaries/manifest are ignored by Git. On a fresh checkout, `npm run dev` or `npm run build` fetches them (or run `scripts/bootstrap-game-saves.ps1`). The pinned URLs and SHA-256 values live in `scripts/bootstrap-game-saves.ps1`, and the licenses/attributions ship in the installer from `backend/resources/ludusavi/`.

## The "Project Backups" page

Zips of project folders in Google Drive or Dropbox (`backend/src/project_backups/`, page in `frontend/app/pages/project-backups/`), a port of the standalone backup_projects app (desktop-utils) that writes the same folders and names, so both apps add to one history:
`<cloud>\Projects Backup\<AppName>\<YYYY-MM Month>\<AppName>_D<day>_V<n>.zip`.

- **Names.** Month folders are named in Greek as backup_projects does and read in any language (`2026-10 October` too). The version carries on from the highest `<AppName>_…_V<n>` in any month folder (`_D_V`, `_V_D` and `_V` names, zips and the old folder backups); `__partial__` leftovers and `all - pre release backups` never count.
- **A backup.** The folder is walked with the exclusions (`walk.rs`), the zip is streamed (Deflate 6) to `%LOCALAPPDATA%\…\data\project-backups-staging`, every entry is read back against the size and CRC-32 read from the source, and the folder is walked once more, trusting nothing but metadata, to make sure no file is missing (`audit`). Only then is it copied next to the backups as `__partial__<name>__<ms>.zip` (hashed with SHA-256 while it is copied), re-read until its size and SHA-256 match (stale cloud views are retried for about a minute) and renamed without replacing anything (`MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`): a zip of that name saved meanwhile, say by backup_projects, fails the backup with `NAME_TAKEN` and is never overwritten. Without room for staging it is built and checked at the destination. `.backup-info.json` inside each zip has backup_projects' fields plus totals, exclusions and the items that cannot be zipped.
- **Files Windows marks with a reparse point** (OneDrive/Dropbox placeholders, WOF-compressed files) are ordinary files to Rust's std and are backed up; links to files are followed, links to folders and junctions are listed as skipped. An online-only file whose app is not running (os error 362) starts OneDrive, Dropbox or Google Drive and the backup is tried once more.
- **Exclusions.** A global list (`rules.rs` `DEFAULT_PATTERNS`) of names that are never sources: VCS and agent folders (`.git`, `.agents`, `.claude`, `.codex`), editor caches (`.idea`, `.vs`, `ipch`), JS/TS (`node_modules`, `dist`, `.next`, `.nuxt`, `.svelte-kit`, `.vite`, `.turbo`, `.angular`, `.expo`, `coverage`, `*.tsbuildinfo`, …), Python (`__pycache__`, `.venv`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`, `*.egg-info`, …), `.gradle`, `.dart_tool`, `DerivedData`, `.build`, `xcuserdata`, `.godot`, `cmake-build-*`, `CMakeFiles`, `*.o`, `*.pdb`, `*.ilk`, logs, temp files, archives and OS junk. Each project adds its own and "always back up" patterns. `name/` is a folder anywhere, `a/b/` is anchored to the project folder, `*.log` a file. Names that are sometimes sources (`target`, `build`, `bin`, `obj`, `out`, `Debug`, `Release`, `x64`, `packages`, `Pods`, `Library`, `Temp`, `venv`, `env`, `gen`, …) are decided by markers (`detect.rs`), read from the folder's own listing and its parent's: `CACHEDIR.TAG`/`.rustc_info.json` or `Cargo.toml` next to `target` (Rust), `pom.xml` (Maven), Gradle files, `pubspec.yaml` (Flutter), `pyproject.toml`/`setup.py` (Python), `*.xcodeproj`/`Package.swift` (Xcode/SwiftPM), `CMakeCache.txt` or `CMakeLists.txt` (CMake), a `.csproj`/`.fsproj`/`.vbproj` next to `bin`/`obj` (.NET), a `.sln`/`.vcxproj` next to `Debug`/`Release`/`x64` (Visual Studio), `go.mod` next to `bin`, `pyvenv.cfg`/`conda-meta` inside (virtual environments), electron-builder's config next to `release`, `Assets` + `ProjectSettings` next to `Library`/`Temp`/`Obj`/`Logs` (Unity), `project.godot` next to `.import`, a NuGet layout next to a `.sln`, a `Podfile` next to `Pods`, Tauri's `gen/schemas`. Without a marker such a folder is left out only when git tracks nothing in it (`build`, `out`, `target`, `bin`, `obj`, `debug`, `release`, `gen`; the index is read directly, `gitindex.rs`) or a `.gitignore` lists it, and a folder git tracks files in is always kept (an installer's `build/installer.nsh`). `*.obj` is a C/C++ object file only next to a Visual Studio/CMake/Makefile project or its source; a Wavefront model stays. Following `.gitignore` is off by default; `.env` files are always backed up. Settings of 9.4.0 (version 1) are brought up to date once: the old unconditional `target/`, `out/`, `release/`, `gen/`, `venv/` and `*.obj` give way to the markers, new defaults are added and the user's own patterns stay. Tests in `detect.rs` build a fixture project per ecosystem, with the cases that must stay.
- **Preview.** The editor and the Exclusions panel show what a backup holds before it runs: the project as a tree (folders with their file count and size, at most 4,000 lines; folders and what is left out always listed), each left-out item with the rule that leaves it out (a pattern, a detected build folder, `.gitignore`), and a count per rule. A new preview stops the one still running, and closing the dialog stops it (`project_backups_cancel_preview`).
- **Destination.** It is made ready before anything else (before closing the program): a failure answers a code the page offers an action for, `NO_PROVIDER` (choose one), `CLOUD_NOT_INSTALLED` (the other cloud, or install Google Drive), `CLOUD_NOT_READY` (start it, try again) or `DESTINATION_MISSING` (choose the folder again), and the remaining projects are not tried. Every attempt is kept on the project (`lastResult`: when, ok/cancelled, code, message) for the page's status tags. Cloud apps are started detached from MYLE (`DETACHED_PROCESS`, breakaway from its job). Google Drive's drive exists only while `GoogleDriveFS.exe` runs: the newest installed version is started and the drive waited for (2 minutes, cancellable); if it comes up under another letter and only one Drive folder is found, that one is used. Dropbox is found through its `info.json` and `Dropbox.exe` is started if it is not running. The cloud detection and launching live in `backend/src/cloud/`, shared with Game Saves.
- **Before a backup** an optional program (`MyApp.exe`) is closed (`taskkill`, then forced after 1.5 s); MYLE never closes itself. A project whose folder is missing answers `SOURCE_MISSING`: the page opens the editor on the folder and the backup carries on after saving.
- **Compare** two backups, or a backup with the project folder: names are normalized (`\` → `/`, Unicode NFC, a single wrapper folder ignored when that pairs more files, case-insensitive), files are matched by size and CRC-32 (a folder side is hashed), and the same exclusions apply. A deleted file that is still in the project folder is flagged (the newer backup is incomplete). Comparing reports its progress and can be cancelled (also mid-file); only one backup or comparison runs at a time and closing the dialog cancels only the comparison. Clicking a file shows its preview, read on demand from the zip or folder (`project_backups_file_diff`): text as a side-by-side or unified diff with line numbers (`similar`, up to 2 MB a side and 20,000 rows; UTF-8/UTF-16 BOMs are read; a difference in CRLF/LF only is said so), an added or deleted text file in full, images (png, jpg, gif, webp, ico, bmp, avif, svg up to 8 MB) old next to new with SVG's source as text, other files by size and SHA-256 (up to 512 MB). The list draws 400 rows at a time; ↑/↓ step through files.
- **First start.** backup_projects' `%APPDATA%\ThomasThanos\Backup-projects\projects.json`, else its copy `Projects Backup\.backup-projects.json`, else the project folders in `Projects Backup` are imported once (by backup name; a program to close is dropped when it is MYLE). **Import** on the page runs it again for new ones. Settings: `%APPDATA%\ThomasThanos\MakeYourLifeEasier\project-backups.json`.
- Tests (`cargo test project_backups`) include a zip made by backup_projects 2.0.6 (`project_backups/testdata/`), read, compared and continued.

## The "GitHub Releases" page

Commit, build and release git projects to GitHub (`backend/src/github_releases/`, page in `frontend/app/pages/github-releases/`). It replaces the standalone Github-Build-Release app (desktop-utils), whose last project and DeepSeek key are brought over once, on first open (the plain-text key is then removed from its `grm-config.json`, and the page says so).

- **Projects.** Add a repository (a folder inside one adds the repository), or scan a folder (four levels down, not inside repositories). A monorepo (no version at its top, or npm workspaces) shows each app folder with a manifest as its own project, with the id `<repo id>/<folder>`; the backend accepts only folders the repository really has. `/` or Ctrl+F searches. Saved in `github-releases.json` in the roaming folder.
- **Status.** Branch, ahead/behind (a quiet `git fetch` when a project is opened), changed files, the versions of every version file (`package.json`, `package-lock.json`, `Cargo.toml` + `Cargo.lock`, `tauri.conf.json` unless it points at `package.json`, `pyproject.toml`, `.csproj`, an extension's `manifest.json`; in the project folder and a Tauri folder one level down) with a warning when they disagree, the last tag and GitHub release, the last Actions run, and badges for what the project is built with.
- **Git.** Run with `git -C`, no pager or prompts, `core.quotepath=false`, file paths only after `--` with `--literal-pathspecs`, and checked against the current status. Push and pull first use the user's own credentials; on an authentication failure they retry with the GitHub token as an `http.https://github.com/.extraheader` given through `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_0`/`GIT_CONFIG_VALUE_0` in the child's environment, never on a command line or in `.git/config`. Failures get codes (no identity, nothing to commit, hook failed, rejected, protected branch, secret blocked by push protection, workflow scope, no upstream, conflict, network, auth) and the page offers the fix. Pull is `--rebase --autostash`; a conflict aborts the rebase.
- **Changes.** Each changed file can be picked (staged) or left out; its diff is the Project Backups `DiffView` (HEAD against the file on disk, `project_backups::compare::bytes_diff`). Commit, Commit & Push (Ctrl+Enter / Ctrl+Shift+Enter), and "Generate with AI" from the staged diff (lock files left out, 14,000 characters at most). With nothing to commit the tab shows the commits since the last release (unpushed ones marked, `github_releases_recent_commits`) and the next steps: pull/push when needed, Build, Release, the folder, GitHub. Alt+1–4 switch tabs and Alt+R refreshes (F5 and Ctrl+R stay blocked app-wide).
- **Build.** Detected commands (a `build-all`/`release`/`dist`/`build` script with the right package manager, `tauri build`, `cargo build --release`, `dotnet publish`, PyInstaller, `go build`, `flutter build windows`) or a typed one. The project's own top-level script is its full build and comes first: its chain (`&&`, `npm run …`) and the named steps of a script file it starts (`Invoke-Step "App" { … }`, `==> App`; MYLE's `build-setup.ps1`) show as a pipeline that lights up as the log reaches each step (`build/detect.rs`, `pipeline.ts`). `tauri build` is offered on its own only when no script runs it, and says so when `bundle.active` is off (the app alone, no installer); picking or typing something else than the full build shows it, with a button back. A release built here runs the same command. It runs through `cmd /d /s /c` in a Job Object (cancel kills the whole tree), with colours off; the log streams in batches, is saved to `%LOCALAPPDATA%\…\github-releases\logs` (10 per project), and errors and warnings (rustc, tsc, svelte-check, MSBuild, ESLint, esbuild, Vite/Rollup, electron-builder, NSIS, Python, Go) fill the Problems panel; a problem opens in VS Code or Cursor (`vscode://file/…:line:col`). Progress follows stages and the last build's length. Fresh installers, update files and signatures are listed with their SHA-256.
- **Release.** Patch/minor/major or a typed version; every version file (and the lock files) is set to it with a preview, keeping the files' formatting. Notes are written by hand or by AI from the commits since the last tag (and the diff), or polished. Checks first (version, files agree, GitHub remote, branch up to date, uncommitted changes, tag free locally and on GitHub, account, workflow or build command). Then: version files → build → commit → push → annotated tag → push the tag, then either a draft release, the files uploaded (streamed, 3 tries each) and published, or (**Actions mode**, picked by itself when a workflow runs on the tag) the workflow followed job by job, its notes written to `docs/release-notes/<version>.md` when the workflow reads that folder. A failure before the push puts the files and commit back; after it, **Resume** carries on (the remaining uploads, publishing). Monorepo apps tag as `<folder>-v<version>`; their earlier shared `v…` tags count as their last release (same major, not newer). Actions mode follows the run of the workflow the tag starts, found by its file (a tag can start several: MYLE's also starts MYLE Passwords, done in seconds); the notes, when written, go into the release commit as `docs/release-notes/<version>.md`, and without them `release.yml` publishes GitHub's generated notes. Building here while a workflow also runs on the tag is a warning. The checks follow the form, fetching at most every 90 seconds, and only the newest answer is shown.
- **Releases (history).** The project's releases with downloads; edit title, notes and pre-release; delete one or several (their tags too, when asked); **combine** several: AI merges their notes into the newest, and one confirmation that names the release getting the notes and every release and tag to go saves the notes, checks GitHub kept them, and only then deletes the others with their tags (on GitHub and here). Nothing is deleted when saving fails; the kept release's tag, and any tag another release still uses, is never deleted; what couldn't be deleted is listed with **Try again**. **Tags without a release** (a failed or abandoned release leaves one) are listed apart, here and on GitHub, from every page of releases (drafts count); one, several or all are deleted on GitHub and here after a confirmation that names them, and the backend looks again first and refuses a tag a release uses.
- **GitHub account.** The GitHub CLI's sign-in (`gh auth token`, read once and checked with `GET /user`) or a pasted token, sealed with DPAPI in `%LOCALAPPDATA%\…\github-releases\github-token.bin`. MYLE never installs anything by itself: missing Git or GitHub CLI shows a button that runs `winget install` in a visible window, and "Sign in" opens `gh auth login --web` in its own console. Device-flow sign-in needs a registered OAuth App and is not built.
- **AI.** One OpenAI-compatible client for Groq (first; `openai/gpt-oss-120b`), Google Gemini (`gemini-flash-latest`), OpenRouter's free models (`openrouter/free`), DeepSeek (`deepseek-v4-flash`) and Ollama on this PC; the order and models are the user's. Keys are checked with the provider, then sealed with DPAPI (`ai-<provider>.bin`) and never sent back to the page. A failure (429 with its wait, bad key, no credit, model gone) names the next ready provider and the page offers it; nothing is sent elsewhere unasked.
- Tests (`cargo test github_releases`): porcelain v2 status, remotes, tags and log, failure codes, token environment, version files (a MYLE-like layout, mismatches, format-preserving JSON/TOML edits, bumps), monorepos and their tags, workflows and tag patterns, build detection, the error parsers, the runner (a real `cmd`), artifacts, the AI requests and replies (a mock server with a 429), the GBR import, settings.

## Owner-only pages

GitHub Releases and Project Backups are for the app's owner only (`backend/src/account/owner.rs`, the address in `OWNER_EMAIL`). The backend decides, never the page:

- **Who.** After sign-in, and again when the window comes back after 10 minutes, the backend asks Supabase for the user (`GET /auth/v1/user` with the session's access token, so Supabase checks the token) and admits only an account whose email is the owner's (any case), confirmed, signed in with Discord (`app_metadata.provider`), with a Discord identity of that email marked verified. Nothing the webview sends counts: the page can only ask for a recheck (`account_access`).
- **Kept.** The verdict is tied to the user id, kept in memory and sealed with DPAPI in `account-access.bin` (roaming folder), so the pages work offline for 30 days after the last check. Signing out, another account or an expired session clears it.
- **Enforced.** `owner_gate` in `lib.rs` wraps the invoke handler: every command named `github_releases_*` or `project_backups_*` is refused unless the verdict admits the session's user. A test reads `lib.rs` and fails if one of those modules' commands is registered under another name.
- **Live.** The backend sends `account-access` (true/false) on every change. On false the sidebar hides both pages (`ownerOnly` in `registry.ts`), an open one falls back to the first page (deep links and the start page too), and running builds, releases, backups and previews are cancelled.

## The "Password Manager" page

An end-to-end encrypted vault (`backend/src/passwords/`, page in `frontend/app/pages/password-manager/`). The vault itself (encryption, entries, 2FA codes, passkeys, imports, website icons, sync and the account's Supabase sign-in) is the `myle-vault` crate in `backend/vault/`, shared with MYLE Passwords for phones; `backend/src/passwords/` keeps what only Windows does (Windows Hello, the clipboard, the browser extension, filling Windows programs, reading pictures with WIC) and the commands.

- **Keys.** The master password gives a master key through Argon2id (64 MiB, 3 passes). A random vault key encrypts each entry on its own with XChaCha20-Poly1305. The associated data is `entry id | revision`, so a ciphertext cannot be moved to another entry or rolled back unnoticed. The vault key is stored only wrapped: by the master key, and by a 52-character recovery code shown once. Changing the master password re-wraps it; entries are not encrypted again.
- **On this PC.** `%APPDATA%\ThomasThanos\MakeYourLifeEasier\passwords.vault` holds only ciphertext (titles, user names and addresses are encrypted too). Keys and decrypted entries live only in Rust and are wiped (`zeroize`). The page never gets the vault key; a password reaches it only when revealed. Copying goes from Rust to the clipboard, marked to stay out of clipboard history and cloud clipboard, and is cleared after 30 seconds. The vault locks after the chosen idle time (5 minutes by default), whenever Windows locks and after the PC wakes from sleep. Only what the user does counts as use: the page, and filling, saving or a new password from the extension at the user's click; look-ups from the browser and the sync never keep it open.
- **Windows Hello.** Optional, per PC (`backend/src/passwords/hello.rs`); opening the page asks it at once, if the window is in front (never when the vault locks while the user is on the page). Turning it on makes a Windows Hello key (`KeyCredentialManager`, in the TPM where there is one) and has it sign a random challenge; the signature (RSA PKCS#1 v1.5, the same every time for the same data) is hashed into a key that wraps the vault key. Only the wrapped key and the challenge are stored, sealed with DPAPI in `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data\passwords-hello.bin`, never synced. Opening the vault this way has Windows Hello sign again, which it does only after the user's PIN, fingerprint or face. A changed master password keeps it working (the vault key is the same); another vault, or Windows Hello reset, turns it off and the master password opens the vault.
- **Generator.** `backend/vault/src/generator.rs`: random characters (8–128, every chosen kind at least once, no modulo bias) or passphrases from the EFF's large wordlist (`eff_large_wordlist.txt`, CC BY 3.0 US; 7,775 words, about 12.9 bits each; 3–12 words, a separator of up to 3 symbols, capitals and a digit on request). `Options` takes both, and a page that sends only the old fields still gets a password.
- **Guarding it.** After three wrong master passwords (or recovery codes) each try waits longer, doubling up to a minute. Exporting a backup asks for the master password again even while the vault is open, and an import reads only the file just picked in its dialog, never a path the page names. An entry that does not open with the vault key (damaged, or not made with it) is left out and counted, instead of keeping the whole vault shut; the page says how many. A shown password, and the password history, hide again after 30 seconds.
- **Sync.** Two Supabase tables with row security per user: `password_vault` (header) and `password_items` (one sealed row per entry, deletions as tombstones). Create them with `docs/supabase/password-manager.sql`. Every change is sent with compare-and-swap on the revision the PC last saw. An entry changed on two PCs is merged once unlocked: the newer change wins and the other password goes into the entry's history. Deletions are sealed with the vault key too (`myle-tombstone|id|revision`), and nothing from the account is taken in while the vault is locked: every row must open with the key first, so someone with the account alone can neither add nor delete entries (a row that does not open is refused, and this PC's version goes back over it one revision up). A second PC takes the account's vault and opens it with the same master password.
- **Browser filling.** `extension/` is the Manifest V3 extension for Chrome, Edge and Brave; `extension/firefox/` has the Firefox manifest; the shared files are copied next to it by `node scripts/prepare-extension.mjs` (they are not committed, and the bundle and `cargo` builds need them there). Both background scripts load `psl.js`, the Public Suffix List, so the extension counts `a.github.io` and `b.github.io` as two sites as the app does; after a `psl` crate update, run `node scripts/update-psl.mjs`. Chromium uses `background.service_worker`, while Firefox uses `background.scripts`. The extension talks to a native messaging host (`com.thomasthanos.myle`): this same program, started by the browser with the extension's id (`run_native_host` in `browser.rs`). The host checks the id and that a browser started it, then passes each request over a per-user named pipe to the running app, which answers only a host that is the same program. The app answers only while "Browser filling" is on and the vault is unlocked, only for https pages (or `localhost`), and a password only for an entry saved for that site (same host, or same registrable domain by the Public Suffix List). Nothing is filled without a click. Browser filling is on by default (`Prefs.browser_filling`; a vault made before that had it turned on once, `filling_default_applied`, and the user's choice stands after that). While it is on, the app writes the host manifests to `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data\native-messaging` and registers them under `HKCU\Software\{Google\Chrome, Microsoft\Edge, BraveSoftware\Brave-Browser, Mozilla}\NativeMessagingHosts`, and `ensure_registered` checks that on every start (from the copy that owns the pipe), every unlock and every look at Browser filling: a key pointing at a deleted manifest, a manifest naming another copy of the program, or a moved program is written again. Turned off, the keys and manifests go; the uninstaller removes the keys. Browser filling shows which browser last asked and when (`contact.json` next to the manifests, written by the app; the host passes the browser's name with each request), what is missing otherwise (with Repair), and the last start the host turned away (`host.log`: another browser or extension). The toolbar popup's Copy buttons (user name, password, 2FA code) send `copy` with the frame's address: the app checks the login is saved for that site, as for a fill, and puts the text on the clipboard itself (marked secret, cleared after 30 seconds), so it never passes through the browser; an older app answers `badRequest`, which the popup shows as "update MYLE". The extension tells apart a browser that cannot find the host ("not found"), an unknown copy of the extension ("forbidden"), the host stopping, and filling being off. Both browser folders ship in the install folder for loading as temporary/unpacked extensions, and `node scripts/prepare-extension.mjs` also packs `extension/myle-passwords-{chrome,firefox}.zip` for the stores (git-ignored). Loaded unpacked, its Chrome id is `gaelkhdpkgnffkfmaaklknijinjmmopo`, fixed by the `key` in its manifest; the store zip leaves the `key` out, so the Chrome Web Store gives it `mifjffbnaeeljjfboiglbcoaokgdilca`. The host allows both (`CHROME_EXTENSION_IDS`). The Firefox id is `myle-passwords@thomast.uk`. Its store page is `myle.extensionUrl` in `package.json`: the setup offers it on a first install ("Browser extension", opened when setup finishes) and Browser filling shows "Get the extension".
- **The extension against the page.** The page's scripts share the document, so the menu (`extension/content.js`) assumes they are hostile. It counts only real input (`isTrusted`); it lives in a closed shadow root under a random tag name, in the browser's top layer (a manual popover, shown again above anything the page put there), with its host's look pinned by `:host { … !important }` rules; a change to its element closes it. A click on it counts only after it has been on screen, still and untouched, for half a second, never as the second click of a double click, only where it is the element under the pointer, and in Chromium only while IntersectionObserver v2 sees nothing drawn over it (otherwise it says so). It never fills hidden fields or a form that sends over plain http (checked before the password is asked for). A sign-in form embedded from another site is filled only from the toolbar popup, where the user sees whose it is. A login is offered for saving only if the user typed it (or picked the suggested password): the next page gets a random nonce and the user name, never the password, and "Save" works only with that nonce. On a password step, the account the page already shows (Google's, say) comes first. The app answers the host only so many requests a minute (20 fills, checks or saves, 120 look-ups), and a page may change only the login saved for its own host (the old password goes into the history).
- **2FA codes.** An entry can keep a TOTP key (`backend/vault/src/totp.rs`, RFC 6238: SHA-1/256/512, 6–8 digits, 10 s–5 min), sealed in the entry as an `otpauth://totp/…` link, so it syncs and backs up with it. The editor takes the key or its link, or reads a QR code (`backend/vault/src/qr.rs`, `rqrr`) from a snip on the clipboard (`CF_DIB`) or a picture file (Windows' own decoders, WIC); the page only ever gets the codes (`passwords_totp`, which never counts as using the vault) and whose key it is. The extension fills a code into a site's 2FA field (`autocomplete="one-time-code"`, a name like `otp`/`2fa`/`verification-code`, or a row of one-digit boxes) for a login saved for that site, by the same rules as a password. Entries keep fields a newer MYLE adds (`Entry.extra`), so an edit on an older copy never drops them; copies up to 9.1.1 still drop the 2FA key and passkeys of an entry they edit.
- **Passkeys.** `extension/passkeys.js` runs in the page's own world at `document_start` and takes `navigator.credentials.create/get` calls with `publicKey` to `content.js`, which asks the user in MYLE's prompt (the same closed shadow root as the menu); "Another device", or no passkey of MYLE's for the site, calls the browser's own as before. The app (`passkeys.rs`) is the authenticator: ES256 keys (P-256, PKCS#8 in the entry), attestation "none", MYLE's AAGUID, backup flags set and the counter always 0 (the passkey lives on every PC of the vault). It makes `clientDataJSON` itself from the address the browser reports, takes only the page's own host or a domain above it that is not a public suffix (`rp_id_for`), and signs only after the click; when the site asks for user verification, Windows Hello confirms it (`hello::verify`, owned by the window in front), or, on a PC without it and a site that insists, the master password typed in MYLE (`ask_master`, `VerifyPasskey.svelte`). A sign-in form that offers passkeys as you type (`mediation: "conditional"`) gets MYLE's in the menu on its user name field, while the browser's own request runs alongside. A new passkey goes into the login of that account on the site, or a new one, and replaces one for the same account. In a frame of exactly the page's origin (an empty helper frame, as Google's sign-in uses; `match_about_blank`, `match_origin_as_fallback`) it hands `publicKey` calls to the top page's wrapper; frames of other origins keep the browser's own. Credential ids are compared as bytes (`same_credential`: base64url with or without padding, or plain base64) and relying parties without regard to case (`same_rp`). Locked, or closed on a site known to have a passkey, MYLE's prompt offers to unlock or open MYLE (`open`) and waits for the vault (`waitUnlocked`, ended by `stopWaiting`), then carries on by itself. Which sites have passkeys comes from `passkeySites` (rp ids only, while unlocked, at most every five minutes) and from list answers; the extension keeps them in `storage.local` as salted SHA-256 fingerprints, never the names. The page-world part wraps both `CredentialsContainer.prototype` and the `navigator.credentials` instance, accepts any `this` (a bound or borrowed method), adopts same-origin frames reached through `contentWindow`/`contentDocument`, reports `isUserVerifyingPlatformAuthenticatorAvailable()` as true, and makes its wrappers read as native code through `Function.prototype.toString`. `content.js` starts at `document_idle`, so it posts `hello` once it listens; questions asked before that (Google asks on load) are posted again then, and `content.js` answers each id once. When the site's `allowCredentials` leaves out MYLE's passkeys for the rp, the list answer carries `unlisted`/`unlistedIds` and the prompt says so. **Debug log:** the popup's switch sets `debug` in `storage.local`; `content.js` then writes each step (`trace` messages from the page part, buffered until `hello`, and its own decisions) to the page console as `[MYLE passkeys]`: rp ids, credential ids, counts and results, never challenges, keys or passwords.
- **Offering to save.** A login the user typed is offered after the form is sent: on the next page, or on the same one when a web app signs in without leaving it (the form gone, or the address changed). While the vault is locked it is offered anyway: Save brings MYLE forward and saves once the vault opens, unless the vault turns out to have it. A 2FA setup page's key, written out or as its `otpauth://` link (only on a page that talks about authenticators, 2FA or a setup key), is offered to keep with the site's login (which one, when there are several), or as a new login; the toolbar popup's "2FA from a QR code" reads a QR code shown in the tab (a screenshot, `captureVisibleTab`, read by the app) the same way. New passkeys are offered in MYLE's prompt.
- **Windows programs.** A login links to a program by its full path (`AppLink.exe`; Browse… in the editor); a program that updates into a new version folder (`app-1.0.9172`, `2.4.1`, `v2.5`) stays linked (`same_program` in `windows_fill.rs`). The hotkey (Ctrl+Shift+L, or Ctrl+Alt+Shift+L when another program holds that) remembers the program in front and its focused field (Store apps are found inside `ApplicationFrameHost.exe`); MYLE then shows its linked logins: Fill both (user name, Tab, password), or one of them, typed as Unicode keystrokes after checking the window and, where Windows can tell, the field again. A program that runs as administrator gets Copy buttons, since Windows drops keys typed into it. A program with no login gets "New login for it" and "Link a login…"; a login linked by file name only (older links) is linked in one click.
- **Website icons.** The list shows each website's icon (`backend/vault/src/icons.rs`, `Favicon.svelte`; on by default, off from ⋯, which deletes them). The app asks each website itself, so no icon service learns the vault's sites: only public names over https, never an IP address, `localhost` or a local, `.arpa` or `.onion` name, following redirects itself and turning `http://` ones into `https://`. It reads the home page up to `</head>` for `<link rel="icon">` and Apple touch icons (or a `<meta http-equiv="refresh">` page), then `/favicon.ico`, and keeps an answer only if its first bytes are an image's (PNG, ICO, GIF, JPEG, WebP or SVG, 200 KB at most, drawn by `<img>` where an SVG's scripts never run). The icons are kept in `passwords-icons.bin` next to the vault, sealed with the vault key, read when the vault opens and fetched again after a month (a site without one after a week). The extension gets the icons of a site's logins from that cache with its answer; the browser never fetches them.
- **Tray.** Only with "Keep running in the tray" switched on (Settings, `KeepInTray` in `HKCU\Software\ThomasThanos\MakeYourLifeEasier`, off when unset: the app never stays behind unless the user chose it), closing the window hides the app next to the clock (`backend/src/tray.rs`), so Ctrl+Shift+L and browser filling keep working; the icon's menu opens the app or the vault, locks the vault, or quits. Started by Windows with "Start minimized", the app starts there (minimized in the taskbar with the tray off). With "Start with Windows" switched off (`StartupShortcut` 0), a Startup shortcut of ours found at start is removed, and a start from one ends at once (`startup::respect_choice`), so an older setup that put it back cannot bring the app up at sign-in. Because a closed window only hides, the setup first sets the event `Local\MakeYourLifeEasier-Quit`, on which the app exits cleanly, before it asks the windows to close (`processes.rs`).

## MYLE Passwords (Android and iPhone)

The Password Manager on phones: see, copy and edit the logins, with their 2FA codes, in sync with MYLE on the PC. It does not fill other apps. It is distributed outside the stores: an APK for Android, and an IPA that [SideStore](https://sidestore.io) installs with the user's own (free) Apple ID.

- **Code.** `backend/vault` (`myle-vault`) is the vault itself, used by both apps: encryption, entries, 2FA codes, passkeys, imports, website icons, the sync and the account's Supabase sign-in (`account.rs`, `Cloud`). It knows nothing of Tauri or Windows; TLS is native-tls on Windows and rustls on phones (Mozilla's root certificates on Android, `http.rs`). `backend/mobile` answers the page with the same command names as the Windows app (`passwords_*`, `account_*`), so `frontend/app/pages/password-manager/` serves both; `frontend/lib/platform.ts` (`MOBILE`, from `vite build --mode mobile`) leaves out what only Windows does (browser filling, Windows programs, import and export, passkey prompts) and words things for a phone. `frontend/mobile/` is the phone's shell: the bar, the welcome screen, the account sheet, the camera's QR frame, the back button.
- **Account.** Sign-in shows Discord's or Google's page inside the app (`backend/mobile/plugin`, `tauri-plugin-myle-mobile`): iOS's sign-in sheet (`ASWebAuthenticationSession`, sharing Safari's sign-ins), and on Android a Custom Tab over the app (Google refuses sign-in inside an app's own webview). Both come back to `uk.thomast.myle.passwords://auth-callback` (a Redirect URL of the Supabase project, Authentication → URL Configuration): iOS hands the address straight back; on Android it arrives as a link, and a return to the app without it means the tab was closed (the sign-in is cancelled). The PKCE verifier waits in `sign-in.json` for 10 minutes, so a return that starts the app again still finishes. The session is kept in `account.json` in the app's own folder, which no other app can read.
- **Unlocking.** Face ID / Touch ID / fingerprint (`unlock.rs`, behind the `passwords_hello_*` commands): a random key in the Keychain or Keystore (`tauri-plugin-biometry`, pinned), which the phone hands back only after the user's face or finger, wraps the vault key; only the wrapped copy is kept, in `passwords-biometry.json`. The vault locks after the chosen idle time, and when the user comes back after a minute away (`passwords_app_hidden`); the page is blurred while the app is out of sight. Copied passwords leave the clipboard after 30 seconds.
- **Builds.** `.github/workflows/mobile.yml` makes everything on GitHub's runners: `tauri android init/build` (aarch64 + armv7, then `zipalign` and `apksigner` with the key in the `ANDROID_KEYSTORE_*` secrets) and `tauri ios init/build --no-sign` on macOS with Xcode 26. A push to a `mobile/…` branch, or a run by hand, keeps the APK and IPA as artifacts. A release tag `vX.Y.Z` builds them too when `backend/mobile/tauri.conf.json` and `Cargo.toml` have that version, and they go into the same GitHub release as `MYLE.exe` (release.yml and mobile.yml each create it or add to it, whichever finishes first; the Windows app's run marks it "latest"); a `mobile-v*` tag is a phone release of its own that never becomes "latest". Either way they go to R2 with `sidestore.json` and `mobile-latest.json` (`scripts/mobile-feeds.mjs`). The Android app checks `mobile-latest.json` (`update.rs`), downloads the new APK into its own cache folder (each version has its own address, `MYLE-Passwords-<version>.apk`, so no cache can hand out the previous one), checks its SHA-256 and opens Android's installer (the plugin; the first time, Android's "install unknown apps" setting for the app). Android installs it over the old one only because it is signed with the same key, so that key (backed up outside the repo) must never be lost. The plugin is an ordinary Tauri plugin crate: its Android library (`REQUEST_INSTALL_PACKAGES`, `androidx.browser`) and Swift package are built into the projects `tauri android/ios init` makes; the workflow only raises Kotlin to 2.1 (for the biometrics plugin) and puts the iOS icon's light, dark and tinted looks (`backend/mobile/ios-icon`, `backend/icons/app-icon-*.png`) into the asset catalog.
- **Installing.** Android: open `https://downloads.thomast.uk/MYLE-Passwords.apk` on the phone and allow installing from the browser. iPhone: in SideStore, add the source `https://downloads.thomast.uk/sidestore.json` and install MYLE Passwords; a free Apple ID re-signs it every 7 days (SideStore does it in the background) and allows 3 such apps, SideStore included.

## The "Windows Optimization" page

Five tabs: **Quick setup**, **Settings**, **Apps**, **Start Menu** and **Tools** (Auto-Logon, restart to BIOS/UEFI). The debloater is MYLE's own (`backend/src/debloat/`, page in `frontend/app/pages/windows-optimization/debloat/`); it replaces the WinUtil and Sparkle launchers, whose downloaded cache (`…\data\windows-optimization`) is deleted on the page's first load.

- **Choose, then apply.** Nothing changes while the user chooses. Quick setup's profiles, the Settings switches (one line per tweak, `SettingRow.svelte`, details on hover or under the line) and the Apps ticks only record choices (`desired` and `removing` in `state.svelte.ts`, kept in `myle.debloat.pending`; an older version's ticked apps are carried over). The bar under those tabs shows how many there are; "Review & apply" lists them (`ReviewPanel.svelte`), where any can be left out, then makes one restore point and does them all. The logic is plain TypeScript in `selection.ts`, tested by `scripts/tests/debloat-selection.test.mjs`.
- **Profiles.** Every tweak and app has a `level` in `catalog.rs`: **Light** (privacy and promoted apps; nothing looks different), **Recommended** (adds what nearly everyone is better off with) and **Maximum** (the rest, with small trade-offs: every promoted app, Phone Link, Teams and Spotify included). Each profile takes in the ones before it and only ever turns things on. `None` is outside every profile: personal taste (dark mode, the taskbar on the left), Edge, and the apps most people use (Calculator, Photos, Notepad, Paint, the Xbox apps). A tweak's `note` says what to know before turning it on; an app's `about` says what it is and `keep` why someone might keep it (the "Keep?" tag).

- **The tables.** `catalog.rs` holds every tweak (registry values, service start types, scheduled tasks, Store apps, the 24-hour clock, Edge, parts of Windows) and every app it may remove, fixed at compile time; the page sends only ids. Values follow WinUtil, Win11Debloat and Sparkle without their known mistakes: WinUtil's `wermgr` is not a service; `SearchboxTaskbarMode` has four values, so the user's own is kept; Widgets are hidden with the `Dsh\AllowNewsAndInterests` policy because the UserChoice Protection Driver blocks `TaskbarDa`, and Windows 10's `ShellFeedsTaskbarViewMode` is best effort too (24H2 answers "access denied"); `SharedAccess` is left alone (Mobile Hotspot needs it). A service changes only from the start types Windows may have (`from`), so one the user set by hand stays. `NEVER` and `removable()` keep Windows' and MYLE's own packages out of reach whatever the table says: the Store, Terminal, winget, Xbox sign-in, frameworks, codecs, and WebView2, which MYLE runs on. Names match exactly or by publisher suffix, never with wildcards.
- **Where it stands.** `detect.rs` reads each tweak's state from the registry, services (`QueryServiceConfig`), scheduled tasks (Task Scheduler COM), the locale's time format and the installed packages (the per-user AppModel repository in the registry, not PowerShell), without administrator rights, in well under a second.
- **Undo.** Before each change `undo.rs` keeps what was there: the value or its absence and the first key the write created, the start type, the task's state, the time format. Only the first original of an operation is kept, so applying twice cannot overwrite it. Undo puts it back exactly and removes the keys it created while empty. The record is `debloat-undo.json` in the local data folder; Edge's undo installs it again with winget, and removed apps are offered again from the Microsoft Store. A tweak that was on before MYLE ran is turned off with the Windows defaults (`detect::default_before`): no policy, the value Windows ships with, and for a service the start type in the catalog's `windows`, never one Windows keeps off itself (Remote Registry stays disabled).
- **Who does what.** The user's own settings (HKCU, the time format through `SetLocaleInfoW`) are changed by the app itself, as the user, so an elevation with another administrator account cannot land them in that account's profile; Explorer is restarted by the app for the same reason. Machine-wide changes, app removal for every user and new ones (`Remove-AppxPackage -AllUsers` and the provisioned package), Edge and restore points go through an administrator helper: this same program started with `--debloat-elevated-helper` over `elevated_pipe.rs` (shared with the System Cleaner; one UAC prompt per session, the helper serves only the app's pipe and the app answers only this same program). The helper checks every undo record against the catalog: only the tweak's own operations, strings from `allowed_machine_strings`, and keys on the way to the value.
- **Windows features.** Parts of Windows few people use (Steps Recorder, Math Input Panel, WordPad, XPS Viewer, Windows Media Player Legacy, PowerShell ISE, Internet Explorer mode, Fax and Scan) are tweaks of the `features` category, shown on the Apps tab. Whether one is installed is read without administrator rights from the servicing store: `HKLM\…\Component Based Servicing\Packages`, the neutral packages of its name, `CurrentState` 0x70 (installed). A Windows without the package shows nothing. The helper removes it with `Remove-WindowsCapability` (matching the catalog's capability name, never one it is sent) and Undo adds it back with `Add-WindowsCapability`, which downloads it from Windows Update. Optional features (`Enable-WindowsOptionalFeature`) are left out: their state is not in the registry, and the ones worth turning off (SMB1, PowerShell 2.0, XPS Services) already are on Windows 11 24H2.
- **Edge.** `setup.exe --uninstall --force-uninstall` from Edge's own uninstall entry, accepted only from Edge's folder, with `EdgeUpdateDev\AllowUninstall` and the old-Edge stub in place for the run (both removed after). EdgeUpdate stays: it also updates WebView2.
- **Restore point.** Made before every "Review & apply" that turns something on or removes an app (`Checkpoint-Computer`, with Windows' once-a-day limit lifted for the call and put back). System Protection is read from `SPP\Clients`; if it is off the page offers to turn it on for the Windows drive, or to go on without.
- **Start Menu.** `start_menu.rs`: layout, alignment, the All Apps view, the Recommended section's parts and the folders next to Power are the user's own (HKCU, set by the app). Hiding Recommended and the pinned-apps presets are machine policies set by the helper; pins come only from `PIN_CATALOG` (the helper checks the JSON again), `start2.bin` is backed up before the first preset, and the `ConfigureStartPins` policy is removed again after the Start menu has read it, so pins stay the user's to change.
- **Tests on a real Windows.** `sandbox_tests.rs` applies every tweak for real, applies it again (nothing new is kept), undoes it and compares every value, key, service, task and the time format with before; it also turns tweaks off from the Windows defaults, sets and clears the Start Menu policies, and makes a restore point. The tests run only in Windows Sandbox (`MYLE_DEBLOAT_SANDBOX=1`, account `WDAGUtilityAccount`) or a VM (`MYLE_DEBLOAT_VM=1`): copy the test executable (`cargo test --lib --no-run`) and `vcruntime140*.dll` into the sandbox and run `myle_lib-<hash>.exe sandbox_ --ignored --test-threads=1 --nocapture`. Edge's removal needs a VM: Windows Sandbox shares Edge's files with the host, read-only, so its uninstaller (allowed there, it says so in `SystemTemp\msedge_installer.log`) cannot delete them.

## Window size

At startup the main window is sized for the screen the cursor is on, and centered. Sizes are in physical pixels:

| Screen | Window |
|---|---|
| 4K (2160p) | 2560×1440 |
| 2K (1440p) | 1920×1080 |
| 1080p | 1280×720 |
| smaller | 90% of the usable area |

The window never exceeds 95% of the area outside the taskbar.

## Updater (Cloudflare R2 + GitHub Releases)

The old Electron app (v4.x) lives on the `old` branch and no longer receives updates; the new one starts at **7.0.0**. The old `latest.yml` on R2 stays untouched, so old installs simply see "up to date".

**Release:**

1. Bump the version in **both** `package.json` **and** `backend/Cargo.toml` (e.g. `7.0.2`) and commit.
2. `git tag v7.0.2 && git push origin main v7.0.2`
3. `.github/workflows/release.yml`:
   - checks the tag and versions match, runs `svelte-check`, and builds (also downloading Ludusavi)
   - **signs** the exe, installer and uninstaller with the certificate from the `WIN_CSC_LINK` / `WIN_CSC_KEY_PASSWORD` secrets (`scripts/sign.ps1`)
   - uploads `MYLE.exe` to **Cloudflare R2** (`downloads.thomast.uk`, `R2_*` secrets), preserves the old `MakeYourLifeEasier-installer.exe` URL for existing links, and finally publishes `latest.json`, and checks R2 serves the exact same bytes
   - publishes the GitHub release too

To test without a release: Actions → Release → **Run workflow**. It builds and signs the same way, keeps the installer as an artifact, and doesn't upload or publish anything.

**On every launch, the splash:**

- reads `https://downloads.thomast.uk/latest.json` (`UPDATE_FEED` in `backend/src/updater.rs`); if that doesn't answer, it asks `api.github.com/repos/thomasthanos/MYLE/releases/latest`
- if a newer version is found, downloads the installer (only from `downloads.thomast.uk` or `github.com`) and checks its SHA-256 (from the feed or GitHub's digest; with no hash it won't install)
- runs the setup silently with `/S /UPDATE /LIVE` **while the app is still open**: files are swapped in with a rename (Windows allows this even for a running exe), the old ones stay as `*.myle-old`, and the new version deletes them once it starts
- opens the new version with `--just-updated` (it doesn't ask the network again, and shows "Updated to v…"), and closes as soon as its window appears; before that it drops the single-instance lock, otherwise the new copy would just hand its arguments to the old one and quit
- if the live update fails (or the app isn't running from the install folder), it falls back to the setup's progress window (`/P /UPDATE /R`), which waits for the app to close and opens the new version half a second before closing itself
- the installer reopens the new version

With no network, the app opens normally after ~2 seconds (the bar counts down).

**What's new** (`backend/src/whats_new.rs`, `frontend/lib/components/WhatsNewHost.svelte`): on the first start of a new version the app shows the notes of every version since the one it last showed (kept in `whats-new.json` in the settings folder), newest first. They are the GitHub release bodies, asked for by Rust without a sign-in (the web view's CSP only reaches the app) and cached in `whats-new-cache.json`; offline or rate-limited, the cache, then this version's `docs/release-notes/<version>.md` (bundled by `backend/build.rs`), then a line with a link stand in. Markdown is read by `frontend/lib/markdown.ts` into plain data rendered as text: raw HTML is reduced to its text, pictures are left out, and only http(s) links are kept, opening in the browser. Versions up to 9.8 never wrote `whats-new.json`; when it is missing but MYLE ran on the PC before (its settings folder has files, or its web view profile exists), this version's notes are shown, and on a first install nothing is. Settings → About → **What's new** shows them again. For everyone, not only the owner.

> The SHA-256 check catches files that got corrupted or changed along the way. It doesn't protect against the GitHub/Cloudflare account itself being compromised.

### Cloudflare Pages site (separate from the updater)

`make-your-life-easier.pages.dev` is a static download/marketing site and doesn't serve the Tauri webview. Its files live in `site/`, and the root `wrangler.toml` declares `pages_build_output_dir = "./site"`. The Pages build command stays empty; it must not point at the Vite build or run `npx vite build`.

- Main download: `https://downloads.thomast.uk/MakeYourLifeEasier-installer.exe`
- GitHub fallback: the repository's latest release
- `/installer.html` redirects to `/` for compatibility with old links
- `npm run check:site` checks required files, internal links, security headers, and stale Electron/Portable references
- The R2 objects, `latest.json`, and the release workflow are independent of the Pages deployment

## Setup and uninstall (per user, no admin)

The setup is our own: the `backend/installer` crate (Rust) with a Svelte window (`frontend/installer/`, `frontend/installer.html`) in the same dark style as the app. It builds two programs:

- **`setup.exe`**: carries the app as one solid XZ payload (`installer/src/payload.rs`). Shows the install folder, toggles for Desktop / Start menu / launch with Windows / open when finished, per-file progress, and a completion screen. On the same version it plainly says "Reinstall" and keeps settings and data. It repairs the app's own recognized shortcuts and checks their targets; the Startup shortcut launches with `--autostart`. The choices are remembered in `HKCU\Software\ThomasThanos\MakeYourLifeEasier` (`DesktopShortcut`, `StartMenuShortcut`, `StartupShortcut`) and offered again next time, minus any the user has deleted since. A shortcut Windows refuses (retried a few times, since the shell and virus scanners open new `.lnk` files at once) does not fail the install: the app is installed and the completion screen lists what is missing. A dead link, or one to an older copy of the app, under our shortcut name is taken over rather than getting a second entry beside it. If the app is running, it asks you to close it before changing files. With "Open when finished" checked, it opens the app and closes the setup once the install succeeds.
- **`uninstall.exe`**: sits next to the app and is what Windows runs from "Installed apps". It asks whether to also remove settings/data (`%APPDATA%\ThomasThanos\MakeYourLifeEasier` / `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data`, cache). While at it, it also cleans up the older `com.thomasthanos.makeyourlifeeasier` folders. Game Saves backups are never touched. Because a program can't delete its own file while running, `uninstall.exe` (like NSIS's) copies itself to a new folder in `%TEMP%` and runs from there (`installer/src/relocate.rs`): the copy does the work, and once the original has exited it deletes it and the folder too. The `/S` exit code still reaches whoever ran it. Old copies in `%TEMP%` are swept up on the next uninstall.

Install is all-or-nothing: each file is written next to the old one, and if something fails partway through, the previous version is restored. `install.json` in the folder records which files the setup put there, so update and uninstall only remove those.

Command line (the same as NSIS's, which the updater already speaks):

| Flag | What it does |
|---|---|
| `/S` | no window |
| `/P` | progress only, starts at once and closes itself (the updater runs `/P /UPDATE /R`) |
| `/UPDATE` | shortcuts stay as the user left them |
| `/R` | opens the app afterward |
| `/NS` | no shortcuts |
| `/LIVE` | (with `/S /UPDATE`) update while the app is running; the app itself reopens it |
| `/D=<folder>` | a different folder (last, unquoted); testing only, the window doesn't change the folder |
| `/PURGE` | (uninstall) also removes settings/data |

Preview the window in a browser: `npx vite` and `http://localhost:1420/installer.html?demo=install` (or `=update`, `=reinstall`, `=uninstall`, `=running`, `=error`, `=launch-fail`, `=passive`).

| What | Where |
|---|---|
| Program | `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\MYLE.exe` (existing installs keep this folder) |
| Settings, account, Game Saves, Project Backups, GitHub Releases | `%APPDATA%\ThomasThanos\MakeYourLifeEasier` |
| Cache and WebView2 (localStorage) | `%LOCALAPPDATA%\ThomasThanos\MakeYourLifeEasier\data` |
| Desktop | Windows Desktop known folder (may redirect to OneDrive) · `MYLE.lnk` |
| Start Menu | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\MYLE.lnk` |
| Launch with Windows (opt-in) | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\MYLE.lnk` (with `--autostart`) |
| Registry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MakeYourLifeEasier` |
| Game Saves task | `MakeYourLifeEasier Game Saves Backup` (legacy task name preserved for existing schedules) |

Existing installs may also keep a `MakeYourLifeEasier.exe` compatibility copy. It hands normal launches to `MYLE.exe` and keeps an old updater or scheduled backup working through the transition. New installs contain only `MYLE.exe`.

Versions up to 7.0.x kept their data in `com.thomasthanos.makeyourlifeeasier` folders. The app moves them once, whole, with a single rename (`backend/src/storage.rs`). If a folder is in use (during a live update, the old version is still running), that time it uses the old one instead, and retries the move on the next launch. A half-finished move never happens, since that would corrupt the WebView2 profile.

- Starting with Windows is off unless chosen: in the setup ("Start with Windows", off for a new install and for `/S`) or in Settings, which adds or removes the same Startup shortcut and keeps the setup's remembered state in step (`backend/src/startup.rs`). Game Saves' scheduled backups do not need it: they run from their own scheduled task (`--game-saves-auto-backup`, no window).
- When the app opens from Startup (`--autostart`), the main window starts minimized to the taskbar unless "Start minimized" is off (the setup's Minimized / On screen switch, or Settings). The choice is the `StartMinimized` value in `HKCU\Software\ThomasThanos\MakeYourLifeEasier`, read in `finish_startup` (`backend/src/lib.rs`); unset means minimized.
- Uninstall also removes the Game Saves Windows Scheduled Task, and only the shortcuts that point to our own app.
- Whichever shortcuts were chosen in the setup. For each one the setup remembers whether it is wanted and whether it was made (`0` off, `1` wanted but not made yet, `2` made). `/UPDATE` (and the in-app update) tries again a wanted one that never got made; one that was made and is gone since was deleted by the user, so it is not brought back and is saved as off (the setup then offers it unticked). Updates refresh the others of ours and remove none. Uninstall turns `2` back into `1`, so a reinstall offers the same shortcuts. The first update over an install from before any of this was remembered adds the Start menu entry and turns the old app's `Run` value into a Startup shortcut.
- After an install or uninstall, `scripts/verify-install.ps1` (or `-Removed`) checks all of the above.
