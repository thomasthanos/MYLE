<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { FreeOffer } from "./api";
  import { reviewLabel, reviewTone, timeLeft } from "./model";

  let { offer, now }: { offer: FreeOffer; now: number } = $props();

  const tone = $derived(reviewTone(offer.game.reviewPercent, offer.game.reviewCount));
  const review = $derived(reviewLabel(offer.game.reviewPercent, offer.game.reviewCount));
  const left = $derived(timeLeft(offer.endsAt, now));
</script>

<button class="offer" type="button" title="Open {offer.game.name} on Steam" onclick={() => void openUrl(offer.game.storeUrl)}>
  {#if offer.game.coverUrl}
    <img src={offer.game.coverUrl} alt="" loading="lazy" />
  {:else}
    <span class="no-art"></span>
  {/if}
  <span class="body">
    <span class="name">{offer.game.name}</span>
    <span class="review {tone}">{review}</span>
    <span class="line">
      <span class="kind">{offer.label}</span>
      {#if left}<span class="left">ends in {left}</span>{/if}
    </span>
  </span>
</button>

<style>
  .offer { display: grid; grid-template-columns: 112px minmax(0, 1fr); width: 100%; min-width: 0; padding: 0; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); box-shadow: var(--surface-depth); color: var(--text-1); font: inherit; text-align: left; cursor: pointer; transition: border-color var(--dur-fast) var(--ease-out); }
  .offer:hover, .offer:focus-visible { border-color: rgb(var(--accent-rgb) / 0.55); }
  img, .no-art { display: block; width: 100%; height: 100%; object-fit: cover; }
  .no-art { min-height: 64px; background: var(--input-fill); }
  .body { display: block; min-width: 0; padding: 7px 10px; }
  .name { display: block; overflow: hidden; font-size: 13px; font-weight: 700; white-space: nowrap; text-overflow: ellipsis; }
  .review { display: block; font-size: 11.5px; }
  .review.good { color: var(--ok); }
  .review.mid { color: var(--warn); }
  .review.none { color: var(--text-3); }
  .line { display: flex; align-items: center; gap: 6px; margin-top: 3px; }
  .kind { padding: 0 6px; overflow: hidden; border-radius: 5px; background: color-mix(in srgb, var(--ok) 22%, transparent); color: var(--text-1); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; }
  .left { margin-left: auto; color: var(--warn); font-size: 11px; white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) { .offer { transition: none; } }
</style>
