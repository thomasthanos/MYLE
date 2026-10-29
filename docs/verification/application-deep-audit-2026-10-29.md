# Deeper application correctness and resource review

29 October 2026. Continuation of `application-audit-2026-10-29.md`, covering the current desktop 10.1.5 workspace, mobile state and shared native code.

## Fixed and verified

| Area | Result |
| --- | --- |
| Password Manager | Replies containing decrypted data belong to their unlock epoch. Late list/reveal/save responses cannot restore locked data or reopen its panel. List requests keep the newest result; overlapping page loads share listener registration and status reads. An obsolete unlock cannot clear a newer unlock's busy state. |
| Desktop account | Session identity and request ordering guard access checks, settings application, uploads, errors and sync acknowledgements. Old responses cannot restore owner access or sign out a newer session, including a new login to the same account. |
| Mobile account and startup | Initialization shares its promise/listener and rejects stale profile reads. Startup completion releases its fallback timer and listener, including registration that finishes late. |
| OAuth loopback | Accepted connection handlers belong to the callback future through a `JoinSet`. Success, cancellation and timeout release spare connections; completed tasks are reaped. Real loopback tests observe socket closure. |
| Disk Analyzer | Scan hydration, progress, breadcrumbs, tree reveal and file/type results reject superseded work. Folder reads share their pending promise, so reveal waits for a loading parent before paging beyond row 500. Type results cannot cross folder changes. Typing coalesces largest-file queries with a 150 ms delay, cancelled by a new scan. |
| Repository UI | Status, fetch and remote reads respect request order and repository removal. Live diagnostic retention is bounded to 1,000 entries. |
| Streamed Git | One future owns both pipes and process waiting. Cancellation works after stderr closes; dropping the future releases its process job. Diagnostic capture retains a 256 KiB tail. Stdout remains exact up to 16 MiB and reports an explicit limit error above it. Regular Git stdin is joined with output handling, without a copied input buffer or detached writer task. |
| Release uploads | A watch channel retains the latest progress snapshot instead of accumulating updates. Completion flushes the final snapshot; cancellation polls while a stalled upload awaits the network. |
| Backup restore | Cancellation is checked during each 256 KiB copy block. Failure cleanup tracks created file paths and removes only empty created directories, preserving unrelated files added after the initial empty-directory check. |
| Backup comparison | Opened file/ZIP metadata enforces the 512 MiB hash limit. Runtime reads stop at the limit plus one detection byte if the source grows or metadata is misleading. An incomplete hash is never returned as a complete SHA-256. |
| Game Saves | Scan/discovery responses from an earlier settings revision cannot restore inventory from the previous backup folder. Current settings are checked again after obsolete work finishes. |
| Project Backups UI | Page reads cannot undo newer saved settings. History request tokens keep older replies/errors from replacing newer history or clearing its loading state. |

## Evidence

Behavioral regressions were run against the previous behavior before implementing fixes. Independent review found and reproduced additional Password Manager busy ownership and Disk Analyzer pagination/type-result races, which were also corrected.

- The filter fixture previously made three native queries for three quick input changes; it now makes one query for the final filter.
- Two simultaneous Password Manager loads now share their native subscriptions and status read.
- A 100,000-update upload fixture retains/delivers the final progress snapshot without a growing queue.
- A large stderr fixture retains its final fatal detail within the 256 KiB diagnostic limit; binary stdout remains byte-for-byte intact within its explicit limit.
- Live synthetic subprocess tests verify cancellation after stderr EOF, joined stdin, and descendant exit after cancellation or dropping the stream future.
- Restore fixtures prove mid-file cancellation and preservation of a second writer's unrelated file.
- Small injected hash limits and oversized ZIP metadata exercise the comparison cap without allocating a huge fixture.

## Final verification

| Command | Result |
| --- | --- |
| `npm run check` | Passed; zero Svelte/TypeScript errors or warnings. |
| `npm test` | **832 Rust tests + 468 JavaScript tests passed: 1,300 total.** Zero failures; 16 existing Rust tests remain ignored. |
| `npm run lint` | Passed: full Rust workspace/all-targets Clippy with `-D warnings`. |
| `npx vite build` | Desktop production build passed. |
| `npx vite build --mode setup` | Setup production build passed. |
| `npx vite build --mode mobile` | Mobile frontend production build passed. |
| `git diff --check` | Passed. |

The suite contains 70 additional test cases compared with the previous pass, including synthetic process fixtures. The final lint adjustment only removes an unnecessary reference in a test assertion. Logs are retained locally under `.preview/deep-*`.

## Coverage limits and remaining profiling targets

Tests use synthetic processes, loopback sockets, temporary archives and fixture accounts. Physical Android/iOS behavior, live provider sign-in and production release uploads still require their real environments. Frontend epochs prevent follow-on stale actions; they cannot undo a native import/upload already dispatched.

Process-tree ownership uses the existing best-effort Windows Job Object mechanism. If job assignment is unavailable, direct-child `kill_on_drop` still applies; descendant guarantees depend on successful assignment.

The bounds above apply to their named queues, previews and captured diagnostics. Whole-process peak memory/CPU on large real workloads, read-only Git/API response buffering and active-attempt OAuth connection concurrency remain profiling targets. No whole-application RAM reduction percentage is claimed. Historical incremental metadata retention and short multi-line console output were subsequently fixed and verified in `application-resource-scan-2026-10-29.md`.
