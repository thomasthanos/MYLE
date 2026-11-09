<script lang="ts" generics="T extends { appid: number }, S extends string">
  // A list of games with the search and sort above it, and what to say while there are none.
  import type { Snippet } from "svelte";
  import Select from "../../../lib/components/Select.svelte";

  interface Props {
    games: T[];
    query: string;
    sort: S;
    sorts: readonly { value: S; label: string }[];
    placeholder: string;
    empty: string;
    loading?: boolean;
    error?: string | null;
    onretry?: () => void;
    chips?: Snippet;
    controls?: Snippet;
    note?: Snippet;
    tile: Snippet<[T]>;
  }

  let { games, query = $bindable(), sort = $bindable(), sorts, placeholder, empty, loading = false, error = null, onretry, chips, controls, note, tile }: Props = $props();

  const SKELETONS = Array.from({ length: 14 }, (_, index) => index);
</script>

<div class="row">
  {@render chips?.()}
  <span class="grow"></span>
  <input class="input search" type="search" {placeholder} aria-label={placeholder} bind:value={query} />
  <Select bind:value={sort} options={sorts} ariaLabel="Sort by" align="end" />
  {@render controls?.()}
</div>
{@render note?.()}

{#if games.length > 0}
  <div class="grid">
    {#each games as game (game.appid)}{@render tile(game)}{/each}
  </div>
{:else if loading}
  <div class="grid" aria-busy="true">
    {#each SKELETONS as index (index)}<div class="ghost"></div>{/each}
  </div>
{:else if error}
  <div class="empty" role="alert">
    <p>{error}</p>
    {#if onretry}<button class="btn small" type="button" onclick={onretry}>Try again</button>{/if}
  </div>
{:else}
  <p class="empty">{empty}</p>
{/if}

<style>
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 14px; }
  .grow { flex: 1 1 0; }
  .search { flex: 0 1 220px; min-width: 140px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(136px, 1fr)); gap: 12px; }
  .ghost { aspect-ratio: 2 / 3; border-radius: var(--radius-md); background: var(--input-fill); }
  .empty { margin: 40px auto; color: var(--text-3); font-size: 13px; text-align: center; }
  .empty p { margin: 0 0 10px; }
</style>
