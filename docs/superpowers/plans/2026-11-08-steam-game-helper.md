# Steam Game Helper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an owner-only page showing what is free on Steam now, twelve weekly picks, and every deal, with a corner notice when something new becomes free.

**Architecture:** A new Rust module reads Steam's public store (search listing for lists, `GetItems` for details, `Query` for the free-weekend scan), caches each section on disk, and exposes one owner-gated command per section so sections load independently. A background task reuses the same free-games code to raise MYLE's existing corner notice. A lazy Svelte page renders the sections.

**Tech Stack:** Rust 2024, Tauri 2, reqwest, regex-lite, serde; Svelte 5 runes, TypeScript; `cargo test`, `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-08-steam-game-helper-design.md`

## Global Constraints

- Work on branch `feature/steam-game-helper` from `main`. Never push, tag, bump the version or release.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- No code from `zevnda/steam-game-idler`, and not its embedded key.
- No Steam password, cookie or API key anywhere.
- No new Cargo or npm dependencies.
- The page sends filters and app ids; Rust builds every URL. Steam's HTML is parsed for ids and never rendered.
- Network goes through `crate::download::http_client("MYLE")`, about 150 ms between requests. `country_code: GR`, `cc=gr`, `language: english`, `l=english`.
- Constants: picks `12`, story share `8`, picks floor `85`% and `5000` reviews; main-row floor `1000` reviews; cache `3600` s; forced refresh floor `60` s; watcher every `3` h; free-weekend scan `1000` games in pages of `100`; `GetItems` batches of `50`.
- UI text is English, sentence case. Colours only from `styles/tokens.css`; every animation respects `prefers-reduced-motion`.
- `npm run lint` is `-D warnings`. `mod steam_helper` carries `#[allow(dead_code)]` from Task 1 until Task 6 removes it.

## Review Focus

1. Steam answers with HTML, an error page, or a changed shape: that section shows an error and keeps its saved data; nothing panics. (Tasks 1, 2, 5)
2. An offer's end time passes while saved or while the page is open: it stops being shown as free. (Tasks 5, 9)
3. The owner flips filters quickly: a late answer for old filters must not replace the current one. (Task 9)
4. The owner signs out or another account signs in while the watcher sleeps: no network, no notice. (Task 7)
5. A free or brand-new game has no reviews and no price: it reads "No reviews yet" rather than "0% positive", and is not amber-flagged as bad. (Tasks 1, 9)

---

### Task 1: Types and parsing

**Files:**
- Create: `backend/src/steam_helper/mod.rs`, `model.rs`, `parse.rs`, `fixtures/items.json`
- Modify: `backend/src/lib.rs` (after `mod spotify_hub;`)

**Interfaces:**
- Produces, in `model.rs`, all `Serialize + Deserialize` with `#[serde(rename_all = "camelCase")]`:
  - `Filters { online_coop: bool, single_player: bool, story_only: bool }` (`Copy, Default`), with `fn key(&self) -> String` returning e.g. `"c1s0t0"`.
  - `Game { appid: u32, name: String, cover_url: Option<String>, store_url: String, review_percent: u8, review_count: u32, is_free: bool, price: Option<String>, original_price: Option<String>, discount_percent: u8, deal_ends_at: Option<u64>, online_coop: bool, single_player: bool, story: bool }`
  - `OfferKind { FreeWeekend, FreeToKeep }`, `FreeOffer { game: Game, kind: OfferKind, label: String, ends_at: Option<u64>, offer_id: String }`
- Produces, in `parse.rs`, `pub(crate)`:
  - `FreeWeekend { end_time: u64, text: Option<String>, appid: Option<u32> }`, `Item { game: Game, adult: bool, free_weekend: Option<FreeWeekend> }`, `SearchPage { ids: Vec<u32>, rows: u32, total: u32 }`
  - `fn items(json: &str) -> Result<Vec<Item>, String>` for both `GetItems` and `Query` (`response.store_items`; a `response` with no list is an empty `Vec`; no `response` is `Err`). Skip entries with no `appid` or `name`.
  - `fn search_page(json: &str) -> Result<SearchPage, String>`

- [ ] **Step 1: Create the branch**

Run: `git switch -c feature/steam-game-helper`
Expected: `Switched to a new branch`

- [ ] **Step 2: Write the fixture and failing tests**

`fixtures/items.json` is a `{"response":{"store_items":[…]}}` built from the spec's field table, with these games: Dying Light 239140 (95%, 376585 reviews, categories `[2,1,9,38]`, `-90`, `2,49€`, was `24,99€`, ends `1794243600`, header `9d1b810b85ae46a0acbdfddf583ccbb81271183e/header.jpg`); NEKOPARA Vol. 2 420110 (tags incl. 9130, descriptors `[1,5]`); Baldur's Gate 3 1086940 (tags incl. 1742, 12095, 6650, descriptors `[1,2,5]`); Cyberpunk 2077 1091500 (tag 6650, descriptors `[1,2,5]`); Warframe 230410 (`is_free: true`, no purchase option); PAYDAY 3 1272080 (`free_weekend: {start_time: 1793901600, end_time: 1794247200}`); Modern Warfare 4 4435490 (`free_weekend: {end_time: 1794420000, text: "Call of Duty®: Modern Warfare® 4 - DMZ Free Trial", appid: 4269880}`); adult test rows 1, 2, 3 (descriptor 3, descriptor 4, tag 24904); and bare row 9 with only `appid` and `name`.

```rust
fn by_id(appid: u32) -> Item { items(ITEMS).unwrap().into_iter().find(|i| i.game.appid == appid).unwrap() }
#[test] fn a_game_is_read_whole() {
    let g = by_id(239140).game;
    assert_eq!((g.name.as_str(), g.review_percent, g.review_count), ("Dying Light", 95, 376585));
    assert_eq!((g.discount_percent, g.price.as_deref(), g.original_price.as_deref(), g.deal_ends_at), (90, Some("2,49€"), Some("24,99€"), Some(1794243600)));
    assert!(g.online_coop && g.single_player && !g.story);
    assert_eq!(g.cover_url.as_deref(), Some("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/239140/9d1b810b85ae46a0acbdfddf583ccbb81271183e/header.jpg"));
    assert_eq!(g.store_url, "https://store.steampowered.com/app/239140");
}
#[test] fn the_adult_rule_matches_what_steam_really_sends() {
    for id in [420110, 1, 2, 3] { assert!(by_id(id).adult, "{id}"); }
    for id in [1086940, 1091500, 239140] { assert!(!by_id(id).adult, "{id}"); }
}
#[test] fn a_free_game_has_no_price() { let g = by_id(230410).game; assert!(g.is_free); assert_eq!(g.price, None); }
#[test] fn a_free_weekend_keeps_its_trial() {
    let w = by_id(4435490).free_weekend.unwrap();
    assert_eq!((w.end_time, w.appid), (1794420000, Some(4269880)));
    assert!(by_id(1272080).free_weekend.unwrap().text.is_none());
}
#[test] fn missing_fields_do_not_lose_the_game() {
    let g = by_id(9).game;
    assert_eq!((g.review_percent, g.review_count, g.cover_url, g.price), (0, 0, None, None));
}
#[test] fn what_is_not_steams_answer_is_an_error() {
    assert!(items("<html>").is_err()); assert!(items("{}").is_err()); assert!(search_page("<html>").is_err());
}
#[test] fn search_counts_rows_and_skips_bundles() {
    let json = r#"{"total_count":498,"results_html":"<a data-ds-appid=\"1211020\"></a><a data-ds-appid=\"239140\"></a><a data-ds-appid=\"10,20\"></a>"}"#;
    let p = search_page(json).unwrap();
    assert_eq!((p.ids, p.rows, p.total), (vec![1211020, 239140], 3, 498));
}
```

- [ ] **Step 3: Run, expect a compile failure**

Run: `cargo test --manifest-path backend/Cargo.toml --lib steam_helper`
Expected: FAIL, `steam_helper` not found.

- [ ] **Step 4: Implement `model.rs`, `parse.rs`, `mod.rs`**

`online_coop` is category 38, `single_player` category 2, `story` tag 1742. `adult` is tag 9130 or 24904, or descriptor 3 or 4. Add `#[allow(dead_code)] mod steam_helper;` to `lib.rs`.

- [ ] **Step 5: Run the tests, expect PASS, then commit**

```bash
git add backend/src/steam_helper backend/src/lib.rs docs/superpowers
git commit -m "Add Steam Game Helper types and store parsing"
```

### Task 2: Talking to Steam

**Files:** Create `backend/src/steam_helper/store.rs`.

**Interfaces:**
- Consumes: `Filters`, `Item`, `SearchPage`, `items`, `search_page`.
- Produces, `pub(crate)`:
  - `enum ListingKind { Deals, FreeToPlay, FreeToKeep, Ranked }`, `enum Mode { OnlineCoop, SinglePlayer }`, `fn modes(f: Filters) -> Vec<Option<Mode>>`
  - `fn search_url(kind: ListingKind, mode: Option<Mode>, story: bool, start: u32, count: u32) -> String`
  - `trait StoreApi: Send + Sync { fn get(&self, url: &str) -> impl Future<Output = Result<String, String>> + Send; }`, `struct HttpStore` with `fn new() -> Result<Self, String>` (non-2xx is `Err`; enforces the request gap)
  - `async fn listing<S: StoreApi>(api: &S, kind: ListingKind, mode: Option<Mode>, story: bool, start: u32, count: u32) -> Result<SearchPage, String>`
  - `async fn details<S: StoreApi>(api: &S, ids: &[u32]) -> Result<Vec<Item>, String>` (input order kept)
  - `async fn scan_free_weekends<S: StoreApi>(api: &S) -> Result<Vec<Item>, String>`
  - `#[cfg(test)] struct FakeStore` with `fn new(routes: Vec<(&str, Result<String, String>)>)` (first route whose text the URL contains wins) and `fn calls(&self) -> Vec<String>`

- [ ] **Step 1: Write failing tests**

```rust
const BASE: &str = "https://store.steampowered.com/search/results/?infinite=1&category1=998&cc=gr&l=english";
#[test] fn listing_urls() {
    assert_eq!(search_url(ListingKind::Deals, Some(Mode::OnlineCoop), false, 0, 100), format!("{BASE}&sort_by=Reviews_DESC&specials=1&category2=38&start=0&count=100"));
    assert_eq!(search_url(ListingKind::FreeToPlay, Some(Mode::SinglePlayer), true, 100, 50), format!("{BASE}&sort_by=Reviews_DESC&maxprice=free&category2=2&tags=1742&start=100&count=50"));
    assert_eq!(search_url(ListingKind::Ranked, None, true, 0, 100), format!("{BASE}&sort_by=Reviews_DESC&tags=1742&start=0&count=100"));
}
// Sorting by reviews drops games with none, and a brand-new 100%-off game may have none.
#[test] fn the_free_to_keep_listing_is_not_sorted() {
    assert_eq!(search_url(ListingKind::FreeToKeep, None, false, 0, 100), format!("{BASE}&maxprice=free&specials=1&start=0&count=100"));
}
#[test] fn modes_follow_the_filters() {
    assert_eq!(modes(Filters::default()), vec![None]);
    assert_eq!(modes(Filters { online_coop: true, single_player: true, story_only: false }), vec![Some(Mode::OnlineCoop), Some(Mode::SinglePlayer)]);
}
#[tokio::test] async fn details_go_out_in_fifties_in_order() { /* 120 ids; FakeStore returns items by id; assert 3 calls and the returned ids equal the input order */ }
#[tokio::test] async fn a_failed_batch_is_an_error() { /* second batch Err => details is Err */ }
#[tokio::test] async fn the_scan_keeps_only_free_weekends_and_stops_at_a_short_page() {
    /* page 0: 100 items, 2 with free_weekend; page 100: 5 items, 1 with free_weekend.
       Assert 3 items, 2 calls, and that each call's decoded input_json has "sort":12 and "start" of 0 then 100. */
}
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_helper::store`

- [ ] **Step 3: Implement `store.rs`**

Build `input_json` with `reqwest::Url::parse_with_params`. `Query` and `GetItems` use the `data_request` from the spec. Scan 10 pages at most.

- [ ] **Step 4: Run, expect PASS, then commit**

```bash
git add backend/src/steam_helper
git commit -m "Read Steam store listings, details and free weekends"
```

### Task 3: Free offers, rows and See all

**Files:** Create `backend/src/steam_helper/sections.rs`.

**Interfaces:**
- Consumes: Task 2's functions.
- Produces, `pub(crate)`:
  - `enum RowKind { Deals, FreeToPlay }` (`Deserialize`, camelCase), `struct MorePage { games: Vec<Game>, next_cursor: Option<u32> }` (`Serialize`, camelCase)
  - `fn offers_from(weekends: Vec<Item>, to_keep: Vec<Item>) -> Vec<FreeOffer>`
  - `fn merge_ranked(lists: Vec<Vec<Game>>) -> Vec<Game>`
  - `async fn free_right_now<S: StoreApi>(api: &S) -> Result<Vec<FreeOffer>, String>`
  - `async fn row<S: StoreApi>(api: &S, kind: RowKind, filters: Filters) -> Result<Vec<Game>, String>` (one 100-row page per mode, at most 8 games)
  - `async fn more<S: StoreApi>(api: &S, kind: RowKind, filters: Filters, cursor: u32) -> Result<MorePage, String>` (50 rows per mode)

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn offers_group_by_trial_and_end_soonest_first() {
    let out = offers_from(vec![mw4(), cod_same_trial(), payday()], vec![to_keep(7, None)]);
    assert_eq!(out.iter().map(|o| o.game.appid).collect::<Vec<_>>(), vec![1272080, 4435490, 7]);
    assert_eq!(out[1].offer_id, "4269880:1794420000");
    assert_eq!(out[1].label, "DMZ Free Trial");
    assert_eq!(out[0].label, "Free weekend");
    assert_eq!(out[2].label, "Free to keep");
    assert_eq!(out[0].offer_id, "1272080:1794247200");
}
#[test] fn adult_offers_are_dropped() { /* adult Item in either list is absent */ }
#[test] fn merging_is_a_union_ordered_by_percent_then_reviews() {
    let m = merge_ranked(vec![vec![g(1, 90, 100), g(2, 95, 10)], vec![g(1, 90, 100), g(3, 95, 50)]]);
    assert_eq!(m.iter().map(|x| x.appid).collect::<Vec<_>>(), vec![3, 2, 1]);
}
#[tokio::test] async fn a_row_needs_a_thousand_reviews_and_stops_at_eight() { /* 20 ids, 2 below 1000, 1 adult; assert 8 returned, none below 1000 */ }
#[tokio::test] async fn see_all_has_no_review_floor_and_advances_by_rows() { /* 50 rows, some under 1000; next_cursor == Some(50) */ }
#[tokio::test] async fn see_all_ends_on_a_short_page() { /* 30 rows; next_cursor == None */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_helper::sections`

- [ ] **Step 3: Implement `sections.rs`**

`label` for a free weekend is the text after the last ` - ` in `text`, else `Free weekend`. `offer_id` is `{trial or game appid}:{end, or 0}`. `free_right_now` calls `scan_free_weekends`, then `listing(FreeToKeep)` and `details` on any ids. `row` and `more` pass `filters.story_only` as `story`, drop adult items and combine modes with `merge_ranked`. Add one `#[tokio::test] #[ignore]` named `live_steam_still_answers`, using `HttpStore`, that asserts `row(Deals, online co-op)` returns games with covers and `free_right_now` is `Ok`.

- [ ] **Step 4: Run, expect PASS, then commit**

```bash
git add backend/src/steam_helper
git commit -m "Build free offers, ranked rows and See all pages"
```

### Task 4: Weekly picks

**Files:** Create `backend/src/steam_helper/picks.rs`. Modify `backend/src/game_saves/mod.rs` (`pub(crate) mod headless;`) and `headless.rs:213` (`pub(crate) fn local_seconds`).

**Interfaces:**
- Consumes: `listing`, `details`, `ListingKind::Ranked`, `modes`.
- Produces, `pub(crate)`:
  - `fn week_key(local_day: i64) -> i64` (the Monday's day number, one-to-one with the ISO week, no date crate)
  - `fn choose(week: i64, story: &[u32], other: &[u32], exclude: &[u32], story_only: bool) -> Vec<u32>`
  - `async fn weekly_picks<S: StoreApi>(api: &S, file: &Path, filters: Filters, week: i64) -> Result<Vec<Game>, String>`

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn a_week_starts_on_monday() {
    assert_eq!(week_key(20765), 20759); // Sunday 2026-11-08
    assert_eq!(week_key(20766), 20766); // Monday 2026-11-09
    assert_eq!(week_key(20759), 20759);
}
#[test] fn the_same_week_gives_the_same_picks_and_another_does_not() {
    assert_eq!(choose(20759, &ids(1..60), &ids(100..300), &[], false), choose(20759, &ids(1..60), &ids(100..300), &[], false));
    assert_ne!(choose(20759, &ids(1..60), &ids(100..300), &[], false), choose(20766, &ids(1..60), &ids(100..300), &[], false));
}
#[test] fn eight_story_then_four_others() {
    let c = choose(20759, &ids(1..60), &ids(100..300), &[], false);
    assert_eq!(c.len(), 12);
    assert_eq!(c.iter().filter(|i| **i < 60).count(), 8);
}
#[test] fn story_only_is_all_story() { assert!(choose(20759, &ids(1..60), &ids(100..300), &[], true).iter().all(|i| *i < 60)); }
#[test] fn a_short_tier_is_filled_by_the_other() { assert_eq!(choose(20759, &ids(1..4), &ids(100..300), &[], false).len(), 12); }
#[test] fn last_week_is_left_out() { let e = ids(1..30); assert!(choose(20766, &ids(1..60), &ids(100..300), &e, false).iter().all(|i| !e.contains(i))); }
#[test] fn last_week_comes_back_only_when_there_is_nothing_else() { assert_eq!(choose(20766, &ids(1..6), &[], &ids(1..6), true).len(), 5); }
#[tokio::test] async fn inside_a_week_the_list_is_kept_and_prices_refresh() { /* run twice, same week, price changes in FakeStore; same appids, new price, listing called only the first time */ }
#[tokio::test] async fn a_corrupt_file_starts_fresh() { /* garbage in the file: Ok with 12 games */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_helper::picks`

- [ ] **Step 3: Implement `picks.rs`**

Order candidates by the FNV-1a hash of `(week, appid)`. `week_key` subtracts `(day + 3).rem_euclid(7)`. A game qualifies at `review_percent >= 85 && review_count >= 5000`, adult dropped. Read the first 200 rows of each tier: mode listings with `story: true`, and unless `story_only`, with `story: false` minus the story ids. `picks.json` maps `Filters::key()` to `{ week, appids, previous }`, written with `game_saves::atomic::write`. Exclude `previous` first, and relax only when fewer than 12 remain.

- [ ] **Step 4: Run, expect PASS, then commit**

```bash
git add backend/src/steam_helper backend/src/game_saves
git commit -m "Choose twelve weekly picks from reviews"
```

### Task 5: Cache

**Files:** Create `backend/src/steam_helper/cache.rs`.

**Interfaces:**
- Produces, `pub(crate)`:
  - `struct Section<T> { items: Vec<T>, updated_at: Option<u64>, stale: bool, error: Option<String> }` (`Serialize`, camelCase)
  - `struct DiskCache` with `fn new(dir: PathBuf) -> Self`
  - `async fn through_cache<T, F, Fut>(cache: &DiskCache, key: &str, force: bool, now: u64, fetch: F) -> Section<T>` where `T: Serialize + DeserializeOwned`, `F: FnOnce() -> Fut`, `Fut: Future<Output = Result<Vec<T>, String>>`
  - `fn live_offers(offers: Vec<FreeOffer>, now: u64) -> Vec<FreeOffer>` (drops any with `ends_at < now`)

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test] async fn a_fresh_entry_costs_no_request() { /* saved at 1000; now 4599; fetch panics if called; stale false */ }
#[tokio::test] async fn an_old_entry_is_fetched_again() { /* now 4600; fetch called */ }
#[tokio::test] async fn a_failure_keeps_what_was_saved_and_says_so() { /* items kept, stale true, error Some, updated_at is the old time */ }
#[tokio::test] async fn a_failure_with_nothing_saved_is_empty_with_an_error() { /* items empty, stale false, error Some, updated_at None */ }
#[tokio::test] async fn refresh_is_held_to_once_a_minute() { /* force at age 59: no fetch; force at age 60: fetch */ }
#[tokio::test] async fn a_corrupt_entry_counts_as_nothing() { /* garbage file: fetch called */ }
#[test] fn an_offer_past_its_end_is_not_live() { /* ends_at 999, now 1000: gone; None and 1000 stay */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_helper::cache`

- [ ] **Step 3: Implement `cache.rs`** with one JSON file per key, written with `game_saves::atomic::write`. A key is a section name plus `Filters::key()`, safe as a file name.

- [ ] **Step 4: Run, expect PASS, then commit**

```bash
git add backend/src/steam_helper
git commit -m "Cache Steam Game Helper sections on disk"
```

### Task 6: Commands behind the owner gate

**Files:** Create `backend/src/steam_helper/commands.rs`. Modify `backend/src/account/owner.rs:9` and its test at `:154`, `backend/src/lib.rs` (`.manage`, handler).

**Interfaces:**
- Consumes: Tasks 3 to 5.
- Produces: `SteamHelperState` (`Default`) with `fn store(&self) -> Result<Arc<HttpStore>, String>` (created once), and `#[tauri::command]` functions in `steam_helper::commands`:
  - `steam_helper_free(force: bool) -> Result<Section<FreeOffer>, String>` (passes results through `live_offers` on every return, cached or not)
  - `steam_helper_picks(filters: Filters, force: bool) -> Result<Section<Game>, String>`
  - `steam_helper_row(kind: RowKind, filters: Filters, force: bool) -> Result<Section<Game>, String>`
  - `steam_helper_more(kind: RowKind, filters: Filters, cursor: u32) -> Result<MorePage, String>`

- [ ] **Step 1: Write failing tests in `owner.rs`**

Extend `only_the_owner_only_pages_commands_are_gated` with `assert!(is_owner_only_command("steam_helper_free"));` and `assert!(!is_owner_only_command("steamhelper_free"));`. In `every_command_of_the_owner_only_pages_is_gated`, add `|| line.starts_with("steam_helper::commands::")` to the filter. The prefix includes `commands::` so the watcher's start-up line in `.setup` is not read as a command.

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib account::owner`

- [ ] **Step 3: Implement**

Add `"steam_helper_"` to `OWNER_ONLY_COMMANDS`. Write `commands.rs`, with the cache and picks under `storage::local_dir()/steam-helper/`, and the week from `week_key(local_seconds(now)? as i64 / 86400)`. Register each command on its own line starting `steam_helper::commands::`, add `.manage(steam_helper::commands::SteamHelperState::default())`, update the `owner_gate` doc comment, and remove `#[allow(dead_code)]` from `mod steam_helper`.

- [ ] **Step 4: Verify and commit**

Run: `cargo test --manifest-path backend/Cargo.toml --lib account::owner` and `npm run lint`
Expected: PASS, and no warnings.

```bash
git add backend/src
git commit -m "Expose Steam Game Helper commands to the owner only"
```

### Task 7: Finding what is new

**Files:** Create `backend/src/steam_helper/watcher.rs`. Modify `backend/src/lib.rs` (`.setup`).

**Interfaces:**
- Consumes: `free_right_now`, `through_cache`, `live_offers`, `AccountState::is_owner`.
- Produces, `pub(crate)`:
  - `struct Seen { offers: HashMap<String, Option<u64>> }` (`Serialize + Deserialize`, `Default`)
  - `pub struct FreeNews { game: String, ends_at: Option<u64>, more: u32 }` (public fields; `Clone, Debug, PartialEq, Serialize, Deserialize`, camelCase)
  - `fn diff(seen: Option<Seen>, offers: &[FreeOffer], now: u64) -> (Seen, Vec<FreeOffer>)`
  - `async fn tick<S: StoreApi>(api: &S, cache: &DiskCache, seen_file: &Path, is_owner: bool, now: u64) -> Option<FreeNews>`
  - `fn start(app: AppHandle)` (first check 60 s after launch, then every 3 h; re-reads `is_owner` each time)

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn the_first_run_only_remembers() { let (s, new) = diff(None, &[o("a:10", Some(10))], 5); assert!(new.is_empty()); assert!(s.offers.contains_key("a:10")); }
#[test] fn a_new_offer_is_reported_once() { /* diff with a seen set: new has it; diff again with the result: empty */ }
#[test] fn the_same_game_returning_later_is_new() { /* "1272080:100" seen; "1272080:900" reported */ }
#[test] fn expired_offers_are_forgotten() { /* entry with end 10, now 11, absent: gone */ }
#[test] fn an_offer_with_no_end_stays_while_listed() { /* None end: kept while present, gone once absent */ }
#[tokio::test] async fn someone_else_costs_no_network() { /* is_owner false: None, api.calls() empty, no file written */ }
#[tokio::test] async fn a_failed_scan_forgets_nothing_and_notifies_nobody() { /* saved cache, Err from store: None, seen file unchanged */ }
#[tokio::test] async fn a_corrupt_seen_file_counts_as_a_first_run() { /* garbage: None, file rewritten valid */ }
#[tokio::test] async fn several_new_offers_become_one_notice() { /* 3 new: game is the soonest to end, more == 2 */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_helper::watcher`

- [ ] **Step 3: Implement `watcher.rs`.** `tick` returns `None` on `section.stale` or `section.error`. In `.setup`, call `steam_helper::watcher::start(app.handle().clone())`. For this task `start` runs `tick` and discards the result; Task 8 shows it.

- [ ] **Step 4: Run, expect PASS, then commit**

```bash
git add backend/src
git commit -m "Notice when something new becomes free on Steam"
```

### Task 8: The corner notice

**Files:** Modify `backend/src/game_saves/notice.rs`, `backend/src/lib.rs` (`page_asked`, `notice_done`, the single-instance callback), `backend/src/steam_helper/watcher.rs`.

**Interfaces:**
- Consumes: `FreeNews`.
- Produces: `NoticeKind::FreeOnSteam`; `Notice.offer: Option<FreeNews>` with `#[serde(default)]`; `pub const OPEN_STEAM_HELPER_FLAG: &str = "--open-steam-helper"`; `fn open_flag_for(kind: NoticeKind) -> &'static str`; `fn open_page(kind: NoticeKind)` replacing `open_game_saves`; `Notice::free_on_steam(news: FreeNews, at: u64) -> Notice`.

- [ ] **Step 1: Write failing tests**

```rust
// notice.rs
#[test] fn a_free_game_notice_survives_the_command_line() { /* name with quotes; encode, decode, equal */ }
#[test] fn an_older_notice_without_an_offer_still_reads() { /* JSON from before: decodes, offer None */ }
#[test] fn the_click_goes_to_the_right_page() {
    assert_eq!(open_flag_for(NoticeKind::FreeOnSteam), "--open-steam-helper");
    assert_eq!(open_flag_for(NoticeKind::BackedUp), "--open-game-saves");
}
// lib.rs
#[test] fn the_helpers_flag_opens_its_page() { assert_eq!(page_asked(&["x".into(), "--open-steam-helper".into()]), Some("steam-helper")); }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib notice`

- [ ] **Step 3: Implement the backend**

`notice_done` takes the managed `Notice` and calls `open_page(notice.kind)`. The single-instance callback emits `myle-navigate` with `steam-helper` for the new flag. `watcher::start` calls `notice::show(&Notice::free_on_steam(news, now))`.

- [ ] **Step 4: Run `cargo test --manifest-path backend/Cargo.toml --lib notice`, expect PASS, then commit**

```bash
git add backend/src
git commit -m "Raise a corner notice for new free Steam games"
```

### Task 9: Page logic

**Files:** Create `frontend/app/pages/steam-helper/model.ts`, `api.ts`, `state.svelte.ts`; Modify `frontend/notice/Notice.svelte`; Test: `scripts/tests/steam-helper.test.mjs`.

**Interfaces:**
- Consumes: Task 6's commands.
- Produces, in `model.ts`, pure and free of Svelte:
  - `Filters`, `DEFAULT_FILTERS = { onlineCoop: true, singlePlayer: false, storyOnly: false }`, `validFilters(v: unknown): boolean`
  - `timeLeft(endsAt: number | null, now: number): string | null`, `isLive(endsAt: number | null, now: number): boolean`
  - `reviewLabel(percent: number, count: number): string`, `reviewTone(percent: number, count: number): "good" | "mid" | "none"`
  - `appendUnique<T extends { appid: number }>(list: T[], more: T[]): T[]`
  - `updatedAgo(updatedAt: number | null, now: number): string`
  - `createLatest(): { next(): number; isCurrent(id: number): boolean }`
- Produces, in `api.ts`: types mirroring Rust, and `steamHelperApi.{free, picks, row, more}`.
- Produces, in `state.svelte.ts`: singleton `steamHelperState` with `filters`, `free`, `picks`, `deals`, `freeToPlay`, `seeAll: null | "picks" | "deals" | "freeToPlay"`, `load(force?: boolean)`, `setFilter(name, value)`, `openSeeAll(section)`, `loadMore()`, `back()`.

- [ ] **Step 1: Write failing tests**

```js
import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_FILTERS, validFilters, timeLeft, isLive, reviewLabel, reviewTone, appendUnique, updatedAgo, createLatest } from "../../frontend/app/pages/steam-helper/model.ts";
test("online co-op starts on, the rest off", () => assert.deepEqual(DEFAULT_FILTERS, { onlineCoop: true, singlePlayer: false, storyOnly: false }));
test("a saved filter must have the right shape", () => { assert.ok(validFilters(DEFAULT_FILTERS)); assert.ok(!validFilters({ onlineCoop: "yes" })); assert.ok(!validFilters(null)); });
test("time left reads short", () => {
  assert.equal(timeLeft(1000 + 3600 * 23 + 30, 1000), "23h");
  assert.equal(timeLeft(1000 + 86400 * 5 + 60, 1000), "5d");
  assert.equal(timeLeft(1000 + 1800, 1000), "30m");
  assert.equal(timeLeft(null, 1000), null);
});
test("an offer past its end is no longer live", () => { assert.equal(isLive(999, 1000), false); assert.equal(isLive(1000, 1000), true); assert.equal(isLive(null, 1000), true); });
test("a game with no reviews is not called bad", () => { assert.equal(reviewLabel(0, 0), "No reviews yet"); assert.equal(reviewTone(0, 0), "none"); assert.equal(reviewLabel(95, 10), "95% positive"); assert.equal(reviewTone(95, 10), "good"); assert.equal(reviewTone(46, 10), "mid"); });
test("See all never repeats a game", () => assert.deepEqual(appendUnique([{ appid: 1 }], [{ appid: 1 }, { appid: 2 }]).map((g) => g.appid), [1, 2]));
test("updated time", () => { assert.equal(updatedAgo(880, 1000), "Updated 2 min ago"); assert.equal(updatedAgo(990, 1000), "Updated just now"); assert.equal(updatedAgo(null, 1000), ""); });
test("only the latest request counts", () => { const l = createLatest(); const a = l.next(); const b = l.next(); assert.equal(l.isCurrent(a), false); assert.equal(l.isCurrent(b), true); });
```

- [ ] **Step 2: Run, expect FAIL.** `node --test scripts/tests/steam-helper.test.mjs`

- [ ] **Step 3: Implement `model.ts`, `api.ts`, `state.svelte.ts`**

`reviewTone` is `mid` below 75. Filters persist with `readJson`/`writeJson` under `myle.steamHelper.filters`; do not add it to `SYNCED_KEYS`. `setFilter` reloads picks, deals and free to play, not free right now. Each section uses its own `createLatest()`, and an answer is dropped if not current. `loadMore` keeps `appendUnique` results and stops at `nextCursor: null`.

- [ ] **Step 4: Teach the notice page the new kind**

Add kind `freeOnSteam`, `offer: { game: string; endsAt: number | null; more: number } | null`, a 10000 ms stay, tone `ok`, the Lucide `Gift` icon, title `Free on Steam`, and text `<game> · ends in <timeLeft>` plus ` · +N more` when `more > 0`. Header and tooltip read `MYLE · Steam Game Helper` / `Open Steam Game Helper` for this kind only. Import `timeLeft` from `../app/pages/steam-helper/model`. Add browser-preview data for `?kind=freeOnSteam`.

- [ ] **Step 5: Run `node --test scripts/tests/steam-helper.test.mjs` and `npm run check`, expect PASS, then commit**

```bash
git add frontend/app/pages/steam-helper scripts/tests/steam-helper.test.mjs frontend/notice
git commit -m "Add Steam Game Helper page logic and notice text"
```

### Task 10: The page, the button and the docs

**Files:** Create `frontend/app/pages/steam-helper/SteamHelper.svelte`, `GameCard.svelte`, `OfferCard.svelte`, `frontend/lib/icons/IconSteamHelper.svelte`. Modify `frontend/app/pages/registry.ts`, `scripts/tests/sidebar.test.mjs`, `docs/DEVELOPMENT.md`.

**Interfaces:** Consumes `steamHelperState` and `model.ts`.

- [ ] **Step 1: Update `sidebar.test.mjs`, expect FAIL**

`all.length` becomes `13`; the owner group becomes `["project-backups", "github-releases", "steam-helper"]`; the sorted owner list gains `"steam-helper"`; the eager-import regex gains `SteamHelper`. Add:

```js
test("Steam Game Helper takes no number, so the owner's shortcuts stay", () => {
  assert.equal(shortcutOf(all, "steam-helper"), null);
  assert.equal(shortcutOf(all, "project-backups"), "Ctrl+9");
  assert.equal(shortcutOf(all, "github-releases"), "Ctrl+0");
});
```

Run: `node --test scripts/tests/sidebar.test.mjs`. Expected: FAIL.

- [ ] **Step 2: Add the registry entry and icon**

After `github-releases`, keeping `ownerOnly` and `numbered` on the first line because the test reads it:

```ts
{ id: "steam-helper", label: "Steam Game Helper", group: "owner", icon: IconSteamHelper, ownerOnly: true, numbered: false,
  load: () => import("./steam-helper/SteamHelper.svelte"),
  description: "Free games, this week's picks and the best deals on Steam." },
```

The icon follows `IconGithubReleases.svelte` (`IconCanvas`, `locked`).

- [ ] **Step 3: Build the page as in the spec's "The page"**

`PageHeader` title `Steam Game Helper`, subtitle `Deals, free games and what's worth playing this week.` Filter chips use `.chip`, with a divider before `Story only`; `Updated …` and a `Refresh` button sit on the right. Section headings are `Free right now` (small: `ends soon first, not affected by filters`), `This week's picks` (`changes every Monday`), `Deals` and `Free to play` (`best reviewed first`).

Card grids use `repeat(auto-fill, minmax(165px, 1fr))`, gap `10px`, and on the main page show a single row (`grid-auto-rows: 0; overflow: hidden`). Cover art has `aspect-ratio: 460 / 215`. Offer cards use a `112px` thumbnail in `repeat(auto-fit, minmax(250px, 1fr))`. Loading uses `.shimmer`. Empty and error blocks follow Game Saves' `.empty.surface`. When no offer is `freeToKeep`, show `No games are 100% off right now.` under the offers. Render offers through `isLive`, so one that expires while the page is open disappears. Clicking a card calls `openUrl(game.storeUrl)`, and cards are buttons, reachable by keyboard. `See all` shows no total and replaces the sections with one full grid and a Back button; picks show all 12, and deals and free to play load more on scroll.

- [ ] **Step 4: Update `docs/DEVELOPMENT.md`**

Add a `### Steam Game Helper` section before `## 6.`, name it alongside the other owner pages in `## 6.` with the `steam_helper_` prefix, add its folder to the map in `## 2.` and the data row in `## 7.`, change `37 node --test suites` to the real count, and mention `cargo test live_steam_still_answers -- --ignored` in `## 10.`

- [ ] **Step 5: Run everything**

Run: `npm run check`, `npm test`, `npm run lint`
Expected: all pass with no warnings.

- [ ] **Step 6: Check against live Steam**

Run: `cargo test --manifest-path backend/Cargo.toml --lib live_steam_still_answers -- --ignored`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend docs/DEVELOPMENT.md scripts/tests/sidebar.test.mjs
git commit -m "Add the Steam Game Helper page to the owner tools"
```

- [ ] **Step 8: The owner checks it in the app**

The owner signs in with Discord (never done on their behalf) and runs `npm run dev`. Check that the button appears, the four sections fill, filters change the lists, a game opens in the browser, and signing out hides the page. Open `/notice.html?kind=freeOnSteam` through `npx vite` to see the notice.
