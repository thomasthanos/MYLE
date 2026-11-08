# Steam Game Helper design

A new owner-only page in MYLE that answers one question without signing in to anything: what is free on Steam right now, what is worth playing this week, and what is on sale. Online co-op and story games come first; single-player is there when the owner wants it.

The layout was picked from three mockups (option B, "all on one page") and shown with live Steam data on 2026-11-08.

## Where this fits

This is the first of five pieces. Each of the others gets its own design later.

| # | Piece | Needs Steam sign-in |
|---|---|---|
| 1 | **Steam Game Helper** (this document) | No |
| 2 | Idler and auto-idle | Yes |
| 3 | Card farming | Yes |
| 4 | Achievement unlocker | Yes |
| 5 | Inventory manager | Yes |

Two constraints apply to all five:

- **No code from Steam Game Idler.** `zevnda/steam-game-idler` is Elastic License 2.0, which forbids changing, disabling or circumventing its license-key functionality, and its terms could not travel inside proprietary MYLE. Everything here is written from scratch. Its embedded Steam Web API key is not used. Public Steam URLs and filters are facts and may be used freely.
- **Steam's rules affect pieces 2 to 5.** Section 4C of the Steam Subscriber Agreement forbids automation, including faked playtime and rewards earned without real play, and Valve may restrict an account without notice. The owner has been told. This piece only reads the public store and never touches an account.

## Goals

- One page, opened from a new sidebar button, showing everything at a glance.
- Find every free weekend and 100%-off game Steam can tell us about, and tell the owner when a new one appears.
- A weekly list of well-reviewed games, weighted toward online co-op with a story.
- Every discount reachable, not just the first screen.

## Non-goals

- Claiming free games. That needs a Steam session, so it arrives with piece 2.
- Anything based on the owner's library, wishlist or friends. Picks are chosen from reviews alone, by the owner's decision.
- Giveaways from outside Steam (for example GamerPower, which links to third-party sites).
- Availability to anyone but the owner, and any phone version.

## Anti-goals

These would make it a failure even if the page works:

- Asking for a Steam password, cookie or API key.
- Any network activity, background task or visible trace for a user who is not the owner.
- Showing a stale or wrong "free" badge as if it were current. A section that could not refresh must say so.

## Access

Uses the existing owner gate unchanged (`backend/src/account/owner.rs`, `OWNER_EMAIL` is already `thomasthanos28@gmail.com`, and the owner must be signed in with Discord under that address).

- Add `"steam_helper_"` to `OWNER_ONLY_COMMANDS`. Register every command in `lib.rs` as `steam_helper::<name>` on its own line, and extend the test `every_command_of_the_owner_only_pages_is_gated` to include that prefix.
- Add to `frontend/app/pages/registry.ts`: id `steam-helper`, label `Steam Game Helper`, group `owner`, `ownerOnly: true`, lazy `load`, a new `lib/icons` icon, and `numbered: false` because Ctrl+9 and Ctrl+0 are already the two existing owner pages.
- Update `scripts/tests/sidebar.test.mjs`, which lists the owner pages exactly.

## The page

Built as the mockup, in English like the rest of MYLE, using existing tokens and `PageHeader`.

**Filters (top of the page):** `Online co-op` (on at first), `Single-player` (off), a divider, `Story only` (off). On the right, "Updated N min ago" and `Refresh`. The owner's last choice is remembered.

**Sections, top to bottom:**

1. **Free right now.** Free weekends and 100%-off games, soonest to end first. Not affected by the filters, so nothing free is hidden. Each shows a badge (`Free weekend`, `Free to keep`, or Steam's own trial label such as `DMZ free trial`) and the time left. Low review scores show in amber but are not hidden. If there are no 100%-off games, one quiet line says so. If nothing at all is free, the section shows an empty state rather than disappearing.
2. **This week's picks.** 12 games, rules below. Shows price, and the discount if there is one.
3. **Deals.** Best reviewed first, at least 1,000 reviews on this page.
4. **Free to play.** Permanently free games, best reviewed first, at least 1,000 reviews on this page.

`See all` on sections 2 to 4 replaces the page with that one section at full length, with a way back, loading more as the owner scrolls, with no minimum review count and with the filters still applying. It shows no total, because Steam's totals are unreliable (see below).

Clicking any game opens its store page in the default browser (`https://store.steampowered.com/app/<appid>`). The existing `opener:allow-open-url` capability already covers this.

Times read as `23h`, `5d`, `ends in 1d`. Prices are Steam's own formatted strings for Greece (`country_code: GR`, euros).

### What the filters mean

The filters apply to sections 2 to 4.

- `Online co-op` means Steam's Online Co-op feature (category 38), `Single-player` means category 2. Both on means either one (a union). Neither on means all games.
- `Story only` further requires the Story Rich tag (1742).
- Adult games are always removed, in every section: any game whose tags include Hentai (9130) or NSFW (24904), or whose `content_descriptorids` include 3 or 4. Descriptors 1 and 5 are too broad to use: Baldur's Gate 3 and NEKOPARA both have them. The Sexual Content and Nudity tags are too broad as well (they appear on Baldur's Gate 3, The Witcher 3 and Cyberpunk 2077).

### This week's picks

- A game qualifies with at least 85% positive and at least 5,000 reviews. It need not be on sale.
- Two tiers from the current mode filters: games with Story Rich first, then the rest. Up to 8 picks come from the story tier and the remainder from the other, or all 12 from the story tier when `Story only` is on. If a tier runs short, the other fills in.
- Chosen once per week per filter combination, using a shuffle seeded by the ISO year and week, and saved locally. A week starts Monday 00:00 local time. Picks do not change during the week; their prices do.
- Last week's picks for that filter combination are excluded, so the list really changes. Without this and the shuffle, ranking by reviews would show the same games forever.

### Failure and empty states

Sections load independently and appear as they are ready. If one fails, it keeps its last saved data with a note that it could not refresh and a way to retry, and the others are unaffected. With no saved data and no connection, the section says so plainly.

## Notification

While MYLE is running, including hidden in the tray, a background task checks what is free shortly after start-up and then every 3 hours. When a new offer appears it shows MYLE's existing corner notice with the game's name, how long it lasts, and "+N more" if there are several. Clicking it opens MYLE on this page.

- The task must check the owner verdict (`AccountState::is_owner`) before each run and do nothing otherwise.
- An offer is identified by its trial or game app id plus its end time, so the same game returning later counts as new. Seen offers are saved locally and pruned after they expire.
- The very first run saves what it finds without any notice, so the owner is not flooded.
- If MYLE was not running when an offer started, the notice appears on the next check after it starts.

The notice lives in `backend/src/game_saves/notice.rs` and `frontend/notice/Notice.svelte` and is written around Game Saves. It needs a new kind with its own text, and click routing that depends on the kind: `notice_done` currently always opens Game Saves. Add an `--open-steam-helper` flag next to `OPEN_FLAG`, handled in `page_asked` and in the single-instance callback (`myle-navigate` with `steam-helper`).

## Data sources

No login and no API key are needed. All of this was observed against live Steam on 2026-11-08. None of it is a documented public API, so parsing must tolerate missing fields, and a changed response must produce a section error, never a crash.

**Game details: `IStoreBrowseService/GetItems`.** `https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json=…` with `ids: [{appid}]`, `context: {language: "english", country_code: "GR"}` and `data_request: {include_basic_info, include_reviews, include_assets, include_all_purchase_options, include_tag_count: 20}`. Ask for up to 50 ids per request, as `game_saves/covers.rs` does.

| Need | Field |
|---|---|
| Features | `categories.supported_player_categoryids` |
| Tags | `tags[].tagid` |
| Adult descriptors | `content_descriptorids` |
| Reviews | `reviews.summary_filtered.percent_positive`, `.review_count` |
| Price | `best_purchase_option.discount_pct`, `.formatted_final_price`, `.formatted_original_price` |
| Deal end (Unix seconds) | `best_purchase_option.active_discounts[0].discount_end_date` |
| Cover | `assets.header`, relative to `https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/<appid>/` (the CSP's `img-src https:` already allows it) |
| Free weekend | `free_weekend.end_time`, optional `.text`, optional `.appid` |

**Lists of games: the store search listing.** `https://store.steampowered.com/search/results/?infinite=1&category1=998&cc=gr&l=english&sort_by=Reviews_DESC&start=<n>&count=<n>` returns JSON with `total_count` and `results_html`. Take the app ids from `data-ds-appid="…"` with `regex-lite`, skip any value that is not one number, and fetch details through `GetItems`. Never render this HTML. `count` works at 25, 50 and 100 (anything lower returns 25), and a page can hold a row without a single numeric id, so advance `start` by the rows returned, not by the ids kept.

| Filter | Parameter |
|---|---|
| On sale | `specials=1` |
| Free to play | `maxprice=free` |
| 100% off | `maxprice=free&specials=1` |
| Online co-op / Single-player | `category2=38` / `category2=2` |
| Story only | `tags=1742` |

With both mode filters on, query each separately, merge by app id and order by percent positive, then review count.

How much to read: one 100-row page is enough to fill the main-page Deals and Free to play rows (with online co-op on, 48 and 40 games had 1,000 reviews or more on the first page). Picks read the first 200 rows of each tier: online co-op with story had 52 qualifying games, all on the first page, and plain online co-op had 70 on the first page.

**Free weekends: `IStoreQueryService/Query`.** Scan the first 1,000 games with `query: {start, count: 100, sort: 12, filters: {released_only: true, type_filters: {include_games: true}}}` and keep items with `free_weekend`. Treat offers sharing a `free_weekend.appid` as one (Call of Duty and Modern Warfare 4 were the same DMZ trial). All four matching items were in the first 300 games, and a scan of 3,000 found no more. A free weekend on an obscure game outside that range would be missed, which is accepted.

**Do not use `Query` for anything else.** Observed problems:

- Category filters are silently ignored (`categoryids_must_match` returned every game).
- Tag filters miss about a third of real online co-op deals: only 68 of Steam's top 99 were found, losing games like Wingspan and TMNT: Shredder's Revenge.
- `min_discount_percent: 100` is ignored and returns everything.
- `total_matching_records` counts duplicates: 347 rows held 265 unique games under the default sort.

**`sort_by=Reviews_DESC` drops games with no reviews** (498 results against 729 for the same search unsorted). That is fine for ranked rows and for `See all`, but the 100%-off listing must leave it out, because a brand-new giveaway may have no reviews yet.

**The Steam store home page is not needed.** It lists free weekends too, but the scan above found everything it did and more.

Both hosts accepted `MYLE` as the user agent, so use `crate::download::http_client("MYLE")`.

## Backend

A new `backend/src/steam_helper/` module, following the conventions in `docs/DEVELOPMENT.md`. The page sends filter values and app ids, and Rust builds every URL. Cache under `storage::local_dir()/steam-helper/`, keyed by section and filter combination, valid for one hour; the `Refresh` button bypasses it, at most once a minute per key. Use atomic writes. Space requests to Steam about 150 ms apart.

## Frontend

`frontend/app/pages/steam-helper/` with `SteamHelper.svelte`, `api.ts` and a singleton in `state.svelte.ts` so results survive navigation. Lazy-loaded, like the other owner pages.

## Testing

Rust unit tests with saved JSON fixtures, no network:

- Parsing `GetItems`, `Query` and search responses, including missing fields.
- Building search URLs from filter combinations.
- The adult-content rule, using the real tag and descriptor examples above.
- Merging two listings for the union of modes.
- Grouping free-weekend offers by trial app id.
- Weekly picks: the same inside a week, different after Monday, last week excluded, tier fill-in.
- Notification: first run is silent, a new offer fires once, a returning game fires again, expired entries are pruned.
- The owner gate covers `steam_helper_` commands.

One ignored live test (`#[ignore]`) that runs each source against real Steam, for checking by hand when Steam changes something. The usual `npm run check`, `npm test` and `npm run lint` must pass.

## Decided by the owner

Option B layout; picks based on reviews rather than the owner's library; owner-only for `thomasthanos28@gmail.com`; this piece first; and the section, filter and notification behaviour as played back and confirmed in conversation.

## Calls made while writing this, for the owner to check

- Lists come from the store search rather than the JSON query, because the JSON query misses a third of the real results.
- `See all` shows no number, unlike the mockup.
- At least 1,000 reviews for the main-page Deals and Free to play rows.
- Picks are saved for the week and do not repeat the previous week.
- The notification check runs every 3 hours.
- Game links open in the browser, not the Steam client.
- The public `README.md` stays unchanged while the page is owner-only; `docs/DEVELOPMENT.md` gets a section.

## Left to the builder

File layout inside the new folders, the command surface and how sections stream to the page, the look of the `See all` view and the empty states within the existing style, how chips are remembered, and the exact shape of the extended notice.
