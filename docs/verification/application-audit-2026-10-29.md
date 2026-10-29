# Application scan and optimizations

Completed 29 October 2026, following the scan started on 28 October. Verified the current Windows workspace with desktop version 10.1.5.

## Changes

| Area | Reproduced issue and correction |
| --- | --- |
| Startup and navigation | Secondary page components shared the initial JavaScript bundle. Keep Install Apps ready at startup and import other pages on demand, with permitted pages warmed sequentially while idle after the window appears. Foreground navigation and warming share pending imports. Errors belong to their page, failed imports can be retried, and saved scroll positions are restored after the component mounts. |
| Disk Analyzer | Concurrent first reads could duplicate folder requests. Late failures and out-of-order navigation could replace newer results. Install a loading placeholder immediately, check the scan/sort generation, and invalidate obsolete navigation and treemap requests. |
| Downloads | Cancellation could wait for a silent HTTP response to time out. Poll cancellation while awaiting headers, bodies and range retries. Join stopped ranges and drain buffered disk writes on every exit before cleanup or a single-connection fallback. Preserve the original transfer error when flushing also fails. |
| Build output | An unbounded channel allowed a fast build to outpace its log consumer. Bound the shared output queue to 64 chunks of up to 16 KiB, applying backpressure while keeping cancellation available. |
| Game covers | Oversized files and chunked HTTP bodies were fully buffered before validation, and completed downloads accumulated before persistence. Enforce the 600 KiB limit during reads, process completions incrementally with four active downloads, and move filesystem persistence/base64 work off the async runtime. |
| Vault synchronization | Repeated ID searches made merge lookups quadratic. Build an ID index while preserving the first matching local record and handling repeated new remote IDs. Persist upload acknowledgements in batches of 20, flushing the final batch and prior successes before a later upload error. Concurrent edits remain dirty. |
| Mobile sign-in | Returning from a slow OAuth callback could invalidate a successful sign-in. Native cancellation reports whether it actually cancelled an attempt; the frontend preserves callbacks already being completed and ties resume timers to their original attempt. |
| Browser extension | An unfinished QR image could be cached as a permanent decoding failure. Defer its scan until load, share its listeners, and remove listeners/timers when the watcher stops or the page is left. |

The final lint pass also corrected the cleaner progress counter's modulo expression to the equivalent `is_multiple_of(3)` call.

## Measured result

The desktop main JavaScript entry changed from **643,796 bytes to 167,772 bytes**, a reduction of approximately **73.9%**. These are production build output sizes. The metric covers the main entry; shared chunks, idle imports, total transfer size and startup duration require separate runtime measurements.

The build queue limit covers queued output chunks, with reader buffers and retained logs additional. Cover processing bounds individual images and active raw downloads; the returned data-URL map still grows with the number of requested covers.

## Verification

| Check | Result |
| --- | --- |
| `npm run check` | Site validation, Svelte and TypeScript passed; zero errors and warnings. |
| `npm test` | **816 Rust tests and 414 JavaScript tests passed**; zero failures. Sixteen existing Rust tests remain ignored by the default suite. |
| `npm run lint` | Full Rust workspace and all targets passed Clippy with `-D warnings`. |
| `npx vite build` | Desktop production build passed; the previous large main-chunk warning is gone. |
| `npx vite build --mode setup` | Setup frontend production build passed. |
| `npx vite build --mode mobile` | Mobile frontend production build passed. |
| `git diff --check` | Passed. |

Focused regressions cover stalled single/range downloads, cancellation before headers, range fallback integrity, buffered write completion/error precedence, bounded build output, oversized and incremental covers, vault batching/partial failure/concurrent edits, reordered vault records, Disk Analyzer request races, page module retries, mobile callback races and deferred QR images.

Browser checks used the real sidebar and content router in a local fixture:

- All twelve registered pages mounted through navigation without a page-module error.
- A deliberately delayed module reproduced the scroll bug: a saved offset of 800 restored to 0 before the fix and 800 after it.
- A deliberate import failure displayed its error, retried once, then mounted successfully with the saved scroll position.
- A public account landing on GitHub Releases returned to Install Apps. Owner buttons were absent and idle loading requested only the nine public lazy modules.
- Cleanup, Software Health and Disk Analyzer remain tabs within System Cleaner. Disk Analyzer has no separate sidebar entry.
- At the observed 1174 × 660 browser viewport, the sidebar fit without vertical overflow and the main content had no horizontal overflow.

Local logs, fixtures and the verified System Cleaner screenshot are retained under `.preview/audit-*` for this workspace.

## Validation limits

Mobile state tests and the host Rust workspace passed. Real Android/iOS device builds and live OAuth provider sign-in still need device validation. Browser navigation checks cover component loading and access presentation; native cleanup, installation, repair and real vault operations were not exercised against the user's machine or data.
