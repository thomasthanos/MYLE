# Windows shortcut repair

**Request:** Only MYLE Windows. Desktop and Start Menu should be defaults, survive updates, and keep their icon. Offer the Windows-approved taskbar pin flow; do not touch the separate BlueStacks Passwords shortcut.

**Evidence:** The installed MYLE.exe exists. Desktop MYLE.lnk is missing; the existing Start Menu link opens the correct exe. Installed Apps still references the missing MakeYourLifeEasier.exe for its icon. Shortcut migration leaves Desktop missing and update logic interprets missing links as opt-outs. Shortcut validation ignores icons. No explicit stable shortcut AppUserModelID or shell refresh is present.

- [x] Prove regressions for missing defaults and stale icons.
- [x] Ship MYLE.ico as a stable resource, write consistent shell identity, refresh shortcut notifications, and repair existing app taskbar links without changing other pins.
- [x] Restore missing selected links on update while retaining explicit setup/Settings opt-outs.
- [x] Persist taskbar selection independently of a pending request. An interactive setup passes an explicit pin request to the foreground app; regular launches and passive updates never auto-prompt. Older Windows receive manual instructions, without registry/policy workarounds.
- [x] Repair the current Windows install's shortcuts and icon, verify through the shell, and run relevant tests/checks.

Follow-up: the user clarified that all unchecked installer options must remain off after updates. TaskbarShortcut now stores the selection; TaskbarPinPending records only an outstanding request. Completing/dismissing a request does not reset the checkbox. Missing Desktop/Start Menu/Startup links do not undo saved choices. The Start menu option is labeled All apps; it registers the app rather than claiming a pinned Start tile.

Local repair verified Desktop and Start Menu targets and the stable icon. Get-StartApps reports MYLE with the expected AppUserModelID. Native Windows context menus were used to pin MYLE to Start and the taskbar; both now offer Unpin. The resulting taskbar link points to this install's MYLE.exe and carries the expected AppUserModelID. Window-only proof is saved at backend/target/myle-pinned-proof.png. The installed executable still predates these source changes; no new installer has been installed or published.

Verification after the preference follow-up: npm test passed (663 Rust tests, 9 existing ignored, 322 JavaScript tests). New tests cover opt-outs, legacy preferences, retained choices after completed pin requests, explicit setup-only pin arguments, foreground timing, declined pin requests, and manual fallback. npm run check and npm run lint passed; both app and setup Vite builds passed. Windows setup smoke coverage was extended for stable icons, preserved completed taskbar requests, and unchecked shortcuts across updates; the live smoke script was not run against the user's active install.

The complete npm run build also passed, producing backend/target/release/bundle/setup/MYLE.exe (19,639,296 bytes). The 30-file payload includes MYLE.ico; its SHA-256 matches backend/icons/icon.ico. The setup artifact SHA-256 is 7DBDAC3DEF5B6EF241FA5E3D39D0A3B5607C67999C3AEADC14B79C9A68909FBE. This local setup has not been installed or published.
