<script lang="ts">
  import type { LibraryGame } from "../api";
  import GameButtons from "../GameButtons.svelte";
  import GameGrid from "../GameGrid.svelte";
  import GameTile from "../GameTile.svelte";
  import { GAME_SORTS, searchGames, sortGames } from "../model";
  import { steamAutomationState as page } from "../state.svelte";

  const find = page.find.games;
  const games = $derived(sortGames(searchGames(page.library ?? [], find.query), find.sort));
  const placeholder = $derived(`Search your ${page.library?.length ?? ""} games`.replace("  ", " "));
</script>

{#snippet tile(game: LibraryGame)}
  <GameTile {game} running={page.status?.running.includes(game.appid)}>
    {#snippet actions()}<GameButtons {game} />{/snippet}
  </GameTile>
{/snippet}

<GameGrid {games} bind:query={find.query} bind:sort={find.sort} sorts={GAME_SORTS} {placeholder} empty={page.library?.length ? "No games match." : "No games yet."} loading={page.libraryLoading} error={page.libraryError} onretry={() => void page.loadLibrary(true)} {tile}>
  {#snippet controls()}
    <button class="btn" type="button" disabled={page.libraryLoading} onclick={() => void page.loadLibrary(true)}>{page.libraryLoading ? "Refreshing…" : "Refresh"}</button>
  {/snippet}
  {#snippet note()}
    {#if page.libraryProblem}<p class="note">Showing the last saved list. {page.libraryProblem}</p>{/if}
  {/snippet}
</GameGrid>
