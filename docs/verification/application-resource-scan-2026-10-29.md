# Resource and cancellation scan, third pass

29 October 2026. Follow-up to the two earlier application audits in this directory. Existing working-tree changes were preserved.

## Verified fixes

| Area | Correction and evidence |
| --- | --- |
| Console output | Short UTF-8/UTF-16 output waiting for encoding detection no longer joins separate lines at EOF. Repaints and blank lines use the streaming parser's rules. Truncated UTF-16 tails are decoded lossily and released. All native consumers handle multiple final lines. Three regressions failed before the fix; the console suite now passes 17 tests, with additional maintenance-callback and live Git diagnostic coverage. |
| Drive probes | Repeated refreshes share each unfinished drive probe instead of starting another thread. Eight refreshes of one blocked synthetic volume previously produced eight callbacks; now they produce one. There is at most one unfinished probe per drive letter. Completed/unavailable volumes are refreshed normally, and a stalled earlier letter does not hide ready later volumes. |
| Disk Analyzer loading | Overlapping page loads share their native drive request. A later completed request can still refresh free space, and failed loads can be retried. |
| Scanner queue | Completed directory listings use a synchronous queue sized to twice the worker count. The receiver drops before scoped workers are joined, releasing blocked senders during cancellation. A stalled-consumer fixture reproduced unbounded run-ahead before the fix; the bounded version cancels while a sender is blocked. |
| Incremental metadata cache | Strong LRU caching is limited to 32 archives and 16 MiB of uncompressed metadata JSON. Valid oversized metadata bypasses caching. Eviction releases cache ownership while existing `Arc` readers retain complete state. Four real-ZIP regressions cover entry/byte budgets, oversize bypass, reader survival, reload and LRU reuse. |
| Backup cancellation | Directory enumeration, per-file traversal and final completeness auditing check cancellation. A cancelled completeness stage no longer publishes a successful archive; existing temporary-archive cleanup runs for direct and staged paths. |
| Cleaner lifecycle | A failed category scan still waits for its Downloads preview before releasing the scan lock, preventing old results from crossing into a new scan. |
| Maintenance stop | Confirmations retain run identity. An old confirmation cannot stop a newer run of the same action. |
| Spotify recovery | Completing a locally started job clears cached active-job state, so a page revisit followed by failed final detection cannot leave the hub locked indefinitely. |

## Final verification

| Check | Result |
| --- | --- |
| `npm run check` | Passed; zero Svelte/TypeScript errors or warnings. |
| `npm test` | **849 Rust + 473 JavaScript tests passed: 1,322 total**, zero failures. Sixteen existing Rust tests remain ignored. |
| `npm run lint` | Full Rust workspace/all-targets Clippy passed with `-D warnings`. |
| Desktop, setup and mobile Vite builds | All three production frontend builds passed. |
| `git diff --check` | Passed. |

The full suite contains 22 additional cases compared with the preceding pass. Focused native results included 88 Project Backups tests, 57 Disk Analyzer tests, 17 console tests and both output-consumer regressions. Synthetic probes/scanner readers exercise the actual production code; ignored `.preview` wrappers enabled isolated compilation before integrated tests. Final logs are `.preview/scan3-final-*.log`.

Independent review found no blocking cache/cancellation issue. Root review checked probe completion ownership, fresh reads, queue receiver lifetime and console caller migration.

## Boundaries

The cache byte budget measures encoded JSON, with parsed allocations additional. Whole trees, MFT snapshots and the largest directory listing still scale with the data scanned. Synchronous Windows/filesystem calls and directory sorting cannot be interrupted mid-call; cancellation is checked between work units. Tests use temporary files and injected readers, without whole-drive scans, real cleanup/repair or cloud writes.

Automatic approval review rejected optional PowerShell deletion of two synthetic red-test folders in `%TEMP%`, reporting only **“blocked by policy.”** They were left untouched: `myle-archive-cancel-audit-direct-41860` and `myle-walk-cancel-flat-41860`. This did not affect implementation or verification.
