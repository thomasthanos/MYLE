<script lang="ts">
  // One game as tall library art, with its playtime, its progress and what can be done with it.
  import type { Snippet } from "svelte";
  import Clock from "@lucide/svelte/icons/clock";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import Layers from "@lucide/svelte/icons/layers";
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

  let { game, drops, running = false, actions }: { game: TileGame; drops?: number; running?: boolean; actions?: Snippet } = $props();

  // A game may have no tall art at that address, so its wide header is tried next.
  let attempt = $state(0);
  const sources = $derived([game.coverUrl, game.headerUrl].filter((url): url is string => !!url));
  const src = $derived(sources[attempt]);
  const playtime = $derived(playtimeText(game.minutes));
  const progress = $derived(achievementsText(game));
  const complete = $derived(game.achievementsTotal > 0 && game.achievementsUnlocked === game.achievementsTotal);
</script>

<article class="tile" class:running>
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
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
  <div class="foot">
    {#if game.shared}<span class="family">Family</span>{/if}
    <h3>{game.name}</h3>
  </div>
</article>

<style>
  .tile { position: relative; aspect-ratio: 2 / 3; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); content-visibility: auto; contain-intrinsic-size: 136px 204px; }
  .tile:hover, .tile:focus-within { border-color: rgb(var(--accent-rgb) / 0.55); }
  .tile.running { border-color: var(--ok); }
  img, .blank { display: block; width: 100%; height: 100%; object-fit: cover; }
  .blank { display: grid; place-items: center; color: var(--text-3); background: var(--input-fill); }
  .shade { position: absolute; inset: 52% 0 0; background: linear-gradient(transparent, var(--scrim)); pointer-events: none; }
  .badge { position: absolute; top: 6px; display: inline-flex; align-items: center; gap: 4px; padding: 1px 6px; border-radius: 7px; background: var(--scrim); color: var(--text-1); font-size: 11px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .badge.left { left: 6px; }
  .badge.right { right: 6px; }
  .badge.gold { color: var(--warn); }
  .actions { position: absolute; left: 0; right: 0; top: 32px; display: flex; justify-content: center; gap: 6px; opacity: 0; transition: opacity 120ms ease; }
  .tile:hover .actions, .tile:focus-within .actions { opacity: 1; }
  .actions :global(button) { display: grid; place-items: center; width: 32px; height: 32px; border: 1px solid var(--btn-border); border-radius: 50%; background: var(--scrim); color: var(--text-1); cursor: pointer; }
  .actions :global(button:hover:not(:disabled)), .actions :global(button:focus-visible) { border-color: rgb(var(--accent-rgb) / 0.8); }
  .actions :global(button.on) { background: var(--btn-primary); }
  .actions :global(button:disabled) { opacity: 0.45; cursor: not-allowed; }
  .foot { position: absolute; left: 9px; right: 9px; bottom: 8px; }
  .family { display: inline-block; margin-bottom: 3px; padding: 0 5px; border-radius: 5px; background: var(--scrim); color: var(--text-2); font-size: 10.5px; }
  h3 { display: -webkit-box; overflow: hidden; margin: 0; color: var(--text-1); font-size: 12.5px; font-weight: 700; line-height: 1.18; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; }
  @media (prefers-reduced-motion: reduce) { .actions { transition: none; } }
</style>
