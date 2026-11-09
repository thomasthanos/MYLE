<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { account } from "../../account/account.svelte";
  import type { FreeOffer } from "./api";
  import { reviewLabel, reviewTone, timeLeft } from "./model";
  import { steamHelperState as helper } from "./state.svelte";

  let { offer, now }: { offer: FreeOffer; now: number } = $props();

  const tone = $derived(reviewTone(offer.game.reviewPercent, offer.game.reviewCount));
  const review = $derived(reviewLabel(offer.game.reviewPercent, offer.game.reviewCount));
  const left = $derived(timeLeft(offer.endsAt, now));
  const appid = $derived(offer.game.appid);
  const owned = $derived(helper.owned.has(appid));
  const adding = $derived(helper.adding.has(appid));
  const toKeep = $derived(offer.kind === "freeToKeep");
  /** A free weekend's game id can differ from the game's own, when it is a separate trial. */
  const trial = $derived(Number(offer.offerId.split(":")[0]) || appid);
</script>

<article class="offer">
  <button class="open" type="button" title="Open {offer.game.name} on Steam" onclick={() => void openUrl(offer.game.storeUrl)}>
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
  <div class="act">
    {#if owned}
      <span class="have">In your library</span>
    {:else if toKeep && account.owner}
      <button class="btn small primary" type="button" disabled={adding} title="Adds it to your Steam account for good" onclick={() => void helper.addToAccount(appid, offer.game.name)}>{adding ? "Adding…" : "Add to account"}</button>
    {:else if toKeep}
      <button class="btn small" type="button" onclick={() => void openUrl(offer.game.storeUrl)}>Get it on Steam</button>
    {:else}
      <button class="btn small" type="button" title="Opens Steam to add it for the free period" onclick={() => void helper.install(trial)}>Play free in Steam</button>
    {/if}
  </div>
</article>

<style>
  .offer { display: grid; min-width: 0; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); box-shadow: var(--surface-depth); transition: border-color var(--dur-fast) var(--ease-out); }
  .offer:hover, .offer:focus-within { border-color: rgb(var(--accent-rgb) / 0.55); }
  .open { display: grid; grid-template-columns: 112px minmax(0, 1fr); width: 100%; min-width: 0; padding: 0; border: 0; background: none; color: var(--text-1); font: inherit; text-align: left; cursor: pointer; }
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
  .act { display: flex; justify-content: flex-end; align-items: center; min-height: 36px; padding: 5px 8px; border-top: 1px solid var(--panel-rim); }
  .have { color: var(--ok); font-size: 12px; font-weight: 700; }
  @media (prefers-reduced-motion: reduce) { .offer { transition: none; } }
</style>
