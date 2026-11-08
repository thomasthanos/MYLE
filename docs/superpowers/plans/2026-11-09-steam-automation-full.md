# Steam Automation Full Section Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the four-panel page with a full section behind the one Steam Automation button: library, idling, favorites, card farming, achievement unlocker, automatic idler and inventory manager.

**Architecture:** The engine, helpers, session and parsers from the previous plan stay. The library gains playtime and achievement progress through a session token. Runtime Start state and manual idling sit next to the saved settings. The page becomes a shell with its own menu and one component per view, sharing a game grid.

**Tech Stack:** Rust 2024, Tauri 2, reqwest, regex-lite, serde; Svelte 5, TypeScript; `cargo test`, `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-08-steam-automation-design.md`, section "Changed after the first version".

## Global Constraints

- Work on `main`. No branches, no push, no tag, no version bump.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- No code from `zevnda/steam-game-idler`. No new dependencies. No API key.
- Every command is named `steam_automation_*`. Cookies and the web token never reach the page or a log.
- Everything is off until the owner turns it on. Bulk selling and Remove listings ask first. A locked item is never sold. Automatic selling sells duplicates only.
- UI text is English, sentence case, MYLE's own tokens and controls, no ads or tiers.

## Review Focus

1. The web token expires or the session is rejected: the library says so and keeps its saved copy. (Task 1)
2. A game is both whitelisted and blacklisted: it is not farmed. (Task 2)
3. Pressing Stop during a run, or closing MYLE: helpers stop, and Started is not remembered across launches unless automatic is on. (Task 2)
4. Selling a locked item, or an item already listed in this session: refused. (Task 3)
5. A very long library or inventory: the grid stays responsive. (Task 5)

---

### Task 1: Library data

**Files:** Modify `backend/src/steam_automation/library.rs`, `commands.rs`, `backend/src/steam_helper/parse.rs`.

- [ ] Write failing tests: `parse_token`; `parse_owned_games` (appid, name, minutes); `apply_progress` (unlocked and total per game, none reads as 0 of 0); a portrait capsule URL with fallback to the header; library order by name; a failed token leaves the saved copy.
- [ ] Implement `LibraryGame { appid, name, cover_url, header_url, minutes, achievements_unlocked, achievements_total, achievements_ok }`, `fetch` through the token, batches of 200 for progress, 50 for `GetItems`. `steam_automation_library(force)`.
- [ ] Run `cargo test --lib steam_automation::library`, commit `Read the whole Steam library with playtime and achievements`.

### Task 2: Start, queues and lists

**Files:** Modify `settings.rs`, `decide.rs`, `engine.rs`, `commands.rs`, `lib.rs`.

- [ ] Write failing tests: settings gain `favorites`, `card_whitelist`, `card_blacklist`, `locked_assets`, and older files still load; `filter_cards` (whitelist narrows, blacklist always wins, empty whitelist means all); `decide` adds manual games alongside farming but not while stopped; engine uses the filter; `Started` starts from the automatic switches and is not saved.
- [ ] Implement `TickInput.manual_idle`, `World.manual`, runtime `Started`, and commands `steam_automation_set_started(feature, on)`, `steam_automation_idle_game(appid, on)`, `steam_automation_stop_all()`, `steam_automation_drops(force)`.
- [ ] Run `cargo test --lib steam_automation`, commit `Add Start, queues, whitelist and manual idling`.

### Task 3: Inventory actions

**Files:** Modify `inventory.rs`, `commands.rs`, `lib.rs`.

- [ ] Write failing tests with the inventory fixture: `parse_items` (name, game, kind for card, foil, background, emoticon and booster, icon URL, marketable); `kind_of` from `item_class` and `cardborder_1`; `parse_listing_ids` from a `mylistings` answer; `list_items` skips locked and already-listed assets, posts the seller's amount, and reports; `remove_listings` posts each id with the session and referer `https://steamcommunity.com/market/`, following pages by `start`.
- [ ] Implement them and commands `steam_automation_inventory(force)`, `steam_automation_price(hash_name)`, `steam_automation_list(items)`, `steam_automation_list_duplicates()`, `steam_automation_listings()`, `steam_automation_remove_listings()`.
- [ ] Run `cargo test --lib steam_automation::inventory`, commit `Add manual inventory actions`.

### Task 4: Page logic

**Files:** Modify `frontend/app/pages/steam-automation/{model.ts,api.ts,state.svelte.ts}`, `scripts/tests/steam-automation.test.mjs`.

- [ ] Write failing tests: `playtimeText` (`1,514h`, `2.4h`, none); `sortGames` by playtime, name and achievements; `searchGames`; `toggleInList`; `addAll` skips what is not eligible; `filterItems` for each filter; `duplicateIds` keeps one of each.
- [ ] Implement them, the typed API, and a state class holding the library, drops, inventory, selection and current view.
- [ ] Run `node --test scripts/tests/steam-automation.test.mjs` and `npm run check`, commit `Add the Steam Automation section's logic`.

### Task 5: The section

**Files:** Modify `SteamAutomation.svelte`. Create `GameGrid.svelte`, `GameTile.svelte`, and `views/{Games,Idling,Favorites,Cards,Achievements,Idler,Inventory}.svelte`. Delete `GamePicker.svelte`.

- [ ] Build the shell with its own menu, the sign-in and waiting banners, and each view as in the mockup. Tiles use `content-visibility: auto` so a long grid stays smooth.
- [ ] Bulk selling and Remove listings go through `lib/confirm.svelte` with `danger`.
- [ ] Check every view in a browser with a mocked backend, then remove the mock. `npm run check`, commit `Build the full Steam Automation section`.

### Task 6: Finish

**Files:** Modify `docs/DEVELOPMENT.md`.

- [ ] Update `### Steam Automation` for the section.
- [ ] Run `npm run check`, `npm test`, `npm run lint`. Expected: all pass. Commit `Describe the full Steam Automation section`.
