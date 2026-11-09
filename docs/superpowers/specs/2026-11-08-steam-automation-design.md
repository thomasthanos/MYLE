# Steam Automation design

An owner-only MYLE page that runs card farming, playtime idling, achievement unlocking and selling duplicate cards on its own, using the Steam client already signed in on the PC. No subscription and no tiers, because none of it exists in MYLE to begin with.

The page was reviewed as a mockup on 2026-11-08. It follows the same all-on-one-page idea the owner picked for Steam Game Helper.

## Changed after the first version

The owner saw the four-panel page and asked for a full section instead, reviewed as a second mockup on 2026-11-09. Everything below still holds except where this section says otherwise.

**One button, its own menu.** `Steam Automation` stays a single sidebar button in Owner Tools. Inside it has its own menu: Games, Idling, Favorites, Card Farming, Achievement Unlocker, Automatic Idler, Inventory Manager. Built in MYLE's own look, with no ads, tiers or upsells.

- **Games:** the whole library with portrait art, playtime and achievement progress, search and sort. A game can be favorited or idled from here.
- **Idling:** what is running now, each with a stop button, plus Stop all and the Activity list.
- **Favorites:** the favorited games.
- **Card Farming:** tabs for games with drops, whitelisted and blacklisted. With a whitelist, only those games are farmed; a blacklisted game never is.
- **Achievement Unlocker and Automatic Idler:** tabs for all games and the queue, Add all to queue, sort.
- **Inventory Manager:** every item, with search, sort and filters (trading cards, foil, backgrounds, emoticons, booster packs, on sale, duplicates, locked, selected), a price lookup per item, List, List selected, List all, List duplicates, and Remove listings.

**Start and automatic are separate.** Each of Card Farming, Achievement Unlocker and Automatic Idler has a Start/Stop button for now, and a switch for starting by itself when MYLE opens. Inventory has only the automatic switch for selling duplicates, plus the manual buttons. Everything is off until the owner turns it on.

**Manual selling can sell the last copy,** because the owner chose those items. Bulk actions and Remove listings ask first. A locked item is never sold, by hand or automatically. Automatic selling still sells duplicates only.

**Library data needs no API key.** `store.steampowered.com/pointssummary/ajaxgetasyncconfig` returns a `webapi_token` for the signed-in session, accepted as `access_token` by `IPlayerService/GetOwnedGames` and `GetTopAchievementsForGames` (checked on the owner's account: playtime, totals and unlocked achievements all came back). Art and features come from `GetItems`. A portrait capsule comes from `assets.library_capsule`, falling back to the header image.

**Refunds cover the idler too,** and any doubt counts as a game that can still be refunded: unreadable dates, or a recent licence that matches no game the owner has. Names are compared by their letters and digits alone. Games idled by hand are the owner's choice and are not checked.

**Steam Family games are in the library,** marked as shared, because Steam's own client counts them. They drop no cards.

**Left out for now:** the Has badge filter, because it cannot be confirmed without a badge page showing a crafted badge; Claim for free games, which is not in the owner's list.

## Changed again after using it

The owner used the section and asked for fewer parts and something easier to understand, on 2026-11-09. Where this differs from the section above, this wins.

- **Four parts across the top,** not a second sidebar: Games, Card farming, Achievements, Inventory.
- **Idling and Favorites are ways of looking at Games.** Pressing Idle on a game runs it now. "Keep idling these when MYLE opens" brings the list back at launch, and only then does the refund check apply. The Automatic Idler is this.
- **What is running is always on show,** in one strip with Stop all, wherever the owner is.
- **Each feature opens with a panel** saying in plain words what is happening and what will happen, with its Start and "Start when MYLE opens".
- **Card farming has one choice per game:** farm it or skip it, as a tick under the game. The whitelist is gone.
- **The Inventory shows only what can be sold,** with filters as chips, and Remove listings under On sale.
- **Free games.** A game free to keep has Add to account, for the owner, using the Steam sign-in. A free weekend or free-to-play game opens Steam's own install window, for anyone.

## Where this fits

Steam Game Helper (`2026-11-08-steam-game-helper-design.md`) is the public, read-only half. This is the half that acts on an account, and only the owner sees it.

## Constraints

- **No code from Steam Game Idler.** `zevnda/steam-game-idler` is Elastic License 2.0, so nothing here copies it or its embedded key. It was read for how Steam behaves, and those facts are used. The Steam client's interface layout comes from Valve's published Steamworks headers.
- **Steam's rules.** Section 4C of the Steam Subscriber Agreement forbids automation, including faked playtime and rewards earned without real play, and Valve may restrict an account without notice. The owner has been told twice and has chosen to go ahead on their own account.
- **Owner only**, through the existing gate: `account/owner.rs`, `thomasthanos28@gmail.com` signed in with Discord.
- **Work directly on `main`**, no branches, no push.
- **No new dependencies.** `windows-sys`, `winreg`, `regex-lite`, `reqwest`, `serde_json`, `uuid` and the DPAPI helpers are already there.

## Goals

- Each of the four features runs by itself once switched on, starting when MYLE opens, with nothing to press.
- The owner never loses a refund, a real gaming session, or the last copy of a card to the automation.
- The page always says plainly what is happening and why something is waiting.

## Non-goals

- A Steam login of MYLE's own, or running without the Steam client (the owner chose the client).
- Several accounts, trading, crafting badges, or time-of-day schedules.
- Claiming free games. It was mentioned for this phase earlier, but the owner's list of four features leaves it out, so it stays a small later addition.
- Availability to anyone but the owner.

## Anti-goals

These would make it a failure even if it works:

- Storing or ever seeing the owner's Steam password or the phone's Steam Guard secret. MYLE never approves a Market listing.
- Idling on one account while Community requests go to another.
- Selling the last copy of a card.
- A helper process left running after MYLE exits.
- Cookies in a log, in the page, or in a file not sealed with DPAPI.

## The page

`Steam Automation`, in Owner Tools, `ownerOnly: true`, `numbered: false`, lazy-loaded, page id `steam-automation`.

Top: two status pills (Steam client, Steam Community) and `Pause everything`. A banner shows when things are paused and why. Below it, four panels in a 2-by-2 grid, each with an on/off switch and its live state, then an Activity list. English, sentence case, existing tokens and controls.

Every switch starts **off**, so nothing acts on the account until the owner turns it on. After that the choice is remembered and everything starts when MYLE opens.

## Shared rules

- **Connection.** Steam client: `HKCU\Software\Valve\Steam`, `SteamPath` and `ActiveProcess\ActiveUser`. If Steam is not running, the page says so and checks again every minute; MYLE does not start Steam. Steam Community: see Sign-in.
- **Same account.** The account in the Community cookies must match `ActiveUser`. If not, everything stops and the page says so.
- **Real games come first.** A Steam game running that MYLE did not start pauses all four features until it closes. Games started outside Steam are not detected.
- **Refunds.** A game bought in the last 14 days with under 2 hours played is skipped. Dates come from the Community licences page; if those cannot be read, every game under 2 hours is skipped for that round and the page says so.
- **Order.** Card farming runs alone, because idling other games beside a game earning drops slows the drops. The idler waits for it and runs when no cards are left or farming is off. The achievement unlocker also waits for card farming, then runs alongside the idler.
- **Stopping.** Losing owner access, closing MYLE, or `Pause everything` stops every helper. Helpers belong to a Windows job object that kills them if MYLE dies. Closing to the tray keeps things running.

## The four features

### Card farming

Reads `steamcommunity.com/profiles/<id>/badges/?l=english&p=<n>` with the owner's cookies. Each `.badge_row` has `.progress_info_bold` ("N card drops remaining"), `.badge_title`, `.badge_title_stats_playtime`, and a `gamecards/<appid>` link. One page every 3 seconds.

1. Games under 2 hours played are idled together, up to 32 at once, until they reach 2 hours, the point at which drops start.
2. Then one game at a time, fewest drops remaining first.
3. The badges page is read again every 20 to 30 minutes, at random. A game with no drops left is dropped, and a card that fell is written to Activity.

Skips refundable games. Done when no drops remain.

### Automatic idler

Idles the games the owner picks, up to 32, from the library list, until switched off. Waits for card farming.

### Achievement unlocker

Only games the owner picks. Games Steam marks as VAC (`feature_categoryids` contains 8) are not offered; other anti-cheats are not marked by Steam, so choosing those is up to the owner. Only games with achievements (category 22) are listed.

Unlocks one locked achievement at a time, at a random game among those picked, with a random 5 to 30 minutes between unlocks. An achievement Steam refuses is skipped and noted. Waits for card farming.

### Inventory manager

Duplicate trading cards only, one of each `market_hash_name` always kept. Foils count as different cards. Emoticons, backgrounds and anything not marketable are left alone.

- Every 6 hours reads `steamcommunity.com/inventory/<id>/753/6?l=english&count=2000`, following `last_assetid`.
- Price: `market/priceoverview/?country=GR&currency=3&appid=753&market_hash_name=…`, one every 4 seconds, listing at the lowest price minus 0,01€, never below the owner's minimum (default 0,05€). A card with no price is left alone.
- `POST market/sellitem/` with `sessionid`, `appid=753`, `contextid=6`, `assetid`, `amount=1` and `price`, the amount the seller receives. Steam adds a 5% and a 10% fee, each at least one cent, so work back from the buyer's price. Send a `Referer` of the owner's inventory URL.
- At most 20 listings per run.
- Each listing needs confirming in Steam Guard. The page counts those waiting and says so. MYLE never does this step.

## Sign-in to Steam Community

Card counts, licences, inventory and the Market need a session. A MYLE window opens Steam's own login page in a separate WebView2 profile, with no access to MYLE's commands. The owner signs in there, by QR or password, and MYLE never sees what is typed. It then reads cookies with Tauri's `cookies_for_url` for `steamcommunity.com` and `store.steampowered.com`: `steamLoginSecure` and `sessionid`.

Cookies are kept in memory and sealed with DPAPI on disk. When Steam rejects a session, MYLE reloads the site in the hidden profile to refresh it; if that fails, the Community pill says to sign in again and the features needing it wait.

## How a game is made to run

No file from Valve ships with MYLE. `steamclient64.dll` is already in `SteamPath`; `steam_api64.dll` is not, so it is not used.

Each running game is its own process, `MYLE.exe --steam-run=<appid>`, handled early in `main.rs` like the other helpers. It sets `SteamAppId`, loads `steamclient64.dll` from `SteamPath`, calls `CreateInterface` for `ISteamClient`, then `CreateSteamPipe` and `ConnectToGlobalUser`, and stays alive with no window until killed.

Achievements use a second helper mode that also pumps `Steam_BGetCallback` for `UserStatsReceived`, reads names and locked state through `ISteamUserStats`, calls `SetAchievement` and `StoreStats`, prints the result as JSON on stdout, and exits.

## Things only the real account can confirm

These cannot be tested without the owner's account, and the plan must check them first, on a game of the owner's choosing:

1. A `--steam-run` process really shows as in-game and earns playtime.
2. Which registry values tell a real game from MYLE's own: `RunningAppID` and `Apps\<id>\Running`.
3. Which domains the login writes cookies to.
4. The exact HTML of the badges and licences pages and the JSON of the inventory and Market, which have to be captured signed in. Parsers are written against those captures, with personal details removed before they become fixtures.

The Steam client's method order is fixed by the interface version asked for. Use a version string that the installed client still serves, and fail with a plain message if `CreateInterface` returns nothing.

## Library list

Owned apps come from `store.steampowered.com/dynamicstore/userdata/` with the session, and names and categories from `GetItems`, reusing `steam_helper::store`'s paced client. Saved for a day.

## Saved data

Under `storage::local_dir()/steam-automation/`, atomic writes: `settings.json` (switches, picked games, minimum price), `activity.json` (last 200 entries), `library.json`, `session.bin` (DPAPI). The WebView2 profile is `webview-steam` beside them.

## Testing

Pure logic is tested in Rust with no Steam: ordering and pause decisions, phase selection, refund rule, duplicates and keep-one, price and fee maths both ways, cookie-account match, badges/inventory/Market parsing from captured fixtures, settings round trip, and corrupt files. Every command is covered by the owner gate. Scheduling is driven by a fake clock and a fake runner, never real timers or real Steam. `npm run check`, `npm test` and `npm run lint` must pass.

## Decided by the owner

Steam client on the PC rather than a separate login; all four features automatic with no subscription; owner only; sells duplicates only; achievements only for games the owner picks; the page layout; going ahead knowing Steam's rules.

## Calls made while designing, for the owner to check

- Every switch starts off.
- Card farming runs alone and the idler waits for it, unlike the mockup, which showed both at once, because idling other games beside a game earning drops slows the drops.
- Real games pause everything, and purchases in the last 14 days are skipped.
- VAC games are not offered for achievements.
- Unlock delay of 5 to 30 minutes, minimum price 0,05€, and at most 20 listings per run.
- If purchase dates cannot be read, games under 2 hours are skipped.
- Steam is never started by MYLE.
- Foil cards are treated as separate cards.

## Left to the builder

File layout inside `backend/src/steam_automation/` and `frontend/app/pages/steam-automation/`, the command surface, how status reaches the page, the look of the pickers and the settings for each panel.
