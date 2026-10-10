# Password Manager and updater reliability implementation plan

> **For agentic workers:** Use systematic-debugging and test-driven-development. Independent extension and updater UI tasks are delegated under dispatching-parallel-agents; backend process fixes and integration run in the parent session.

**Goal:** Repair reproducible Password Manager–extension bugs and updater stalls requested by the user on 2026-10-10.

**Architecture:** Preserve vault authentication, native host identity checks, exclusive pipe ownership, installer verification and the existing update UI. Fix operation cancellation and process ownership at the boundaries where the failures occur. Keep updater Settings state independent of page mounting.

**Tech stack:** Rust/Tokio/Tauri on Windows, browser extension JavaScript, Svelte/TypeScript, Rust and Node regression tests.

**Spec:** The user's request in this conversation: find and fix additional Password Manager–extension bugs and improve updater behaviour when stages get stuck.

## Global constraints

- Preserve the preceding uncommitted pipe takeover and malformed unlock-reply fixes.
- Changes stay on `codex/passwords-updater-reliability` in the current checkout, preserving existing edits; no deployment, install, or publication.
- Regression tests must fail on the original behaviour and pass after the fix.
- No destructive operations on the installed application or real vault.
- Never start a second updater installer after a timeout. Keep a live installer owned until verified exit; do not hard-kill its file transaction.
- Keep native pipe peer authentication and SHA-256 download verification intact.

## Review focus

1. Status returns `ok: true` alongside `enabled: false`; it must not resume filling.
2. Dismissal/cancellation occurs during a pending status call; no later save/sign-in may execute.
3. A stalled installer outlives its deadline; fallback must not race it.
4. A setup exits without showing a window; the app must stay usable and report failure.
5. A stale asset size or remounted Settings page must not present false completion or lose active progress.

## Task 1: Extension status and cancellation

**Files:** `extension/background.js`, relevant extension content/passkey scripts, `scripts/tests/extension-sites.test.mjs`, `scripts/tests/extension-passkeys.test.mjs`.

**Interfaces:** Native status `{ok, enabled, state}`; `untilUnlocked(stopped, limit)`; pending save nonce; native request deadlines.

- [x] Add tests: disabled status remains locked; native `new` stops as noVault; stopWaiting during an in-flight status returns unlocked false; dismissal during locked save never sends a later save; completed native replies clear their timers.
- [x] Run focused extension tests and observe regression failures.
- [x] Normalize native status, check cancellation after await, bind pending save to its nonce, and settle/cancel request timers.
- [x] Re-run the same tests and inspect all results.
- [x] Extend cancellation through native passkey verification and the final vault commit, scoped to browser/copy/origin/operation ID. Preserve older extensions and clear only the matching master-password prompt.

## Task 2: Owned updater processes and failure recovery

**Files:** `backend/src/updater.rs`; `backend/src/lib.rs` only if startup recovery requires changes.

**Interfaces:** `live_install(path)`; a bounded async child wait; `shown_window::Wait` outcomes.

- [x] Add Windows child-process regressions: a live setup survives its initial wait timeout and keeps maintenance/retry ownership until its exact child exits and handover completes. Verify using the owned child handle rather than PID reuse.
- [x] Add tests that a setup's `Exited`/`TimedOut` result is a failure. Keep `Shown` as the successful handoff; a real visible installer is outside automated test scope.
- [x] Run the focused Rust tests and observe failures.
- [x] Replace detached blocking status waits with owned Tokio children. Retain slow live setups in a background observer while returning an actionable timeout message. Stop only an invisible window-mode setup, which has not copied files yet. Propagate setup launch failure instead of quitting the app.
- [x] Run focused Rust updater tests and check the updater's exclusive operation covers verified exit and late handover. Fallback requires a confirmed nonzero exit; unknown live wait errors retain child ownership.

## Task 3: Updater UI consistency

**Files:** `frontend/splash/Splash.svelte`, `frontend/app/pages/settings/Settings.svelte`, a focused updater state module, `scripts/tests/updater-ui.test.mjs`.

**Interfaces:** `DownloadEvent`; persisted Settings operation state; existing `checkForUpdate`/`installUpdate` bridge.

- [x] Execute the actual splash progress handler with `assetSize=100`, downloaded `150`, total `null`; expect indeterminate progress and no stale denominator.
- [x] Add tests that Settings state survives another page consumer, overlapping operations do not replace an active update, and verification displays its own stage.
- [x] Run focused Node tests and observe failures.
- [x] Trust nullable backend totals and persist the Settings operation outside the component without changing other settings.
- [x] Run focused Node tests and Svelte/TypeScript checks.

## Task 4: Integration and final review

- [x] Refresh Firefox/store bundles with `node scripts/prepare-extension.mjs`.
- [x] Run `npm test`, `npm run check`, and `npm run lint`; inspect failures and fix those caused by the patch.
- [x] Inspect `git diff --check` and a fresh read-only review of all changes.
- [x] Report confirmed repairs, verification, and the fact that the installed binary needs a new build.

## Execution ledger

- Baseline: original pipe regression fixed in preceding task; full suite then passed (96 JS tests). Preserve those changes.
- Ruling: continue in the existing checkout on a feature branch to preserve current user edits and build dependencies; no new checkout is needed for these scoped changes.
- Investigation: live update handoff releases the single-instance lock and launches the next version before this one exits; pipe takeover fix remains necessary.
- Investigation: `live_install` times out a `spawn_blocking` wait without owning its process, then starts a competing fallback setup. The installer mutex serializes file writers, but retry attempts can fail while the first setup still runs.
- Extension: six initial status/save/timer regressions failed, then passed. Native passkey cancellation request failed deserialization before the added protocol support; focused native verification/commit and modal cleanup regressions passed after it.
- Updater process regressions: three failures before the first patch. Review identified that hard-killing a live installer could bypass rollback; a new slow-setup regression reproduced that issue. Revised observer/ownership implementation passed all 16 focused updater tests.
- Updater UI: six initial regressions failed; final eight passed. Svelte/TypeScript check passed with zero errors/warnings.
- Limitation: cancellation cannot forcibly close an already running blocking Windows Hello OS dialog, but the cancelled async operation cannot resume and commit a passkey.
- Review corrections: page departure now cancels prompted/conditional passkeys with a background navigation fallback; same-document URL changes remain cancellable without document IDs. Delayed updater handover retains exclusive ownership and leaves an actionable error view if it fails.
- Final verification: `npm test` passed (517 Rust + 128 Node tests, 8 Rust tests ignored); `npm run check` passed (zero Svelte errors/warnings plus TypeScript/site validation); `npm run lint` passed with warnings denied. Clippy's new large-enum diagnostic was corrected by boxing the pending child before the final runs.
- Final artifacts: Chrome/Firefox packages refreshed; generated background/content/passkey copies match the shared source; JS syntax and `git diff --check` passed.
- Fresh combined read-only review has no remaining findings after re-inspecting all corrections. Existing single-instance release and 20-second handover timeout behavior is unchanged; already-running Windows Hello OS dialogs are not forcibly closed.
- Delivery is source-only. No installed binary, real vault, or visible setup was exercised; a rebuilt app and refreshed extension are required. Unrelated `frontend/public/icons/Wand-Enhancer.svg` edits were preserved.
