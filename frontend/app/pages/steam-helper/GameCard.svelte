<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Check from "@lucide/svelte/icons/check";
  import Clock from "@lucide/svelte/icons/clock";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import ThumbsUp from "@lucide/svelte/icons/thumbs-up";
  import type { Game } from "./api";
  import { dealOver, reviewLabel, reviewTone, timeLeft } from "./model";
  import { steamHelperState as helper } from "./state.svelte";

  let { game, now, showModes = false }: { game: Game; now: number; showModes?: boolean } = $props();

  const tone = $derived(reviewTone(game.reviewPercent, game.reviewCount));
  const review = $derived(reviewLabel(game.reviewPercent, game.reviewCount));
  const over = $derived(dealOver(game.discountPercent, game.dealEndsAt, now));
  const left = $derived(game.discountPercent > 0 ? timeLeft(game.dealEndsAt, now) : null);
  const modes = $derived(
    [game.onlineCoop && "Online co-op", game.story && "Story", game.singlePlayer && "Single-player"].filter(Boolean).join(" · "),
  );
  const owned = $derived(helper.owned.has(game.appid));
</script>

<article class="wrap">
  <button
    class="card-head"
    type="button"
    title="Open {game.name} on Steam"
    onclick={() => void openUrl(game.storeUrl)}
  >
    <div class="media">
      {#if game.coverUrl}
        <img src={game.coverUrl} alt="" loading="lazy" />
      {:else}
        <div class="no-art"><Gamepad2 size={24} /></div>
      {/if}

      {#if game.isFree}
        <span class="badge-cover free">Free</span>
      {:else if over}
        <span class="badge-cover ended">Ended</span>
      {:else if game.discountPercent > 0}
        <span class="badge-cover discount">-{game.discountPercent}%</span>
      {/if}
    </div>

    <div class="body">
      <span class="name" title={game.name}>{game.name}</span>
      <span class="review {tone}">
        {#if tone === "good"}
          <ThumbsUp size={11} strokeWidth={2.4} />
        {:else if tone === "mid"}
          <ThumbsUp size={11} strokeWidth={2} />
        {/if}
        <span class="review-label">{review}</span>
      </span>
      {#if showModes}
        <span class="modes" title={modes || ""}>{modes || "\u00A0"}</span>
      {/if}
    </div>
  </button>

  <div class="foot">
    <div class="price-wrap">
      {#if game.isFree}
        <span class="free-text">Free to play</span>
      {:else if over}
        <span class="ended-text">Deal ended</span>
      {:else}
        {#if game.discountPercent > 0 && game.originalPrice}
          <span class="was">{game.originalPrice}</span>
        {/if}
        {#if game.price}
          <span class="now">{game.price}</span>
        {/if}
      {/if}
    </div>

    <div class="act-wrap">
      {#if game.isFree}
        {#if owned}
          <span class="badge-owned" title="In your Steam library">
            <Check size={11} strokeWidth={2.5} /> In library
          </span>
        {:else}
          <button
            class="btn-act"
            type="button"
            title="Add {game.name} to your Steam library"
            onclick={(e) => {
              e.stopPropagation();
              void helper.install(game.appid);
            }}
          >
            Add in Steam
          </button>
        {/if}
      {:else if left}
        <span class="badge-left" title="Deal ends in {left}">
          <Clock size={11} /> {left}
        </span>
      {:else}
        <button
          class="btn-link"
          type="button"
          title="Open {game.name} on Steam"
          onclick={() => void openUrl(game.storeUrl)}
        >
          <ExternalLink size={12} />
        </button>
      {/if}
    </div>
  </div>
</article>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    min-width: 0;
    height: 100%;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, rgba(34, 37, 48, 0.85) 0%, rgba(18, 20, 26, 0.95) 100%);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08), 0 3px 10px rgba(0, 0, 0, 0.35);
    transition: transform 0.2s var(--ease-out),
                border-color 0.2s var(--ease-out),
                box-shadow 0.2s var(--ease-out);
  }

  .wrap:hover,
  .wrap:focus-within {
    transform: translateY(-3px);
    border-color: rgb(var(--accent-rgb) / 0.55);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16),
                0 12px 26px -4px rgba(0, 0, 0, 0.65),
                0 0 0 1px rgb(var(--accent-rgb) / 0.3);
  }

  .card-head {
    display: flex;
    flex-direction: column;
    flex: 1;
    width: 100%;
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-1);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  /* Cover Media */
  .media {
    position: relative;
    width: 100%;
    aspect-ratio: 460 / 215;
    overflow: hidden;
    background: #14161e;
  }

  .media img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform 0.35s var(--ease-out);
  }

  .wrap:hover .media img {
    transform: scale(1.05);
  }

  .media .no-art {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background: linear-gradient(135deg, #1e2230 0%, #11131a 100%);
    color: var(--text-3);
    opacity: 0.5;
  }

  .media::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, transparent 65%, rgba(16, 18, 24, 0.65) 100%);
    pointer-events: none;
  }

  /* Badges over cover */
  .badge-cover {
    position: absolute;
    bottom: 6px;
    right: 6px;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    padding: 2px 6px;
    border-radius: 5px;
    font-size: 11px;
    font-weight: 800;
    line-height: 1.2;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.6);
  }

  .badge-cover.discount {
    background: #a4d007;
    color: #121f00;
    font-size: 11.5px;
    letter-spacing: -0.2px;
  }

  .badge-cover.free {
    background: linear-gradient(135deg, #10b981, #059669);
    color: #ffffff;
    font-size: 10px;
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  .badge-cover.ended {
    background: rgba(30, 32, 40, 0.9);
    color: var(--text-3);
    border: 1px solid rgba(255, 255, 255, 0.1);
  }

  /* Body */
  .body {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    padding: 8px 9px 6px;
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

  .wrap:hover .name {
    color: #ffffff;
  }

  .review {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
  }

  .review.good {
    color: #34d399;
  }

  .review.mid {
    color: #fbbf24;
  }

  .review.none {
    color: var(--text-3);
    font-weight: 500;
  }

  .review-label {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .modes {
    display: block;
    min-height: 15px;
    color: var(--text-3);
    font-size: 11px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* Foot */
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    height: 36px;
    padding: 0 9px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(0, 0, 0, 0.18);
  }

  .price-wrap {
    display: flex;
    align-items: baseline;
    gap: 5px;
    min-width: 0;
    font-variant-numeric: tabular-nums;
  }

  .was {
    color: var(--text-3);
    font-size: 11px;
    text-decoration: line-through;
  }

  .now {
    font-size: 12.5px;
    font-weight: 700;
    color: var(--text-1);
  }

  .free-text {
    color: #34d399;
    font-size: 11.5px;
    font-weight: 700;
  }

  .ended-text {
    color: var(--text-3);
    font-size: 11px;
  }

  .act-wrap {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
  }

  .badge-owned {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: #34d399;
    font-size: 11px;
    font-weight: 700;
    white-space: nowrap;
  }

  .badge-left {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: #f5b454;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }

  .btn-act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 24px;
    padding: 0 8px;
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

  .btn-link {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-3);
    border-radius: 4px;
    cursor: pointer;
    opacity: 0.5;
    transition: opacity var(--dur-fast), color var(--dur-fast);
  }

  .wrap:hover .btn-link,
  .btn-link:hover {
    opacity: 1;
    color: var(--accent);
  }

  @media (prefers-reduced-motion: reduce) {
    .wrap,
    .media img,
    .btn-act {
      transition: none;
    }
  }
</style>
