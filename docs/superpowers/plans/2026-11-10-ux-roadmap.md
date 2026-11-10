# UX, UI and disk space roadmap

> **For agentic workers:** this is a roadmap, not one implementation plan. Each phase gets its own plan in `docs/superpowers/plans/` when it starts (superpowers:writing-plans), and is carried out with superpowers:executing-plans. Phase 1 is listed task by task because it has started.

**Goal:** make MYLE easier to understand at a glance, with fewer surprises before anything is changed, and a smaller footprint on disk.

**Approach:** quick, local changes first (Phase 1), then structure and navigation (Phase 2), which needs the owner's choices. Space savings (Phase 3) and perceived speed (Phase 4) follow, and accessibility polish (Phase 5) runs alongside every phase.

**Tech stack:** Tauri 2, Rust, Svelte 5 with runes, TypeScript; `cargo test`, `node --test`, `npm run check`, `npm run lint`.

**Spec:** the roadmap presented on 2026-11-10 after the technical scan, and the owner's answers below.

## Global constraints

- Work on `main`. No branches, no push, no tag, no version bump unless asked.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- UI text is English and sentence case. Colours come from the tokens in `frontend/styles/tokens.css`. Shared controls come from `frontend/styles/controls.css`.
- Notifications are MYLE's own corner notice (`frontend/notice/`), never Windows toasts.
- Tooltips are drawn by `TooltipHost`, never by Windows.
- User data in `%APPDATA%\ThomasThanos\MakeYourLifeEasier` is never moved or renamed. Older builds, including the installed release, read the vault, the sign-in and the settings there by a fixed path. Only caches and internal files in `%LOCALAPPDATA%\...\data` may be tidied.
- A destructive action always asks first, and says what it removes and whether it can be undone.
- Steam Automation stays owner-only. Steam Game Helper is for everyone.

## Decisions made

| Question | Answer | Effect |
| --- | --- | --- |
| Windows notifications or MYLE's corner notice? | MYLE's own notice | Phase 3 item "Windows notifications instead of the notice window" is dropped. The notice stays, and shows what a click does inside the card. |
| Tidy the roaming data folder too? | Tried on 2026-11-10 and reverted | Moving `passwords.vault` into a folder made the installed 11.0.0 offer to create a new vault. Nothing was lost: the files went back. The roaming folder keeps its layout. |

## Open questions for the owner

1. Which pages do you use most, and which almost never? This decides the sidebar groups and what a first start shows.
2. Should System Cleaner, System Maintenance and Disk Analyzer become one "PC Care" page with tabs? Should there be a home page that shows the PC's state?
3. Should Ludusavi download the first time Game Saves opens, instead of shipping with every install (about 55 MB)?

---

## Phase 1: clearer at a glance (started)

### Task 1.1: custom tooltips in every window (done 2026-11-10)

- `frontend/splash.ts` and `frontend/installer/main.ts` mount `TooltipHost`, like the main and notice windows.
- `WindowButton` gives its tooltip as `data-tip`. A tooltip that comes and goes with a state ("Setup is working, please wait") no longer stays behind after the state ends.
- The notice card fills its small window, so its "Open" hint shows in the card on hover instead of in a tooltip over it.

### Task 1.2: nothing to act on before a scan (done 2026-11-10)

- System Cleaner: Select all, Clean selected and the category ticks wait for the first scan, and say why.
- The cards that need administrator rights carry an "Admin" tag. They are drawn fainter until approval is given, with what that means in the tooltip.

### Task 1.3: an explanation panel on every tool page

- **What:** the same short panel as Card farming has: what the tool does, what it will change, and one main button.
- **Where, in order:** System Cleaner, Windows Optimization (`debloat`), System Maintenance, Disk Analyzer.
- **Done when:** each page opens with that panel above its controls, and a page never starts work without one press of its main button.

### Task 1.4: a preview before every destructive action

- **What:** what will be deleted or changed, how much space, and whether there is an undo. The confirm dialog gets that list, not a generic sentence.
- **Where:** Windows Optimization apply, System Maintenance tasks, Software Health removals, Disk Analyzer deletes.
- **Done when:** no destructive action runs from a single click without the list, and the list matches what runs (tested in each page's `model.ts`).

### Task 1.5: one empty state and one error state

- **What:** a shared `lib/components/EmptyState.svelte` with an icon, one line, and one next step ("Scan", "Sign in", "Try again"). Errors say what failed and what to do.
- **Where:** every list and grid that can be empty (game grids, inventories, backups, saves, the vault).
- **Done when:** no page shows a blank area or a raw error string.

### Task 1.6: saved data shows at once

- **What:** as Steam Game Helper does, show the last result while a fresh one loads.
- **Where:** Install Apps (installed versions), Disk Analyzer (last scan), Software Health (last scan).
- **Done when:** reopening those pages shows the last result without waiting, marked with when it was taken.

## Phase 2: navigation and structure (waits for the open questions)

1. **Sidebar in groups** (Games, PC, Security, Tools) and **Ctrl+K** to jump to any page or action.
2. **Home page** with the PC's state: disk space, the last backup, updates, free games.
3. **Fewer pages:** System Cleaner, System Maintenance and Disk Analyzer as one "PC Care" page with tabs, if the owner agrees.
4. **First start:** a short tour, and a choice of which pages show.

## Phase 3: disk space

1. **Ludusavi on demand** (about 55 MB less on every install), if the owner agrees.
2. **Settings → Storage:** what MYLE takes, per folder, with a clear button for each cache. It shows the roaming folder but never moves anything in it.
3. **WebView2 components** MYLE does not use (Widevine and others, about 56 MB): find out whether they can be left out.
4. ~~Windows notifications instead of the notice window~~: dropped, the notice stays MYLE's own.

## Phase 4: speed as it feels

1. The start-up preload prepares only the pages the user opens.
2. The owner-only pages are not loaded at all for other users.
3. One shared clock for the whole app, stopped while the window is hidden.

## Phase 5: accessibility and polish

1. Every control reachable and usable from the keyboard, with visible focus.
2. Labels for screen readers on every icon-only button.
3. Contrast checked in both themes.
4. One set of words and icons for the same action everywhere.
5. `TooltipHost` takes a `title` over on first hover. A `title` that later becomes empty leaves its old text in `data-tip`. Conditional tooltips should use `data-tip`, as `WindowButton` now does, or `TooltipHost` should restore the `title` after the hover.

## Review focus

1. An older build reads the same data folders: nothing in a phase may change where user data lives. (Global constraints)
2. A page whose data cannot load: it shows the error state with a way forward, never a blank page. (Task 1.5)
3. A destructive action started from the keyboard: it still shows its preview and asks. (Task 1.4)
4. Saved data that is out of date: it is marked with when it was taken. (Task 1.6)
5. A tooltip on a disabled control: it still shows, drawn by `TooltipHost`. (Task 1.1)
