<script lang="ts">
  import { onMount } from "svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { BadgeGame, LibraryGame } from "../api";
  import FeaturePanel from "../FeaturePanel.svelte";
  import GameTile from "../GameTile.svelte";
  import { farmPanel, plural, untilText } from "../model";
  import StartButton from "../StartButton.svelte";
  import { steamAutomationState as page } from "../state.svelte";

  type CardGame = LibraryGame & { drops: number };
  let now = $state(Date.now() / 1000);

  const blank = (appid: number, name: string): LibraryGame => ({ appid, name, coverUrl: null, headerUrl: null, minutes: 0, achievementsUnlocked: 0, achievementsTotal: 0, achievementsOk: false, shared: false });
  const known = $derived(new Map((page.library ?? []).map((game) => [game.appid, game])));
  const skipped = $derived(page.settings?.cardBlacklist ?? []);
  const games = $derived(
    (page.drops ?? [])
      .map((game: BadgeGame): CardGame => ({ ...(known.get(game.appid) ?? blank(game.appid, game.name)), drops: game.drops, minutes: game.minutesPlayed }))
      .sort((a, b) => Number(skipped.includes(a.appid)) - Number(skipped.includes(b.appid)) || b.drops - a.drops || a.name.localeCompare(b.name)),
  );
  const chosen = $derived(games.filter((game) => !skipped.includes(game.appid)));
  const cards = $derived(chosen.reduce((sum, game) => sum + game.drops, 0));
  const status = $derived(page.status);
  const started = $derived(status?.started.farm ?? false);
  const current = $derived(chosen.find((game) => status?.running.includes(game.appid))?.name ?? null);
  const panel = $derived(farmPanel(started, status?.farm ?? { phase: "done", gamesLeft: 0, dropsLeft: 0 }, cards, chosen.length, current));
  const next = $derived(started && status?.farm.nextCheckAt ? untilText(status.farm.nextCheckAt, now) : "");
  const shared = $derived((page.library ?? []).filter((game) => game.shared).length);
  const total = $derived(page.library?.length ?? 0);

  onMount(() => {
    void page.loadDrops(false);
    const clock = setInterval(() => (now = Date.now() / 1000), 15_000);
    return () => clearInterval(clock);
  });
</script>

<FeaturePanel title={panel.title} detail={next ? `${panel.detail} Next check ${next}.` : panel.detail} active={started}>
  {#if page.settings}
    <label class="auto"><input class="switch" type="checkbox" checked={page.settings.farmOn} onchange={(event) => void page.change({ farmOn: event.currentTarget.checked })} />Start when MYLE opens</label>
  {/if}
  <StartButton feature="farm" disabled={chosen.length === 0} hint="No games to farm" />
</FeaturePanel>

{#if status?.unmatchedLicence && started}
  <p class="note">Games under 2 hours are waiting, in case one of them is "{status.unmatchedLicence}", which you can still refund.</p>
{:else if status?.farm.skippedRefundable && started}
  <p class="note">{plural(status.farm.skippedRefundable, "game")} left alone because you can still refund {status.farm.skippedRefundable === 1 ? "it" : "them"}.</p>
{/if}

<div class="row">
  <h3>Games with card drops left</h3>
  <span class="count">{games.length}</span>
  <span class="grow"></span>
  <button class="icon-btn" type="button" aria-label="Look for cards again" title="Look for cards again" disabled={page.dropsLoading} onclick={() => void page.loadDrops(true)}><RefreshCw size={15} /></button>
</div>

{#if games.length > 0}
  <div class="grid">
    {#each games as game (game.appid)}
      {@const on = !skipped.includes(game.appid)}
      <GameTile {game} drops={game.drops} running={status?.running.includes(game.appid)} muted={!on} state={status?.running.includes(game.appid) ? "Farming now" : ""}>
        {#snippet caption()}
          <label><input class="check" type="checkbox" checked={on} aria-label={`Farm cards in ${game.name}`} onchange={() => page.toggleSkipped(game.appid)} />{on ? "Will be farmed" : "Skipped"}</label>
        {/snippet}
      </GameTile>
    {/each}
  </div>
{:else if page.dropsLoading}
  <p class="empty" aria-busy="true">Looking for cards…</p>
{:else if page.dropsError}
  <div class="empty" role="alert">
    <p>{page.dropsError === "Sign in to Steam first." ? "Sign in to Steam to look for card drops." : page.dropsError}</p>
    <button class="btn small" class:primary={!page.status?.communityOk} type="button" onclick={() => void (page.status?.communityOk ? page.loadDrops(true) : page.signIn())}>
      {page.status?.communityOk ? "Try again" : "Sign in to Steam"}
    </button>
  </div>
{:else}
  <div class="empty">
    <p>
      {#if status?.steamAccount}
        Steam says <strong>{status.steamAccount}</strong> has no card drops left.
      {:else}
        Steam says there are no card drops left.
      {/if}
    </p>
    <p class="hint">
      Steam drops only about half of a card set. The “trading cards left to collect” count on a game's store page is the rest of the set, and those cards come from trading or the Market, never from playing. Cards you have already earned are on the Inventory tab. {#if shared}{shared} of your {total} games {shared === 1 ? "comes" : "come"} through Steam Family, which never drops cards. {/if}Free promotional copies do not drop cards either, and Steam can take a while to list a newly added game here.
    </p>
    <button class="btn small" type="button" onclick={() => void openUrl("https://steamcommunity.com/my/badges/")}>Open Steam's badges page</button>
  </div>
{/if}

<style>
  .row { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; }
  h3 { margin: 0; font-size: 13.5px; font-weight: 700; }
  .grow { flex: 1 1 0; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(136px, 1fr)); gap: 12px; }
  .empty { margin: 40px auto; color: var(--text-3); font-size: 13px; text-align: center; }
  .empty p { margin: 0 0 10px; }
  .empty .hint { max-width: 540px; margin: 6px auto 14px; font-size: 12px; line-height: 1.55; }
  .empty strong { color: var(--text-2); font-weight: 600; }
</style>
