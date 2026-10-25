# Maintenance output and disk scan fixes

**Goal:** Make SFC/DISM progress readable and restore Check Disk when Windows refuses to start chkdsk.exe, including from an elevated process.

**Scope:** The user's screenshot and request. Keep the existing Svelte/Tauri UI and single-task execution. Do not change Windows permissions or security policies, schedule a reboot, or request offline disk repair.

**Evidence:** The console honors carriage returns but appends progress terminated by newlines. Check Disk accepts every nonzero exit as a completed result. On this machine, chkdsk.exe /? fails to launch with Win32 access denied both normally and with confirmed administrator elevation.

Additional regression evidence: in Windows PowerShell 5.1, polling Start-Process -PassThru without first retaining its Handle leaves ExitCode null after exit; the wrapper then reports 0 for failure. Confirmed with a harmless cmd /c exit 7 probe, and with the disk worker's provider-failure tests. Cache the process handle before polling.

- [x] Add regression tests for repeated and fragmented progress, preserving diagnostic lines, and failed disk scans.
- [x] Compact percentage updates and show readable progress using the existing console styles. Verify backspace processing.
- [x] Use Windows Storage's online scan for Check Disk, select the actual system volume, report storage outcomes explicitly, and retain the task lock until the scan returns. Do not offer cancellation that cannot stop the storage service.
- [x] Run focused regression tests, frontend checks/build, and Rust maintenance/console tests; inspect the final diff.

The online scan uses Repair-Volume -Scan. Its operation reports detected corruption to Windows; it does not request SpotFix or OfflineScanAndFix. Update the old description rather than promising that the scan writes nothing.

Verified: the actual elevated Windows Storage scan on this PC returned NoErrorsFound (0). Regression tests cover success, findings, an existing scan, missing output, access denial, and Greek errors through the real PowerShell 5.1 worker and log redirect with a stubbed provider. npm test passed (651 Rust tests, 9 existing ignored tests, and 315 JavaScript tests). npm run check, npm run lint, and the Vite production build passed. Inspected the rendered maintenance cards; screenshot: backend/target/maintenance-output-preview.png. Temporary preview page and dev server were removed/stopped.
