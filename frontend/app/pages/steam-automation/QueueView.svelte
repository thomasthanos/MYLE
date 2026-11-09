<script lang="ts">
  // All games and the queue of those a feature works through.
  import type { Snippet } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Plus from "@lucide/svelte/icons/plus";
  import type { Feature, LibraryGame } from "./api";
  import GameGrid from "./GameGrid.svelte";
  import GameTile from "./GameTile.svelte";
  import { GAME_SORTS, searchGames, sortGames, type GameSort } from "./model";
  import StartButton from "./StartButton.svelte";
  import { steamAutomationState as page } from "./state.svelte";

  interface Props {
    find: { query: string; sort: GameSort; tab: "all" | "queue" };
    queue: "idleGames" | "achievementGames";
    feature: Feature;
    candidates: LibraryGame[];
    autoLabel: string;
    autoOn: boolean;
    onAuto: (on: boolean) => void;
    canAdd?: (appid: number) => boolean;
    fullHint?: string;
    emptyAll: string;
    extra?: Snippet;
    note?: Snippet;
  }

  let { find, queue, feature, candidates, autoLabel, autoOn, onAuto, canAdd = () => true, fullHint = "", emptyAll, extra, note }: Props = $props();

  const queued = $derived(page.settings?.[queue] ?? []);
  const inQueue = $derived(candidates.filter((game) => queued.includes(game.appid)));
  const games = $derived(sortGames(searchGames(find.tab === "all" ? candidates : inQueue, find.query), find.sort));
</script>

{#snippet tabs()}
  <div class="tabs" role="tablist" aria-label="Which games">
    <button class="tab" class:on={find.tab === "all"} type="button" role="tab" aria-selected={find.tab === "all"} onclick={() => (find.tab = "all")}>All games<i>{candidates.length}</i></button>
    <button class="tab" class:on={find.tab === "queue"} type="button" role="tab" aria-selected={find.tab === "queue"} onclick={() => (find.tab = "queue")}>Queue<i>{inQueue.length}</i></button>
  </div>
{/snippet}

{#snippet tile(game: LibraryGame)}
  {@const on = queued.includes(game.appid)}
  <GameTile {game} running={page.status?.running.includes(game.appid)}>
    {#snippet actions()}
      <button type="button" class:on aria-pressed={on} disabled={!on && !canAdd(game.appid)} aria-label={on ? `Take ${game.name} off the queue` : `Add ${game.name} to the queue`} title={!on && !canAdd(game.appid) ? fullHint : on ? "Take off the queue" : "Add to the queue"} onclick={() => page.toggleQueue(queue, game.appid)}>{#if on}<Check size={15} />{:else}<Plus size={15} />{/if}</button>
    {/snippet}
  </GameTile>
{/snippet}

<GameGrid {games} bind:query={find.query} bind:sort={find.sort} sorts={GAME_SORTS} placeholder="Search" empty={find.tab === "queue" ? "The queue is empty. Add games from All games." : emptyAll} loading={page.libraryLoading} error={page.libraryError} onretry={() => void page.loadLibrary(true)} {tabs} {tile} {note}>
  {#snippet controls()}
    <label class="auto">{autoLabel}<input class="switch" type="checkbox" checked={autoOn} onchange={(event) => onAuto(event.currentTarget.checked)} /></label>
    {@render extra?.()}
    <StartButton {feature} disabled={inQueue.length === 0} hint="Add a game to the queue first" />
  {/snippet}
</GameGrid>
