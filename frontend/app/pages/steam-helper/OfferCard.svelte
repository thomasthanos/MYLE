<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Check from "@lucide/svelte/icons/check";
  import Clock from "@lucide/svelte/icons/clock";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import ThumbsUp from "@lucide/svelte/icons/thumbs-up";
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

<article class="offer" class:owned>
  <button
    class="open"
    type="button"
    title="Open {offer.game.name} on Steam"
    onclick={() => void openUrl(offer.game.storeUrl)}
  >
    <div class="thumb">
      {#if offer.game.coverUrl}
        <img src={offer.game.coverUrl} alt="" loading="lazy" />
      {:else}
        <div class="no-art"><Gamepad2 size={20} /></div>
      {/if}
      <span class="badge" class:keep={toKeep}>
        {#if toKeep}
          <Sparkles size={10} /> 100% OFF
        {:else}
          WEEKEND
        {/if}
      </span>
    </div>

    <div class="body">
      <span class="name" title={offer.game.name}>{offer.game.name}</span>
      <div class="meta">
        <span class="review {tone}">
          {#if tone === "good"}<ThumbsUp size={10} strokeWidth={2.4} />{/if}
          <span>{review}</span>
        </span>
        <span class="dot" aria-hidden="true">·</span>
        {#if left}
          <span class="left" title="Ends in {left}">
            <Clock size={10} /> {left}
          </span>
        {/if}
      </div>
    </div>
  </button>

  <div class="act">
    {#if owned}
      <span class="have" title="In your Steam library">
        <Check size={12} strokeWidth={2.5} /> In library
      </span>
    {:else if toKeep && account.owner}
      <button
        class="btn-act primary"
        type="button"
        disabled={adding}
        title="Claim it for free forever"
        onclick={() => void helper.addToAccount(appid, offer.game.name)}
      >
        {adding ? "Adding…" : "Claim forever"}
      </button>
    {:else if toKeep}
      <button
        class="btn-act primary"
        type="button"
        title="Get on Steam"
        onclick={() => void openUrl(offer.game.storeUrl)}
      >
        Get on Steam
      </button>
    {:else}
      <button
        class="btn-act"
        type="button"
        title="Play during free weekend"
        onclick={() => void helper.install(trial)}
      >
        Play in Steam
      </button>
    {/if}
  </div>
</article>

<style>
  .offer {
    position: relative;
    display: flex;
    align-items: center;
    min-width: 0;
    height: 64px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, rgba(34, 37, 48, 0.85) 0%, rgba(18, 20, 26, 0.95) 100%);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08), 0 3px 10px rgba(0, 0, 0, 0.35);
    overflow: hidden;
    transition: transform 0.2s var(--ease-out),
                border-color 0.2s var(--ease-out),
                box-shadow 0.2s var(--ease-out);
  }

  .offer:hover,
  .offer:focus-within {
    transform: translateY(-2px);
    border-color: rgb(var(--accent-rgb) / 0.55);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16),
                0 10px 22px -4px rgba(0, 0, 0, 0.65),
                0 0 0 1px rgb(var(--accent-rgb) / 0.3);
  }

  .open {
    display: flex;
    align-items: center;
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-1);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .thumb {
    position: relative;
    width: 110px;
    height: 100%;
    flex-shrink: 0;
    overflow: hidden;
    background: #14161e;
  }

  .thumb img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform 0.35s var(--ease-out);
  }

  .offer:hover .thumb img {
    transform: scale(1.06);
  }

  .thumb .no-art {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background: linear-gradient(135deg, #1e2230 0%, #11131a 100%);
    color: var(--text-3);
    opacity: 0.5;
  }

  .thumb::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, transparent 60%, rgba(18, 20, 26, 0.7) 100%);
    pointer-events: none;
  }

  .badge {
    position: absolute;
    bottom: 5px;
    left: 5px;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1.5px 5px;
    border-radius: 4px;
    background: linear-gradient(135deg, #10b981, #059669);
    color: #ffffff;
    font-size: 9.5px;
    font-weight: 800;
    letter-spacing: 0.3px;
    text-transform: uppercase;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.6);
  }

  .badge.keep {
    background: #a4d007;
    color: #121f00;
  }

  .body {
    display: flex;
    flex-direction: column;
    justify-content: center;
    flex: 1;
    min-width: 0;
    padding: 0 10px;
    gap: 3px;
  }

  .name {
    display: block;
    overflow: hidden;
    font-size: 12.5px;
    font-weight: 700;
    color: var(--text-1);
    white-space: nowrap;
    text-overflow: ellipsis;
    line-height: 1.25;
    transition: color var(--dur-fast);
  }

  .offer:hover .name {
    color: #ffffff;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    min-width: 0;
  }

  .review {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-weight: 600;
    white-space: nowrap;
  }

  .review.good { color: #34d399; }
  .review.mid { color: #fbbf24; }
  .review.none { color: var(--text-3); font-weight: 500; }

  .dot {
    color: var(--text-3);
    opacity: 0.5;
  }

  .left {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: #f5b454;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }

  .act {
    display: flex;
    align-items: center;
    padding-right: 10px;
    flex-shrink: 0;
    z-index: 2;
  }

  .have {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: #34d399;
    font-size: 11px;
    font-weight: 700;
    white-space: nowrap;
  }

  .btn-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 26px;
    padding: 0 9px;
    border: 1px solid var(--btn-border);
    border-radius: 6px;
    background: var(--btn-fill);
    color: var(--text-1);
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--dur-fast), border-color var(--dur-fast), transform var(--dur-fast);
  }

  .btn-act:hover {
    background: var(--btn-fill-hover);
    border-color: rgb(var(--accent-rgb) / 0.55);
    transform: translateY(-1px);
  }

  .btn-act:active {
    background: var(--btn-fill-press);
    transform: translateY(0);
  }

  .btn-act.primary {
    background: var(--btn-primary);
    border-color: var(--btn-primary-hover);
    color: #ffffff;
  }

  .btn-act.primary:hover {
    background: var(--btn-primary-hover);
  }

  @media (prefers-reduced-motion: reduce) {
    .offer,
    .thumb img,
    .btn-act {
      transition: none;
    }
  }
</style>
