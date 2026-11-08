<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Game } from "./api";
  import { reviewLabel, reviewTone, timeLeft } from "./model";

  let { game, now, showModes = false }: { game: Game; now: number; showModes?: boolean } = $props();

  const tone = $derived(reviewTone(game.reviewPercent, game.reviewCount));
  const review = $derived(reviewLabel(game.reviewPercent, game.reviewCount));
  const left = $derived(game.discountPercent > 0 ? timeLeft(game.dealEndsAt, now) : null);
  const modes = $derived(
    [game.onlineCoop && "Online co-op", game.story && "Story", game.singlePlayer && "Single-player"].filter(Boolean).join(" · "),
  );
</script>

<button class="card" type="button" title="Open {game.name} on Steam" onclick={() => void openUrl(game.storeUrl)}>
  {#if game.coverUrl}
    <img src={game.coverUrl} alt="" loading="lazy" />
  {:else}
    <span class="no-art"></span>
  {/if}
  <span class="body">
    <span class="name">{game.name}</span>
    <span class="review {tone}">{review}</span>
    {#if showModes && modes}<span class="modes">{modes}</span>{/if}
    <span class="price">
      {#if game.isFree}
        <span class="free">Free to play</span>
      {:else}
        {#if game.discountPercent > 0}
          <span class="off">-{game.discountPercent}%</span>
          {#if game.originalPrice}<span class="was">{game.originalPrice}</span>{/if}
        {/if}
        {#if game.price}<span class="now">{game.price}</span>{/if}
        {#if left}<span class="left">{left}</span>{/if}
      {/if}
    </span>
  </span>
</button>

<style>
  .card { display: block; width: 100%; min-width: 0; padding: 0; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); box-shadow: var(--surface-depth); color: var(--text-1); font: inherit; text-align: left; cursor: pointer; transition: border-color var(--dur-fast) var(--ease-out); }
  .card:hover, .card:focus-visible { border-color: rgb(var(--accent-rgb) / 0.55); }
  img, .no-art { display: block; width: 100%; aspect-ratio: 460 / 215; object-fit: cover; }
  .no-art { background: var(--input-fill); }
  .body { display: block; padding: 7px 9px 9px; }
  .name { display: block; overflow: hidden; font-size: 13px; font-weight: 700; white-space: nowrap; text-overflow: ellipsis; }
  .review { display: block; font-size: 11.5px; }
  .review.good { color: var(--ok); }
  .review.mid { color: var(--warn); }
  .review.none { color: var(--text-3); }
  .modes { display: block; margin-top: 2px; color: var(--text-2); font-size: 11.5px; }
  .price { display: flex; align-items: center; gap: 6px; min-height: 18px; margin-top: 5px; font-size: 13px; }
  .off { padding: 0 5px; border-radius: 5px; background: color-mix(in srgb, var(--ok) 28%, transparent); color: var(--text-1); font-size: 12px; font-weight: 700; }
  .was { color: var(--text-3); font-size: 11.5px; text-decoration: line-through; }
  .now { font-weight: 700; }
  .free { color: var(--ok); font-weight: 700; }
  .left { margin-left: auto; color: var(--warn); font-size: 11px; white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) { .card { transition: none; } }
</style>
