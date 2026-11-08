# Steam Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an owner-only page that runs card farming, playtime idling, achievement unlocking and selling duplicate cards by itself, through the Steam client signed in on the PC.

**Architecture:** Each running game is a `MYLE.exe --steam-run=<appid>` child process that loads the installed `steamclient64.dll`. A pure decision function says which games should run given the world's state; an engine applies that through a runner and asks Steam Community with a sealed session. A lazy Svelte page shows status and holds the switches.

**Tech Stack:** Rust 2024, Tauri 2, windows-sys, winreg, reqwest, regex-lite, serde; Svelte 5, TypeScript; `cargo test`, `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-08-steam-automation-design.md`

## Global Constraints

- Work on `main`. No branches, no worktrees, no push, no tag, no version bump.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- No code from `zevnda/steam-game-idler` and not its key. No new Cargo or npm dependencies.
- Never store or read the Steam password or Steam Guard secret. Cookies only in memory and DPAPI-sealed (`account::vault`), never logged, never sent to the page.
- Every command is named `steam_automation_*` and gated.
- Interface strings: `SteamClient017`, `STEAMUSERSTATS_INTERFACE_VERSION012` (both present in the installed `steamclient64.dll`; 012 still has `RequestCurrentStats`).
- Constants: 32 games at once; refund window 14 days and 120 minutes; farming threshold 120 minutes; drop re-check 20 to 30 minutes; unlock delay 5 to 30 minutes; inventory every 6 hours; 20 listings per run; Community pages 3 s apart; price lookups 4 s apart; default minimum 5 cents; activity 200 entries; library saved 1 day; Steam re-check every 60 s.
- Paths under `storage::local_dir()/steam-automation/`; WebView2 profile `webview-steam`.
- Tests never touch real Steam, real timers or real processes. Real behaviour is checked only in Task 4, by the owner.

## Review Focus

1. MYLE exits or crashes while games run: no helper survives. (Task 2)
2. The owner starts a real game mid-run, then closes it: everything stops, then resumes. (Task 9)
3. The session expires mid-run: Community features wait with a clear status and no listing is attempted. (Tasks 5, 9)
4. Steam restarts or another account signs in: nothing runs on the wrong account. (Tasks 2, 9)
5. A sell run fails partway, or Steam refuses or has no price: the same asset is never listed twice and the last copy is never sold. (Task 8)

---

### Task 1: The helper process

**Files:** Create `backend/src/steam_automation/{mod.rs,client.rs,helper.rs}`. Modify `backend/src/lib.rs`, `backend/src/main.rs`.

**Interfaces:**
- Produces in `helper.rs`: `enum HelperMode { Run(u32), Achievements(u32), Unlock { appid: u32, name: String } }`, `fn mode_from_args(args: &[String]) -> Option<HelperMode>`, `pub fn run_helper_from_args() -> Option<i32>`.
- Produces in `client.rs`: `struct SteamClient`, `fn open(steam_path: &Path, appid: u32) -> Result<SteamClient, String>`, `fn user_stats(&self) -> Result<UserStats, String>`.
- Produces in `lib.rs`: `pub fn run_steam_helper() -> Option<i32>`, called in `main.rs` next to the other helpers.

- [ ] **Step 1: Commit the Steam Game Helper fix that is already in the working tree**

```bash
git add README.md backend docs frontend scripts
git commit -m "Keep Steam's search limit from breaking the Helper and open it to everyone"
```

- [ ] **Step 2: Write failing tests in `helper.rs`**

```rust
fn args(list: &[&str]) -> Vec<String> { list.iter().map(|s| s.to_string()).collect() }
#[test] fn a_normal_launch_is_not_a_helper() { assert_eq!(mode_from_args(&args(&["MYLE.exe"])), None); }
#[test] fn run_takes_an_app_id() { assert_eq!(mode_from_args(&args(&["x", "--steam-run=620"])), Some(HelperMode::Run(620))); }
#[test] fn a_bad_app_id_is_not_a_helper() { assert_eq!(mode_from_args(&args(&["x", "--steam-run=abc"])), None); assert_eq!(mode_from_args(&args(&["x", "--steam-run=0"])), None); }
#[test] fn unlock_takes_a_name() { assert_eq!(mode_from_args(&args(&["x", "--steam-unlock=620", "--steam-achievement=ACH_WIN"])), Some(HelperMode::Unlock { appid: 620, name: "ACH_WIN".into() })); }
#[test] fn achievements_takes_an_app_id() { assert_eq!(mode_from_args(&args(&["x", "--steam-achievements=620"])), Some(HelperMode::Achievements(620))); }
```

- [ ] **Step 3: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation::helper`

- [ ] **Step 4: Implement**

`client.rs`, `unsafe` FFI with `windows-sys` `LoadLibraryExW` and `GetProcAddress`. Set `SteamAppId` first. Load `steamclient64.dll` from `steam_path`, with that folder as the DLL search directory. Exports: `CreateInterface(version, *mut i32) -> *mut c_void`, `Steam_BGetCallback(pipe, *mut CallbackMsg, *mut i32) -> bool`, `Steam_FreeLastCallback(pipe) -> bool`. `CallbackMsg` is `#[repr(C)] { user: i32, callback: i32, param: *mut u8, size: i32 }`.

`ISteamClient` vtable indices: `CreateSteamPipe` 0, `BReleaseSteamPipe` 1, `ConnectToGlobalUser` 2, `ReleaseUser` 4, `GetISteamUserStats` 13. `ISteamUserStats` v012: `RequestCurrentStats` 0, `GetAchievement` 6, `SetAchievement` 7, `StoreStats` 10, `GetNumAchievements` 14, `GetAchievementName` 15. `UserStatsReceived` callback id is 1101, with `{ game_id: u64, result: i32 }`. A null interface returns `Err("This Steam version is not supported.")`.

`--steam-run` reads `SteamPath` from `HKCU\Software\Valve\Steam`, opens the client and sleeps forever; exit code 1 on `Err`.

- [ ] **Step 5: Run tests, expect PASS, then commit**

```bash
git add backend/src
git commit -m "Run a Steam game as a helper process through the installed client"
```

### Task 2: Reading Steam and running games

**Files:** Create `backend/src/steam_automation/{steam_state.rs,runner.rs}`.

**Interfaces:**
- Produces in `steam_state.rs`: `struct SteamState { path: PathBuf, active_user: u32, running: Vec<u32> }`, `fn read() -> Option<SteamState>` (`None` when no user is active), `fn real_game(running: &[u32], ours: &BTreeSet<u32>) -> Option<u32>`, `fn same_account(active_user: u32, steam_id64: u64) -> bool`.
- Produces in `runner.rs`: `trait GameRunner: Send + Sync { fn apply(&self, wanted: &BTreeSet<u32>) -> Result<(), String>; fn running(&self) -> BTreeSet<u32>; fn stop_all(&self); }`, `struct ProcessRunner`, `#[cfg(test)] struct FakeRunner`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn our_own_games_are_not_a_real_game() { assert_eq!(real_game(&[620], &BTreeSet::from([620])), None); }
#[test] fn a_game_we_did_not_start_is_real() { assert_eq!(real_game(&[620, 730], &BTreeSet::from([620])), Some(730)); }
#[test] fn nothing_running_is_no_game() { assert_eq!(real_game(&[], &BTreeSet::new()), None); }
#[test] fn accounts_match_on_the_low_bits() { assert!(same_account(12345, 76561197960265728 + 12345)); assert!(!same_account(12345, 76561197960265728 + 99)); }
// runner.rs, through a test process spawner
#[test] fn applying_starts_what_is_missing_and_stops_the_rest() { /* running {1,2}, wanted {2,3}: 1 killed, 3 spawned, 2 untouched */ }
#[test] fn no_more_than_thirty_two_games_run() { /* wanted 40: 32 spawned, in ascending order */ }
#[test] fn a_helper_that_dies_is_noticed() { /* a spawned child reports exit: running() no longer lists it */ }
```

Make `ProcessRunner` generic over a small `Spawner` trait so these tests use fake children.

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation`

- [ ] **Step 3: Implement**

`read()` uses `winreg`: `SteamPath`, `ActiveProcess\ActiveUser`, and every `Apps\<id>\Running == 1`. The real spawner launches `current_exe()` with `--steam-run=<id>`, `CREATE_NO_WINDOW`, and assigns each child to the job object in `apps/process.rs` (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), widening that type's visibility if needed.

- [ ] **Step 4: Run tests, expect PASS, then commit**

```bash
git add backend/src
git commit -m "Read Steam's state and supervise running games"
```

### Task 3: Sign-in, gate and a first page

**Files:** Create `backend/src/steam_automation/{session.rs,commands.rs}`, `frontend/app/pages/steam-automation/{api.ts,SteamAutomation.svelte}`, `frontend/lib/icons/IconSteamAutomation.svelte`. Modify `backend/src/account/owner.rs`, `backend/src/account/mod.rs`, `backend/src/lib.rs`, `frontend/app/pages/registry.ts`, `scripts/tests/sidebar.test.mjs`.

**Interfaces:**
- Produces in `session.rs`: `struct Session { steam_login_secure: String, session_id: String, steam_id: u64 }`, `fn from_cookies(pairs: &[(String, String)]) -> Option<Session>`, `fn cookie_header(&self) -> String`, `fn save(&self, path: &Path) -> Result<(), String>`, `fn load(path: &Path) -> Option<Session>`.
- Produces in `commands.rs`: `SteamAutomationState` (managed; has `stop_all()`), `steam_automation_status() -> Status`, `steam_automation_sign_in()`, `steam_automation_capture() -> String` (the folder written).

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn cookies_give_a_session() {
    let pairs = vec![("steamLoginSecure".into(), "76561198000000001%7C%7Ctoken".into()), ("sessionid".into(), "abc".into()), ("other".into(), "x".into())];
    let s = from_cookies(&pairs).unwrap();
    assert_eq!((s.steam_id, s.session_id.as_str()), (76561198000000001, "abc"));
    assert_eq!(s.cookie_header(), "steamLoginSecure=76561198000000001%7C%7Ctoken; sessionid=abc");
}
#[test] fn a_missing_cookie_is_no_session() { /* without sessionid: None; without steamLoginSecure: None */ }
#[test] fn a_cookie_without_an_id_is_no_session() { /* "garbage": None */ }
#[test] fn a_session_is_not_stored_in_plain_text() { /* save, then the file bytes do not contain the token; load gives it back */ }
```

In `owner.rs`, extend `only_the_owner_only_pages_commands_are_gated` with `assert!(is_owner_only_command("steam_automation_status"))`, and `every_command_of_the_owner_only_pages_is_gated` with `line.starts_with("steam_automation::commands::")`. In `sidebar.test.mjs`: `all.length` 14; owner group gains `"steam-automation"`, in both lists; a test that its shortcut is null.

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib -- steam_automation account::owner` and `node --test scripts/tests/sidebar.test.mjs`

- [ ] **Step 3: Implement**

Add `"steam_automation_"` to `OWNER_ONLY_COMMANDS`. `stop_owner_only_work` also calls `SteamAutomationState::stop_all`. Register the page: `id: "steam-automation"`, label `Steam Automation`, group `owner`, `ownerOnly: true, numbered: false` on the first line, lazy `load`.

`steam_automation_sign_in` opens label `steam-login` at `https://steamcommunity.com/login/home/?goto=`, `data_directory(local/webview-steam)`, no capability file lists it. It polls `cookies_for_url` for `https://steamcommunity.com` and `https://store.steampowered.com` until `from_cookies` succeeds, seals the session and closes the window.

`steam_automation_capture` is a temporary owner tool: it saves the raw badges page, licences page, inventory JSON, one `priceoverview` answer and `dynamicstore/userdata` into `steam-automation/captures/`. The first page has status pills, `Sign in to Steam` and `Capture`.

- [ ] **Step 4: Run tests and `npm run check`, expect PASS, then commit**

```bash
git add backend frontend scripts
git commit -m "Sign in to Steam Community and add the Steam Automation page"
```

### Task 4: Checkpoint with the owner

Nothing else starts until this is done. It needs the owner, and it acts on their account.

- [ ] **Step 1: Ask the owner to pick a game they own**, ideally a single-player one.
- [ ] **Step 2: Run it for 90 seconds.** `cargo run --manifest-path backend/Cargo.toml -- --steam-run=<appid>`. Read `Apps\<id>\Running` and `RunningAppID` during and after, and ask the owner whether Steam shows them in-game. Expected: Running is 1 and the friends list shows the game. If not, stop and rethink the helper: every later task depends on it.
- [ ] **Step 3: Check `real_game` against what the registry showed** and correct it if the values differ from Task 2's assumption.
- [ ] **Step 4: The owner runs `npm run dev`, signs in on Steam's page, and presses Capture.** Expected: all five files exist and the session's `steam_id` matches `ActiveUser`.
- [ ] **Step 5: Turn the captures into fixtures** under `backend/src/steam_automation/fixtures/`. Remove names, avatars, Steam IDs, session values and wallet figures. Delete the captures. Remove the `Capture` button and command.
- [ ] **Step 6: Commit the fixtures**

```bash
git add backend frontend
git commit -m "Add Steam Community fixtures and remove the capture tool"
```

### Task 5: Steam Community and badges

**Files:** Create `backend/src/steam_automation/{community.rs,badges.rs}`.

**Interfaces:**
- Consumes `Session`, fixtures.
- Produces in `community.rs`: `trait Community: Send + Sync { fn get(&self, url: &str) -> impl Future<Output = Result<String, CommunityError>> + Send; fn post(&self, url: &str, form: &[(&str, String)], referer: &str) -> impl Future<Output = Result<String, CommunityError>> + Send; }`, `enum CommunityError { SignedOut, Limited, Other(String) }`, `struct HttpCommunity`, `#[cfg(test)] struct FakeCommunity`.
- Produces in `badges.rs`: `struct BadgeGame { appid: u32, name: String, drops: u32, minutes_played: u32 }`, `fn parse_badges(html: &str) -> Result<(Vec<BadgeGame>, u32 /* last page */), String>`, `fn parse_licences(html: &str) -> Vec<(String, u64)>`, `fn is_refundable(purchased_at: Option<u64>, minutes_played: u32, now: u64) -> bool`, `async fn read_badges<C: Community>(api: &C, steam_id: u64) -> Result<Vec<BadgeGame>, CommunityError>`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn the_fixture_gives_games_with_drops() { /* parse_badges(BADGES): known appid has its drops and minutes, as hand-read from the fixture */ }
#[test] fn a_game_with_no_drops_is_left_out() { /* "No card drops remaining" row absent */ }
#[test] fn a_page_that_is_not_badges_is_an_error() { assert!(parse_badges("<html></html>").is_err()); }
#[test] fn inside_the_window_and_under_two_hours_is_refundable() { assert!(is_refundable(Some(1000), 119, 1000 + 13 * 86400)); }
#[test] fn two_hours_played_is_not_refundable() { assert!(!is_refundable(Some(1000), 120, 1000 + 86400)); }
#[test] fn past_fourteen_days_is_not_refundable() { assert!(!is_refundable(Some(1000), 10, 1000 + 15 * 86400)); }
#[test] fn an_unknown_date_counts_as_refundable_under_two_hours() { assert!(is_refundable(None, 119, 5)); assert!(!is_refundable(None, 120, 5)); }
#[tokio::test] async fn every_badge_page_is_read() { /* last page 3: 3 calls, games combined */ }
#[test] fn a_redirect_to_login_is_signed_out() { /* classify a login page body or 401 as SignedOut; 429 as Limited */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation::badges`

- [ ] **Step 3: Implement** with `regex-lite`, splitting on `badge_row`. `HttpCommunity` sends `Cookie` from the session, 3 s apart, no redirects followed to `/login`.

- [ ] **Step 4: Run tests, expect PASS, then commit** `Read card drops and purchase dates from Steam Community`, with `git add backend/src`.

### Task 6: Deciding what runs

**Files:** Create `backend/src/steam_automation/decide.rs`.

**Interfaces:**
- Consumes `BadgeGame`, `is_refundable`.
- Produces: `enum FarmStep { Accumulate(Vec<u32>), Solo(u32), Done }`, `fn farm_step(games: &[BadgeGame], purchases: &HashMap<u32, u64>, now: u64) -> FarmStep`; `struct World { steam_running: bool, account_ok: bool, real_game: Option<u32>, user_paused: bool, farm_on: bool, farm: FarmStep, idle_on: bool, idle_games: Vec<u32>, session_ok: bool }`; `enum Waiting { NoSteam, OtherAccount, RealGame(u32), Paused }`; `struct Decision { games: BTreeSet<u32>, waiting: Option<Waiting>, achievements_may_run: bool, inventory_may_run: bool }`; `fn decide(world: &World) -> Decision`.

- [ ] **Step 1: Write failing tests**

```rust
// farm_step
#[test] fn games_under_two_hours_build_up_together() { /* A 30 min, B 60 min, C 500 min: Accumulate([A, B]) */ }
#[test] fn then_one_at_a_time_fewest_drops_first() { /* B 3 drops, C 1 drop, both over 120 min: Solo(C) */ }
#[test] fn refundable_games_are_skipped() { /* bought yesterday, 30 min: left out */ }
#[test] fn no_more_than_thirty_two_build_up() { /* 40 games: 32, nearest the threshold first */ }
#[test] fn nothing_left_is_done() { /* [] => Done */ }
// decide
#[test] fn farming_runs_alone() { /* farm Solo(5), idle on with [7]: games {5}, achievements false */ }
#[test] fn the_idler_runs_once_farming_is_done() { /* Done, idle [7,8]: games {7,8}, achievements true */ }
#[test] fn farming_off_frees_the_idler() { /* farm_on false, farm Solo(5), idle [7]: games {7} */ }
#[test] fn a_real_game_stops_everything() { /* real_game Some(9): games empty, waiting RealGame(9), both flags false */ }
#[test] fn no_steam_stops_everything() { /* waiting NoSteam */ }
#[test] fn the_wrong_account_stops_everything() { /* waiting OtherAccount */ }
#[test] fn pause_stops_everything() { /* waiting Paused */ }
#[test] fn no_session_holds_back_only_what_needs_one() { /* session_ok false, farm Done, idle [7]: games {7}, inventory false */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation::decide`

- [ ] **Step 3: Implement. Step 4: Run, expect PASS, then commit** `Decide which Steam games should run`, with `git add backend/src`.

### Task 7: Achievements

**Files:** Create `backend/src/steam_automation/achievements.rs`. Modify `helper.rs`.

**Interfaces:**
- Consumes `SteamClient`, `HelperMode`.
- Produces: `struct AchievementList { names: Vec<String>, locked: Vec<String> }` (`Serialize + Deserialize`); `trait AchievementTool: Send + Sync { fn list(&self, appid: u32) -> impl Future<Output = Result<AchievementList, String>> + Send; fn unlock(&self, appid: u32, name: &str) -> impl Future<Output = Result<(), String>> + Send; }`; `struct UnlockQueue` with `fn next(&mut self, picked: &[u32], lists: &HashMap<u32, AchievementList>, random: u64) -> Option<(u32, String)>` and `fn mark_refused(&mut self, appid: u32, name: &str)`; `fn next_delay_secs(random: u64) -> u64`; `fn eligible(feature_categories: &[u32]) -> bool`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn the_delay_stays_between_five_and_thirty_minutes() { for r in [0, 1, 999_999, u64::MAX] { let d = next_delay_secs(r); assert!((300..=1800).contains(&d)); } }
#[test] fn only_locked_achievements_of_picked_games_come_up() { /* game 1 locked ["A"], game 2 not picked: (1, "A") */ }
#[test] fn a_finished_game_is_passed_over() { /* no locked left: None */ }
#[test] fn a_refused_achievement_is_not_tried_again() { /* mark_refused(1, "A"): next gives another or None */ }
#[test] fn vac_games_are_not_eligible() { assert!(!eligible(&[22, 8])); }
#[test] fn games_without_achievements_are_not_eligible() { assert!(!eligible(&[29])); assert!(eligible(&[22, 29])); }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation::achievements`

- [ ] **Step 3: Implement.** The helper modes: `--steam-achievements=<id>` calls `RequestCurrentStats`, pumps `Steam_BGetCallback` for 1101 for up to 15 s, prints `AchievementList` as JSON; `--steam-unlock=<id> --steam-achievement=<name>` also calls `SetAchievement` and `StoreStats`. The real `AchievementTool` runs these and reads stdout.

- [ ] **Step 4: Run tests, expect PASS, then commit** `Unlock chosen achievements with random delays`, with `git add backend/src`.

### Task 8: Inventory

**Files:** Create `backend/src/steam_automation/inventory.rs`.

**Interfaces:**
- Consumes `Community`, `CommunityError`, fixtures.
- Produces: `struct Card { asset_id: String, hash_name: String }`; `fn parse_inventory(json: &str) -> Result<(Vec<Card>, Option<String>), String>`; `fn duplicates(cards: &[Card]) -> Vec<Card>`; `fn parse_price(text: &str) -> Option<u32>`; `fn list_price(lowest: u32, minimum: u32) -> u32`; `fn seller_gets(buyer_cents: u32) -> u32`; `async fn sell_duplicates<C: Community>(api: &C, session: &Session, minimum: u32, already_listed: &mut HashSet<String>) -> Result<SellReport, CommunityError>`; `struct SellReport { listed: u32, cents: u32, waiting_confirmation: u32 }`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn only_marketable_trading_cards_count() { /* fixture: emoticon, background and a non-marketable card left out (item_class_2 only) */ }
#[test] fn the_last_copy_is_always_kept() { /* A x3, B x1: 2 of A, none of B */ }
#[test] fn foils_are_different_cards() { /* "Card" x1 and "Card (Foil)" x1: nothing to sell */ }
#[test] fn prices_read_in_euros() { assert_eq!(parse_price("0,31€"), Some(31)); assert_eq!(parse_price("1,84€"), Some(184)); assert_eq!(parse_price("2,--€"), Some(200)); assert_eq!(parse_price(""), None); }
#[test] fn the_price_is_a_cent_under_the_market() { assert_eq!(list_price(31, 5), 30); }
#[test] fn the_price_never_goes_below_the_minimum() { assert_eq!(list_price(5, 5), 5); assert_eq!(list_price(3, 5), 5); }
#[test] fn fees_are_taken_off_the_buyers_price() { assert_eq!(seller_gets(3), 1); assert_eq!(seller_gets(5), 3); assert_eq!(seller_gets(31), 28); assert_eq!(seller_gets(100), 88); }
#[tokio::test] async fn a_run_stops_at_twenty_listings() { /* 30 duplicates: 20 posts */ }
#[tokio::test] async fn a_card_with_no_price_is_left_alone() { /* no lowest_price: no post */ }
#[tokio::test] async fn an_asset_is_never_listed_twice() { /* run twice, first run marked it: second posts none for it */ }
#[tokio::test] async fn being_signed_out_stops_the_run() { /* Err(SignedOut) on the 2nd post: returned, 1 post counted */ }
#[tokio::test] async fn the_form_sends_what_the_seller_gets() { /* lowest 31: form price "27", sessionid, appid 753, contextid 6, amount 1, referer the inventory URL */ }
#[tokio::test] async fn waiting_listings_are_counted() { /* response needs_mobile_confirmation true: waiting_confirmation 1 */ }
```

In `the_form_sends_what_the_seller_gets`, the list price is 30, so the seller gets `seller_gets(30)`, which is 27 by hand: 27 + 1 + 2 = 30.

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation::inventory`

- [ ] **Step 3: Implement** from the spec's URLs, following `last_assetid`, with price lookups 4 s apart and one lookup per `hash_name`.

- [ ] **Step 4: Run tests, expect PASS, then commit** `Sell duplicate cards on the Market`, with `git add backend/src`.

### Task 9: The engine

**Files:** Create `backend/src/steam_automation/{settings.rs,library.rs,engine.rs}`. Modify `commands.rs`, `lib.rs`.

**Interfaces:**
- Consumes Tasks 2 to 8.
- Produces in `settings.rs`: `struct Settings { farm_on, idle_on, achievements_on, inventory_on: bool, idle_games: Vec<u32>, achievement_games: Vec<u32>, minimum_cents: u32 }` (all switches default false, `minimum_cents` default 5), `fn load(path) -> Settings`, `fn save(path, &Settings)`, `struct Activity` keeping 200 entries.
- Produces in `engine.rs`: `struct Engine<R, C, A>` with `async fn tick(&mut self, input: TickInput, now: u64) -> Status`; `TickInput { steam: Option<SteamState>, session: Option<Session>, settings: Settings, user_paused: bool, is_owner: bool }`; `Status` (`Serialize`, camelCase) with what the page shows.
- Produces in `commands.rs`: `steam_automation_status`, `steam_automation_set_settings(settings)`, `steam_automation_pause(paused)`, `steam_automation_library()`.

- [ ] **Step 1: Write failing tests** with `FakeRunner`, `FakeCommunity`, a fake `AchievementTool`, and `now` passed in:

```rust
#[tokio::test] async fn everything_off_touches_nothing() { /* default settings: no runner calls, no Community calls */ }
#[tokio::test] async fn not_the_owner_stops_everything() { /* is_owner false: runner.stop_all */ }
#[tokio::test] async fn a_real_game_stops_and_closing_it_resumes() { /* running includes 9: wanted empty; next tick without 9: farm game back */ }
#[tokio::test] async fn badges_are_read_again_after_the_wait_not_before() { /* 19 min: no call; 31 min: one call */ }
#[tokio::test] async fn a_drop_is_written_to_activity() { /* drops 3 then 2: entry */ }
#[tokio::test] async fn a_lost_session_waits_and_says_so() { /* SignedOut: status communityOk false, no further posts this tick */ }
#[tokio::test] async fn achievements_wait_for_their_delay() { /* unlock, next tick before delay: no unlock; after: one */ }
#[tokio::test] async fn the_inventory_runs_every_six_hours() { /* 5h59: no; 6h: yes */ }
#[tokio::test] async fn the_wrong_account_runs_nothing() { /* session steam_id not matching: wanted empty, waiting OtherAccount */ }
#[test] fn corrupt_settings_mean_everything_off() { /* garbage file: defaults */ }
#[test] fn activity_keeps_the_last_two_hundred() { /* 205 pushed: 200, newest kept */ }
```

- [ ] **Step 2: Run, expect FAIL.** `cargo test --manifest-path backend/Cargo.toml --lib steam_automation::engine`

- [ ] **Step 3: Implement.** A task spawned in `.setup` ticks every 60 s and re-reads `AccountState::is_owner` each time, and on `steam_automation_set_settings` or pause. On `SignedOut`, reload Steam Community once in the hidden `webview-steam` profile to refresh the cookies and seal them again; if that fails, `communityOk` goes false until the owner signs in. When purchase dates cannot be read, `Status` says so and the page shows that games under 2 hours were skipped. The library combines `dynamicstore/userdata` and `steam_helper::store::details`, saved for a day. `steam_automation_library` marks each game's eligibility for achievements with `eligible`.

- [ ] **Step 4: Run tests and `npm run lint`, expect PASS, then commit** `Run Steam Automation on its own`, with `git add backend/src`.

### Task 10: The full page

**Files:** Modify `frontend/app/pages/steam-automation/{api.ts,SteamAutomation.svelte}`. Create `state.svelte.ts`, `model.ts`, `GamePicker.svelte`. Test: `scripts/tests/steam-automation.test.mjs`. Modify `docs/DEVELOPMENT.md`.

**Interfaces:** Consumes Task 9's commands and `Status`.

- [ ] **Step 1: Write failing tests for `model.ts`**

```js
test("the banner says why things are waiting", () => {
  assert.equal(waitingText({ kind: "realGame" }), "Paused while you play. Everything picks up again when you close your game.");
  assert.equal(waitingText({ kind: "noSteam" }), "Steam is not running.");
  assert.equal(waitingText({ kind: "otherAccount" }), "Steam and Steam Community are signed in to different accounts, so nothing is running.");
  assert.equal(waitingText(null), "");
});
test("cents read as euros", () => { assert.equal(euros(184), "1,84€"); assert.equal(euros(5), "0,05€"); });
test("a minimum price must be sensible", () => { assert.equal(validMinimum(5), true); assert.equal(validMinimum(0), false); assert.equal(validMinimum(1.5), false); });
test("the picker hides games that cannot be chosen", () => { /* eligible false removed from the achievements picker, kept in the idler's */ });
```

- [ ] **Step 2: Run, expect FAIL.** `node --test scripts/tests/steam-automation.test.mjs`

- [ ] **Step 3: Build the page as in the spec's "The page"**, matching the reviewed mockup, with the idler showing `Waiting for card farming to finish` while farming runs. Poll `steam_automation_status` every 5 s while the page is open.

- [ ] **Step 4: Add `### Steam Automation` to `docs/DEVELOPMENT.md`**, name it in `## 6.`, in the map in `## 2.`, and the data row in `## 7.`

- [ ] **Step 5: Run `npm run check`, `npm test`, `npm run lint`. Expected: all pass.**

- [ ] **Step 6: Commit** `Finish the Steam Automation page`, with `git add frontend docs scripts`.

- [ ] **Step 7: The owner tries each switch** on a small set of games, watching Activity.
