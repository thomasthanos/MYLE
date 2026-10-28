# Auto Clicker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add a local-profile auto clicker with cancellable multi-point execution and global F6.
**Architecture:** Validated serializable configuration feeds a Rust worker and platform input driver. A global hotkey uses the same state transitions as the page. Svelte receives status events and recovers status when returning to the page.
**Tech Stack:** Existing Tauri 2, windows-sys, serde, Svelte 5, TypeScript; no new dependencies.
**Spec:** `docs/superpowers/specs/2026-10-28-auto-clicker-design.md` and the user's detailed request.

## Global constraints
- Initially isolate on `codex/auto-clicker`; user later authorized main integration and moved the files there. Finish on main without pushing or releasing.
- Use MYLE theme tokens and shared controls.
- No real desktop click injection during automated tests.
- Existing commands, data and numeric navigation shortcuts retain their behavior.

## Review focus
- Stop during long waits, partial SendInput failures, or a double click must release any pressed button.
- Repeated Start/F6 and location picking cannot create overlapping workers.
- Negative screen coordinates and disconnected monitors must have useful validation/errors.
- Corrupt profile files or failed writes must not destroy saved data.
- Navigation and delayed async responses must not overwrite newer progress/configuration.

### Task 1: Models, persistence and scheduling
Files: create `backend/src/autoclicker/{model,store,engine}.rs` and module tests.
- [x] Write and run failing validation, profile IO and execution tests with an in-memory input driver.
- [x] Implement bounded models, atomic JSON storage, cancellable waits and finite/unlimited scheduling.
- [x] Verify interval/transition/countdown cancellation and count/order semantics.

### Task 2: Native runtime and commands
Files: create `backend/src/autoclicker/{native,mod}.rs`; modify `backend/src/lib.rs`.
Interfaces: six requested commands plus `autoclicker_configure(config)` to keep global F6 current without starting. Status event `autoclicker-status` carries run ID, phase, loop, step, click, totals and hotkey/error state.
- [x] Test event pair construction and runtime exclusion/stop using an input driver.
- [x] Implement SendInput, SetCursorPos, temporary mouse hook and RegisterHotKey on dedicated threads.
- [x] Register commands, managed state and exit cancellation; compile and run focused Rust tests.

### Task 3: Page, profiles and navigation
Files: create `frontend/app/pages/auto-clicker/{api.ts,model.ts,state.svelte.ts,AutoClicker.svelte,StepEditor.svelte}`, `frontend/lib/icons/IconAutoClicker.svelte`; modify registry and sidebar tests.
- [x] Write/run failing state/model tests for unit conversion, profile changes, latest configuration, pick cancellation and progress ordering.
- [x] Build editor, repeat controls, profiles CRUD and event-backed run controls; register page after System Maintenance.
- [x] Run focused Node tests and `npm run check`.

### Task 4: Review and verification
- [x] Run full `npm test`, `npm run check`, Clippy and Vite production build.
- [x] Inspect the actual page using a fixture preview at wide/narrow sizes and busy/picking states.
- [x] Review and fix relevant findings; commit only this feature locally and report branch/checks.

## Execution notes
- Native create_worktree returned "Not a git repository" because the chat's old attached worktree no longer exists. Used git's worktree fallback at `H:/Projects/ThomasThanos/MYLE-auto-clicker`, leaving the primary checkout untouched.
- User supplied the full implementation specification and explicit design discretion; proceed inline with these implementation choices.
- User moved all feature files into main after commit `ca9f081` and deleted the temporary branch/worktree. Existing Settings and global style edits are separate user work and must remain unstaged.
- Review fixes: cancel pending Starts before configuration returns and after in-flight Start acknowledgement; retain next-run configuration during a running snapshot; keep exact counters while throttling only notifications; let MYLE's host window receive Cancel clicks; identify and tombstone cancelled picker requests before late startup.
- The test runtime initially linked Tauri's dialog plugin through an unused AppHandle owner, requiring a Common Controls activation context. Replaced the runtime's concrete AppHandle storage with its status notification callback, keeping core tests independent of desktop UI resources.
- Automated scheduling tests use in-memory input drivers; no real desktop clicks are sent. Hook state transitions and native mouse event pairs are unit-tested. Interactive preview substitutes native APIs with disposable in-memory data.

## Final verification
- Full npm test: 787 Rust tests passed (15 existing ignored), 397 Node tests passed.
- npm run check: zero errors and zero warnings.
- npm run lint: Clippy with warnings denied passed.
- Vite production build and cargo Windows desktop debug build passed. Vite retains the existing main-bundle size advisory.
- Fixture UI verified for simple/sequence, busy/Stop, coordinate selection, graphite/navy themes, and narrow/short layouts; no real desktop input injection.
- Temporary branch/worktree already removed by the user. Feature is committed locally on main; no push/release.
