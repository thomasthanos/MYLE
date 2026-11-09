<script lang="ts">
  // One game as tall library art, with its playtime, its progress and what can be done with it.
  import type { Snippet } from "svelte";
  import Clock from "@lucide/svelte/icons/clock";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import Layers from "@lucide/svelte/icons/layers";
  import Star from "@lucide/svelte/icons/star";
  import Trophy from "@lucide/svelte/icons/trophy";
  import { achievementsText, playtimeText } from "./model";

  interface TileGame {
    name: string;
    coverUrl: string | null;
    headerUrl: string | null;
    minutes: number;
    achievementsUnlocked: number;
    achievementsTotal: number;
    shared: boolean;
  }

  interface Props {
    game: TileGame;
    drops?: number;
    running?: boolean;
    muted?: boolean;
    favorite?: boolean;
    /** What is happening with this game right now. */
    state?: string;
    /** Appears over the art on hover. */
    actions?: Snippet;
    /** Always on show, under the art. */
    caption?: Snippet;
  }

  let { game, drops, running = false, muted = false, favorite = false, state: label = "", actions, caption }: Props = $props();

  // A game may have no tall art at that address, so its wide header is tried next.
  let attempt = $state(0);
  const sources = $derived([game.coverUrl, game.headerUrl].filter((url): url is string => !!url));
  const src = $derived(sources[attempt]);
  const playtime = $derived(playtimeText(game.minutes));
  const progress = $derived(achievementsText(game));
  const complete = $derived(game.achievementsTotal > 0 && game.achievementsUnlocked === game.achievementsTotal);
</script>

<article class="tile" class:muted>
  <div class="art" class:running>
    {#if src}
      <img {src} alt="" loading="lazy" decoding="async" onerror={() => (attempt += 1)} />
    {:else}
      <div class="blank"><Gamepad2 size={28} /></div>
    {/if}
    <div class="shade"></div>
    {#if playtime}<span class="badge left"><Clock size={11} />{playtime}</span>{/if}
    {#if drops !== undefined}
      <span class="badge right"><Layers size={11} />{drops}</span>
    {:else if progress}
      <span class="badge right" class:gold={complete}><Trophy size={11} />{progress}</span>
    {/if}
    <div class="foot">
      {#if label}<span class="state">{label}</span>{/if}
      {#if game.shared}<span class="family" title="Shared through your Steam Family. It drops no cards.">Family</span>{/if}
      <h3>{#if favorite}<Star class="star" size={12} />{/if}{game.name}</h3>
    </div>
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </div>
  {#if caption}<div class="caption">{@render caption()}</div>{/if}
</article>

<style>
  .tile { min-width: 0; content-visibility: auto; contain-intrinsic-size: auto 136px auto 204px; }
  .tile.muted .art { opacity: 0.45; }
  .art { position: relative; aspect-ratio: 2 / 3; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); transition: opacity 120ms ease; }
  .art:hover, .art:focus-within { border-color: rgb(var(--accent-rgb) / 0.55); }
  .art.running { border-color: var(--ok); box-shadow: 0 0 0 1px var(--ok); }
  img, .blank { display: block; width: 100%; height: 100%; object-fit: cover; }
  .blank { display: grid; place-items: center; color: var(--text-3); background: var(--input-fill); }
  .shade { position: absolute; inset: 52% 0 0; background: linear-gradient(transparent, var(--scrim)); pointer-events: none; }
  .badge { position: absolute; top: 6px; display: inline-flex; align-items: center; gap: 4px; padding: 1px 6px; border-radius: 7px; background: var(--scrim); color: var(--text-1); font-size: 11px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .badge.left { left: 6px; }
  .badge.right { right: 6px; }
  .badge.gold { color: var(--warn); }
  .foot { position: absolute; left: 9px; right: 9px; bottom: 8px; display: grid; justify-items: start; gap: 3px; }
  .state { padding: 0 6px; border-radius: 5px; background: var(--ok); color: var(--bg-solid-panel); font-size: 10.5px; font-weight: 700; }
  .family { padding: 0 5px; border-radius: 5px; background: var(--scrim); color: var(--text-2); font-size: 10.5px; }
  h3 { display: -webkit-box; overflow: hidden; margin: 0; color: var(--text-1); font-size: 12.5px; font-weight: 700; line-height: 1.18; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; }
  h3 :global(.star) { margin-right: 4px; vertical-align: -1px; color: var(--warn); fill: var(--warn); }
  .actions { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; gap: 6px; background: var(--scrim); opacity: 0; transition: opacity 120ms ease; }
  .art:hover .actions, .art:focus-within .actions { opacity: 1; }
  .actions :global(button) { display: inline-flex; align-items: center; justify-content: center; gap: 6px; height: 34px; min-width: 34px; padding: 0 12px; border: 1px solid var(--btn-border); border-radius: 17px; background: var(--btn-primary); color: var(--text-1); font: inherit; font-size: 12.5px; font-weight: 700; cursor: pointer; }
  .actions :global(button.quiet) { padding: 0; background: var(--scrim); }
  .actions :global(button.quiet.on) { color: var(--warn); }
  .actions :global(button.stop) { background: var(--danger); }
  .actions :global(button:hover:not(:disabled)), .actions :global(button:focus-visible) { border-color: var(--text-1); }
  .caption { display: flex; align-items: center; min-height: 26px; padding: 4px 2px 0; color: var(--text-2); font-size: 12px; }
  .caption :global(label) { display: flex; align-items: center; gap: 7px; min-width: 0; cursor: pointer; }
  @media (prefers-reduced-motion: reduce) { .art, .actions { transition: none; } }
</style>
