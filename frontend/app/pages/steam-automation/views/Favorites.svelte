<script lang="ts">
  import type { LibraryGame } from "../api";
  import GameButtons from "../GameButtons.svelte";
  import GameGrid from "../GameGrid.svelte";
  import GameTile from "../GameTile.svelte";
  import { GAME_SORTS, searchGames, sortGames } from "../model";
  import { steamAutomationState as page } from "../state.svelte";

  const find = page.find.favorites;
  const favorites = $derived((page.library ?? []).filter((game) => page.settings?.favorites.includes(game.appid)));
  const games = $derived(sortGames(searchGames(favorites, find.query), find.sort));
</script>

{#snippet tile(game: LibraryGame)}
  <GameTile {game} running={page.status?.running.includes(game.appid)}>
    {#snippet actions()}<GameButtons {game} />{/snippet}
  </GameTile>
{/snippet}

<GameGrid {games} bind:query={find.query} bind:sort={find.sort} sorts={GAME_SORTS} placeholder="Search your favorites" empty={favorites.length ? "No favorites match." : "Nothing here yet. Star a game in Games to keep it close."} loading={page.libraryLoading} error={page.libraryError} onretry={() => void page.loadLibrary(true)} {tile} />
