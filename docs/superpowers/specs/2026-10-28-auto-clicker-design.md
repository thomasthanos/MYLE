# Auto Clicker & Multi-Point Macro design

Implements the user-supplied specification, initially isolated on `codex/auto-clicker` from v10.1.2. The user subsequently authorized integration and moved the work into `main` above `ca9f081`, removing the temporary branch/worktree. Continue on main; no push or release requested for this feature.

## Behavior
- Simple mode clicks at the current cursor; sequence mode moves to each saved screen coordinate, then performs its configured number of single/double left/right/middle clicks. Negative coordinates support secondary monitors.
- Intervals are milliseconds internally (10 ms minimum, 24 hours maximum); the simple editor also displays seconds. Sequence intervals separate clicks; transition delay runs after each step, including before a repeated sequence. A click count measures one single/double action, not individual down/up events.
- Finite repeat counts are positive; unlimited execution continues until Stop/F6. Simple mode uses an optional finite click count through the same repeat control.
- A short, cancellable 3-second start countdown leaves time to move away from the Start button. All timing waits are interruptible. Stop never leaves a mouse button pressed.
- F6 is registered with MOD_NOREPEAT when the tool is first opened and remains available while MYLE runs, including after page navigation. It starts the last valid configured profile, or stops a running/picking operation. A conflicting registration is reported visibly; there is no silent replacement. Profiles never auto-run at app startup.
- Pick location temporarily hooks physical left clicks outside MYLE, consumes the chosen down/up pair so the target is not activated, ignores injected clicks, and cleans up on success, Stop/F6/Escape, timeout or exit. Clicks in MYLE's host window remain available for Cancel and moving the window, including WebView2 child windows. A 30-second timeout and request-specific cancellation prevent abandoned or late-starting pickers.
- Profiles are versioned JSON in MYLE's roaming App Data folder, with active selection, validated bounds and unique IDs. Atomic writes preserve the previous file on error; malformed files are surfaced, never overwritten silently.

## Structure and look
Rust `autoclicker/{model,engine,native,store,mod}.rs` separates validation, testable execution, Win32 operations, persistence and Tauri orchestration. Svelte `auto-clicker/{api,model,state.svelte,AutoClicker,StepEditor}.ts/.svelte` owns typed data and page state. Add one registry entry and a matching sidebar icon, preserving existing numeric shortcuts by appending after System Maintenance.

Use MYLE's graphite/navy theme tokens: existing #0d0d0d/#171717/#262626 surfaces, #6b85ff accent and #e5484d danger, with its Comic Neue/Kalam typography. Preserve the user's updated timing presets, behavior card and profile controls in the execution panel. Sequence steps use actual order markers. The execution panel contains the primary blue Start or red Stop, repeat settings and live progress. Collapse to one column at narrow widths; keep columns in wide, short windows. Controls keep existing depth and focus treatments; no new font or palette dependency.

## Verification
Validate malformed/oversized configs and profiles, finite/infinite loop boundaries, click/step counts, cancellation during countdown/interval/transition, worker exclusion and profile round-trips. Simulated drivers exercise scheduling without moving/clicking the user's desktop. Check the real native input builder produces paired mouse events. Verify Svelte/TypeScript with zero diagnostics, the full test suite, Clippy and frontend build. Visually inspect a fixture-backed page without sending real clicks.
