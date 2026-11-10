<script lang="ts">
  import { onMount } from "svelte";
  import Trophy from "@lucide/svelte/icons/trophy";
  import type { LibraryGame } from "../api";
  import AchievementsDialog from "../AchievementsDialog.svelte";
  import FeaturePanel from "../FeaturePanel.svelte";
  import GameGrid from "../GameGrid.svelte";
  import GameTile from "../GameTile.svelte";
  import { addAll, GAME_SORTS, plural, searchGames, sortGames, untilText } from "../model";
  import StartButton from "../StartButton.svelte";
  import { steamAutomationState as page } from "../state.svelte";

  const find = page.find.achievements;
  const status = $derived(page.status);
  const settings = $derived(page.settings);
  const candidates = $derived((page.library ?? []).filter((game) => game.achievementsOk));
  const chosen = $derived(settings?.achievementGames ?? []);
  const queue = $derived(candidates.filter((game) => chosen.includes(game.appid)));
  const games = $derived(sortGames(searchGames(find.show === "all" ? candidates : queue, find.query), find.sort));
  const started = $derived(status?.started.achievements ?? false);
  const unlocked = $derived(status ? status.achievements.total - status.achievements.locked : 0);
  const title = $derived.by(() => {
    if (started && status?.achievements.waitingForFarming) return "Waiting for card farming to finish";
    if (started) return `Unlocking achievements in ${plural(queue.length, "game")}`;
    return queue.length ? `Auto-unlock is on for ${plural(queue.length, "game")}` : "Open a game to unlock its achievements";
  });
  const detail = $derived.by(() => {
    if (started && status && status.achievements.total > 0) {
      const next = status.achievements.nextAt ? ` Next one ${untilText(status.achievements.nextAt, now)}.` : "";
      return `${unlocked} of ${status.achievements.total} unlocked.${next}`;
    }
    return "Open a game to unlock or lock its achievements now. Ticked games get the rest one at a time, 5 to 30 minutes apart. Games with VAC are left out.";
  });
  let now = $state(Date.now() / 1000);
  /** The game whose achievements are open. */
  let open = $state<LibraryGame | null>(null);

  onMount(() => {
    const clock = setInterval(() => (now = Date.now() / 1000), 15_000);
    return () => clearInterval(clock);
  });
</script>

{#snippet chips()}
  <div class="chips" role="group" aria-label="Which games">
    <button class="chip" class:active={find.show === "all"} type="button" aria-pressed={find.show === "all"} onclick={() => (find.show = "all")}>All<span class="count">{candidates.length}</span></button>
    <button class="chip" class:active={find.show === "queue"} type="button" aria-pressed={find.show === "queue"} onclick={() => (find.show = "queue")}>Auto-unlock<span class="count">{queue.length}</span></button>
    <button class="chip" type="button" disabled={!settings} onclick={() => settings && void page.change({ achievementGames: addAll(settings.achievementGames, candidates) })}>Auto-unlock all</button>
    {#if chosen.length > 0}<button class="chip" type="button" onclick={() => void page.change({ achievementGames: [] })}>Clear</button>{/if}
  </div>
{/snippet}

{#snippet tile(game: LibraryGame)}
  {@const on = chosen.includes(game.appid)}
  <GameTile {game} running={page.status?.running.includes(game.appid)}>
    {#snippet actions()}
      <button type="button" onclick={() => (open = game)}><Trophy size={14} /> Achievements</button>
    {/snippet}
    {#snippet caption()}
      <label title="MYLE unlocks the rest one at a time while Achievements is started"><input class="check" type="checkbox" checked={on} aria-label={`Unlock the rest of ${game.name}'s achievements automatically`} onchange={() => page.toggleAchievementGame(game.appid)} /><span class="auto-text">Auto-unlock</span></label>
      <button class="open" type="button" onclick={() => (open = game)}>Open</button>
    {/snippet}
  </GameTile>
{/snippet}

<FeaturePanel {title} {detail} active={started}>
  {#if settings}
    <label class="auto"><input class="switch" type="checkbox" checked={settings.achievementsOn} onchange={(event) => void page.change({ achievementsOn: event.currentTarget.checked })} />Start when MYLE opens</label>
  {/if}
  <StartButton feature="achievements" disabled={queue.length === 0} hint="Tick Auto-unlock on a game first" />
</FeaturePanel>

<GameGrid {games} bind:query={find.query} bind:sort={find.sort} sorts={GAME_SORTS} placeholder="Search" empty={find.show === "queue" ? "No game has Auto-unlock on yet." : "No games with achievements to offer."} loading={page.libraryLoading} error={page.libraryError} onretry={() => void page.loadLibrary(true)} {chips} {tile} />

{#if open}<AchievementsDialog game={open} onclose={() => (open = null)} />{/if}

<style>
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .auto-text { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .open { flex: none; margin-left: auto; padding: 2px 8px; border: 1px solid var(--btn-border); border-radius: 7px; background: var(--btn-fill); color: var(--text-1); font: inherit; font-size: 11.5px; cursor: pointer; }
  .open:hover { background: var(--btn-fill-hover); }
</style>
