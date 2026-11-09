<script lang="ts">
  import { onMount } from "svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
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
  <h3>Games with cards left</h3>
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
  <div class="empty" role="alert"><p>{page.dropsError}</p><button class="btn small" type="button" onclick={() => void page.loadDrops(true)}>Try again</button></div>
{:else}
  <p class="empty">No games have cards left to drop.</p>
{/if}

<style>
  .row { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; }
  h3 { margin: 0; font-size: 13.5px; font-weight: 700; }
  .grow { flex: 1 1 0; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(136px, 1fr)); gap: 12px; }
  .empty { margin: 40px auto; color: var(--text-3); font-size: 13px; text-align: center; }
  .empty p { margin: 0 0 10px; }
</style>
