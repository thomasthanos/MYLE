<script lang="ts">
  // The whole library. Idling and favorites are ways of looking at it, not places of their own.
  import Play from "@lucide/svelte/icons/play";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Square from "@lucide/svelte/icons/square";
  import Star from "@lucide/svelte/icons/star";
  import type { LibraryGame } from "../api";
  import GameGrid from "../GameGrid.svelte";
  import GameTile from "../GameTile.svelte";
  import { GAME_SORTS, plural, searchGames, sortGames } from "../model";
  import { steamAutomationState as page, type GamesShown } from "../state.svelte";

  const find = page.find.games;
  const all = $derived(page.library ?? []);
  const favorites = $derived(all.filter((game) => page.settings?.favorites.includes(game.appid)));
  const idling = $derived(all.filter((game) => page.isIdling(game.appid)));
  const lists = $derived<Record<GamesShown, LibraryGame[]>>({ all, favorites, idling });
  const games = $derived(sortGames(searchGames(lists[find.show], find.query), find.sort));
  const placeholder = $derived(all.length ? `Search ${all.length} games` : "Search your games");
  const EMPTY: Record<GamesShown, string> = {
    all: "No games yet.",
    favorites: "Nothing here yet. Hover a game and press the star to keep it close.",
    idling: "Nothing is idling. Hover a game and press Idle to add playtime to it.",
  };
  const CHIPS: { value: GamesShown; label: string }[] = [
    { value: "all", label: "All" },
    { value: "favorites", label: "Favorites" },
    { value: "idling", label: "Idling" },
  ];
  const empty = $derived(lists[find.show].length ? "No games match." : EMPTY[find.show]);
</script>

{#snippet chips()}
  <div class="chips" role="group" aria-label="Which games">
    {#each CHIPS as chip (chip.value)}
      <button class="chip" class:active={find.show === chip.value} type="button" aria-pressed={find.show === chip.value} onclick={() => (find.show = chip.value)}>{chip.label}<span class="count">{lists[chip.value].length}</span></button>
    {/each}
  </div>
{/snippet}

{#snippet tile(game: LibraryGame)}
  {@const on = page.isIdling(game.appid)}
  {@const favorite = page.settings?.favorites.includes(game.appid) ?? false}
  {@const running = page.status?.running.includes(game.appid) ?? false}
  <GameTile {game} {running} {favorite} state={on ? "Idling" : running ? "Farming" : ""}>
    {#snippet actions()}
      <button type="button" class:stop={on} onclick={() => void page.idleGame(game.appid, !on)}>{#if on}<Square size={12} />Stop{:else}<Play size={13} />Idle{/if}</button>
      <button class="quiet" class:on={favorite} type="button" aria-pressed={favorite} aria-label={favorite ? `Remove ${game.name} from favorites` : `Add ${game.name} to favorites`} title={favorite ? "Remove from favorites" : "Add to favorites"} onclick={() => page.toggleFavorite(game.appid)}><Star size={14} /></button>
    {/snippet}
  </GameTile>
{/snippet}

<GameGrid {games} bind:query={find.query} bind:sort={find.sort} sorts={GAME_SORTS} {placeholder} {empty} loading={page.libraryLoading} error={page.libraryError} onretry={() => void page.loadLibrary(true)} {chips} {tile}>
  {#snippet controls()}
    <button class="icon-btn" type="button" aria-label="Read your library again" title="Read your library again" disabled={page.libraryLoading} onclick={() => void page.loadLibrary(true)}><RefreshCw size={15} /></button>
  {/snippet}
  {#snippet note()}
    {#if page.libraryProblem}<p class="note">Showing the last saved list. {page.libraryProblem}</p>{/if}
    {#if find.show === "idling" && page.settings}
      <div class="under">
        <label class="auto"><input class="switch" type="checkbox" checked={page.settings.idleOn} onchange={(event) => void page.change({ idleOn: event.currentTarget.checked })} />Keep idling these when MYLE opens</label>
        {#if page.status?.idleSkipped}<span class="note inline">Skipping {plural(page.status.idleSkipped, "game")} at launch that can still be refunded.</span>{/if}
      </div>
    {/if}
  {/snippet}
</GameGrid>

<style>
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .under { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin-bottom: 14px; }
</style>
