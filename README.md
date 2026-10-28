<p align="center">
  <img src="docs/assets/hero.svg" alt="Make Your Life Easier: apps, game saves, cleanup and Windows tools in one place" width="100%" />
</p>

<p align="center">
  <a href="https://github.com/thomasthanos/MYLE/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/thomasthanos/MYLE?display_name=tag&style=for-the-badge&color=8b83ff&labelColor=151c31" /></a>
  <a href="https://github.com/thomasthanos/MYLE/actions/workflows/ci.yml"><img alt="Build status" src="https://img.shields.io/github/actions/workflow/status/thomasthanos/MYLE/ci.yml?branch=main&style=for-the-badge&label=build&labelColor=151c31" /></a>
  <img alt="Windows 10 and 11" src="https://img.shields.io/badge/windows-10%20%2F%2011-45c8e6?style=for-the-badge&labelColor=151c31" />
</p>

<p align="center">
  <a href="https://downloads.thomast.uk/MYLE.exe"><img src="docs/assets/download.svg" alt="Download Make Your Life Easier for Windows" width="380" /></a>
</p>

<p align="center">
  <a href="https://github.com/thomasthanos/MYLE/releases/latest"><kbd>Releases</kbd></a>
  <a href="docs/DEVELOPMENT.md"><kbd>Development guide</kbd></a>
  <a href="LICENSE"><kbd>License</kbd></a>
</p>

<img src="docs/assets/divider.svg" alt="" width="100%" />

<h2 align="center">What's inside</h2>

<p align="center">
  <img src="docs/assets/features.svg" alt="Install Apps, Game Saves, Spotify Hub, Creative Suite, Windows Optimization, System Cleaner, System Maintenance, Sync &amp; Updates" width="100%" />
</p>

| Page | What it does |
|---|---|
| <a href="frontend/app/pages/install-apps"><kbd>Install Apps</kbd></a> | An app store on top of winget: install, update and keep track of your programs, with your own list and app packs |
| <a href="frontend/app/pages/creative-hub"><kbd>Creative Suite</kbd></a> | Your own creative packages (Adobe, Clip Studio Paint, Office…): download, unpack and run the setup in one click |
| <a href="frontend/app/pages/spotify-hub"><kbd>Spotify Hub</kbd></a> | Customize Spotify with Spicetify and a theme, or put it back to stock; Purge removes the leftovers |
| <a href="frontend/app/pages/game-saves"><kbd>Game Saves</kbd></a> | Finds your game saves and backs them up (three snapshots each), restores them with a safety copy, and can back up daily or weekly by itself |
| <a href="frontend/app/pages/project-backups"><kbd>Project Backups</kbd></a> | Zips your project folders into Google Drive or Dropbox, checked twice before they are kept, with a diff between any two backups |
| <a href="frontend/app/pages/github-releases"><kbd>GitHub Releases</kbd></a> | Commit, build and release your own projects to GitHub — version files, notes, tag, upload, or the tag's Actions workflow |
| <a href="frontend/app/pages/windows-optimization"><kbd>Windows Optimization</kbd></a> | Bloat removal and tuning in profiles (Light, Recommended, Maximum), with a full undo and a restore point |
| <a href="frontend/app/pages/system-cleaner"><kbd>System Cleaner</kbd></a> | Finds and removes the files Windows leaves behind, and reports honestly what is in use |
| <a href="frontend/app/pages/system-maintenance"><kbd>System Maintenance</kbd></a> | Windows diagnostics and repairs, one at a time, with their output on screen |
| <a href="frontend/app/pages/disk-analyzer"><kbd>Disk Analyzer</kbd></a> | Shows what is filling a drive — as a treemap, a folder tree, the largest files and file types — and moves what you pick to the Recycle Bin. Also makes cloud files online-only |
| <a href="frontend/app/pages/auto-clicker"><kbd>Auto Clicker</kbd></a> | Repeats clicks at an interval, or runs a sequence of recorded points, with F6 to start and stop |
| <a href="frontend/app/pages/password-manager"><kbd>Password Manager</kbd></a> | An end-to-end encrypted vault for your logins and 2FA codes, with Windows Hello, passkeys and browser filling |
| <a href="frontend/app/pages/settings"><kbd>Settings</kbd></a> | Your account and sync, the appearance, start-up and the tray, and updates |

<p align="center">
  <sub><a href="https://downloads.thomast.uk/MYLE-Passwords.apk"><kbd>MYLE Passwords · Android</kbd></a>
  <a href="https://downloads.thomast.uk/sidestore.json"><kbd>iPhone · SideStore</kbd></a>
  <a href="https://chromewebstore.google.com/detail/mifjffbnaeeljjfboiglbcoaokgdilca"><kbd>Browser filling</kbd></a></sub>
</p>

<h2 align="center">Install</h2>

<p align="center">
  <img src="docs/assets/install-steps.svg" alt="Download, install for your account without admin rights, and it updates itself" width="100%" />
</p>

<p align="center"><sub>Install for your own account — no administrator rights. If the WebView2 Runtime is missing, the setup installs it from Microsoft. Your settings and data are kept across every update.</sub></p>

<h2 align="center">How it works</h2>

<p align="center">
  <img src="docs/assets/how-it-works.svg" alt="Svelte frontend, Rust and Tauri backend, Windows tools, and updates verified with SHA-256" width="100%" />
</p>

- **One window, thirteen pages.** Svelte 5 in front, Rust and Tauri behind, on the WebView2 runtime Windows already has.
- **Your data stays yours.** Everything that matters is written to your own profile: no account is needed, and the parts that can sync are end-to-end encrypted (the Password Manager's vault never leaves a PC unsealed).
- **Nothing needs administrator rights until it truly does**, and then it is one prompt, through the same program.
- **Updates are verified.** The app checks its feed, downloads the installer, and installs it only if its SHA-256 matches. If the update fails, it falls back to the setup's own window.

<img src="docs/assets/divider.svg" alt="" width="100%" />

## For developers

```powershell
npm install
npm run dev      # the app, with hot reload
npm run build    # installer → backend/target/release/bundle/setup/MYLE.exe
npm run check    # svelte-check, TypeScript and the site check
npm test         # Rust tests (workspace), then the script tests
npm run lint     # cargo clippy -D warnings
```

You'll need **Node.js 20+**, **Rust** (stable, MSVC) and the **Visual Studio 2022 Build Tools** with the C++ workload, on Windows 10 or 11.

The flow of the app, the structure of the repository, every page's internals, the updater, the release workflow and the setup are all in the development guide.

<p align="center">
  <a href="docs/DEVELOPMENT.md"><kbd>Read the development guide</kbd></a>
</p>

## License

A proprietary project by **ThomasThanos**. You're free to use the official, unmodified builds for personal use. Copying, modifying, or redistributing the code needs written permission.

<p align="center">
  <a href="LICENSE"><kbd>Read the full license terms</kbd></a>
</p>

<br />

<p align="center">
  <a href="https://github.com/thomasthanos"><img src="docs/assets/footer.svg" alt="Made to make the little things easier. Built with care and coffee by ThomasThanos. Visit the GitHub profile." width="100%" /></a>
</p>
