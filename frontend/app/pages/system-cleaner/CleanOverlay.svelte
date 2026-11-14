<script lang="ts">
  import { untrack } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import Radar from "@lucide/svelte/icons/radar";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import { formatSize } from "./api";
  import { stopCleanAudio } from "./clean-sound";
  import { cleanerState, type CelebrationSummary } from "./state.svelte";

  let { summary, onclose, onscan }: {
    summary: CelebrationSummary | null;
    onclose: () => void;
    onscan: () => void;
  } = $props();

  let heading = $state<HTMLHeadingElement>();
  const titleId = $props.id();
  const progress = $derived(cleanerState.progress);
  const percent = $derived(Math.round(cleanerState.barPercent * 100));
  const title = $derived(!summary ? "Cleaning selected items"
    : summary.freed === 0 ? "No space freed"
    : "Cleanup complete");
  const detail = $derived(cleanerState.settling ? "Checking for remaining files"
    : progress?.current || "Finishing this pass");

  function attachToPanel(node: HTMLElement) {
    // Mount beside the shell scroller so the overlay stays inside its visible frame at any scroll position.
    const panel = node.closest("main");
    if (!panel) return;
    panel.append(node);
    return () => node.remove();
  }

  $effect(() => {
    const result = summary;
    if (result) untrack(() => cleanerState.presentResult(result));
    return stopCleanAudio;
  });

  $effect(() => {
    // Announce the new heading when the cleanup starts and again when the result appears.
    if (title) heading?.focus({ preventScroll: true });
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    event.preventDefault();
    if (summary) onclose();
  }
</script>

<div class="layer" {@attach attachToPanel}>
  <dialog open class="glass glass--3" aria-labelledby={titleId} onkeydown={onKeydown}>
    <div class="body">
      <header>
        <h2 id={titleId} bind:this={heading} tabindex="-1">{title}</h2>
        <button class="sound" type="button" aria-label="Cleanup sound" aria-pressed={cleanerState.soundEnabled} title={cleanerState.soundEnabled ? "Mute cleanup sound" : "Enable cleanup sound"} onclick={() => cleanerState.toggleSound()}>
          {#if cleanerState.soundEnabled}<Volume2 size={16} />{:else}<VolumeX size={16} />{/if}
        </button>
      </header>

      <div class="instrument" class:completed={!!summary} class:success={(summary?.freed ?? 0) > 0}>
        <div class="readout">
          <div class="radar" aria-hidden="true">
            <span class="meter" style:--cleaner-reading="{summary ? 100 : percent}%"></span>
            {#if !summary}<span class="sweep"></span>{/if}
            <span class="radar-icon">
              {#if summary?.freed}<Check size={24} strokeWidth={1.7} />
              {:else if summary}<HardDrive size={21} strokeWidth={1.6} />
              {:else}<Radar size={22} strokeWidth={1.5} />{/if}
            </span>
          </div>
          <div class="readout-content">
            {#if summary}
              <span class="readout-label">Space freed</span>
              <strong class="reading">{formatSize(summary.freed)}</strong>
            {:else}
              <p class="readout-label" role="status">{detail}</p>
              <div class="progress">
                <strong class="reading" role="progressbar" aria-label="Cleanup progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>{percent}%</strong>
                {#if progress}<span class="step">Step {progress.step} of {progress.total}</span>{/if}
              </div>
            {/if}
          </div>
        </div>

        {#if summary && (summary.remaining > 0 || summary.skipped > 0)}
          <dl class="totals">
            {#if summary.remaining > 0}
              <div><dt>Still on disk</dt><dd>{formatSize(summary.remaining)}</dd></div>
            {/if}
            {#if summary.skipped > 0}
              <div><dt>{summary.skipped === 1 ? "File skipped" : "Files skipped"}</dt><dd>{summary.skipped.toLocaleString()}</dd></div>
            {/if}
          </dl>
        {/if}
      </div>

      {#if summary}
        {#if summary.systemNote}
          <p class="notice"><ShieldCheck size={14} /> {summary.systemNote}</p>
        {/if}

        {#if summary.items.length}
          <details>
            <summary><ChevronRight size={14} /> Space freed by category</summary>
            <ul class="breakdown">
              {#each summary.items as item (item.id)}
                <li>
                  <div class="category-result"><span>{item.title}</span><strong>{formatSize(item.freed)}</strong></div>
                  {#if item.remaining || item.inUse || item.otherSkipped || item.systemFoldersSkipped}
                    <div class="category-notes">
                      {#if item.remaining > 0}<span>{formatSize(item.remaining)} still on disk</span>{/if}
                      {#if item.inUse > 0}
                        <span class="in-use"><LockKeyhole size={13} />
                          {item.inUse.toLocaleString()} {item.inUse === 1 ? "file" : "files"} in use{#if item.inUseBytes > 0 && item.inUseBytes !== item.remaining} ({formatSize(item.inUseBytes)}){/if}
                        </span>
                      {/if}
                      {#if item.otherSkipped > 0}
                        <span>{item.otherSkipped.toLocaleString()} {item.otherSkipped === 1 ? "other item" : "other items"} could not be removed</span>
                      {:else if item.remaining > item.inUseBytes && !item.systemFoldersSkipped}
                        <span>No deletion reason recorded for the rest</span>
                      {/if}
                      {#if item.systemFoldersSkipped}<span>System folders were outside this cleanup</span>{/if}
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          </details>
        {/if}
      {/if}
    </div>

    {#if summary}
      <div class="actions">
        <button class="btn" type="button" onclick={onscan}>Scan again</button>
        <button class="btn primary" type="button" onclick={onclose}>Done</button>
      </div>
    {/if}
  </dialog>
</div>

<style>
  @property --cleaner-reading {
    syntax: "<percentage>";
    inherits: false;
    initial-value: 0%;
  }

  .layer {
    position: fixed;
    z-index: 3;
    inset: 0;
    display: grid;
    place-items: center;
    grid-template-rows: minmax(0, 1fr);
    padding: clamp(10px, 3vmin, 22px);
    background: rgb(0 0 0 / 0.44);
  }
  :global(main) > .layer { position: absolute; border-radius: inherit; }
  dialog {
    /* The app opens at 1280, 1920 or 2560 physical pixels wide, before Windows DPI scaling. */
    --dialog-width: clamp(410px, 210px + 15.625vw, 610px);
    --dialog-gutter: clamp(16px, 7px + 0.7vw, 25px);
    --dialog-text: clamp(13px, 9px + 0.3125vw, 17px);
    --dialog-small: clamp(12px, 8px + 0.3125vw, 16px);
    --radar-size: clamp(62px, 24px + 3vw, 100px);
    position: relative;
    display: flex;
    flex-direction: column;
    width: min(100%, var(--dialog-width));
    max-width: none;
    max-height: 100%;
    margin: 0;
    padding: 0;
    color: var(--text-1);
    font-size: var(--dialog-text);
    overflow: hidden;
    animation: appear 140ms var(--ease-out) both;
  }
  .body {
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    padding: calc(var(--dialog-gutter) * 0.8125) var(--dialog-gutter);
  }
  header { display: flex; align-items: center; gap: 9px; }
  h2 { flex: 1; font-size: clamp(19px, 11px + 0.625vw, 27px); line-height: 1.15; overflow-wrap: anywhere; }
  h2:focus { outline: none; }
  .sound {
    flex: none;
    display: grid;
    place-items: center;
    width: calc(var(--dialog-gutter) * 1.875);
    height: calc(var(--dialog-gutter) * 1.875);
    border-radius: 7px;
    color: var(--text-2);
  }
  .sound:hover { background: var(--hover); color: var(--text-1); }
  .sound :global(svg) { width: 1.2em; height: 1.2em; }

  .instrument {
    --signal: 139 163 255;
    margin-top: var(--fluid-gap-3);
    border: 1px solid rgb(var(--signal) / 0.23);
    border-radius: 11px;
    background: var(--input-fill);
    overflow: hidden;
    transition: border-color 360ms var(--ease-out);
  }
  .completed { --signal: 180 194 212; }
  .success { --signal: 136 224 186; }
  .readout {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: center;
    gap: calc(var(--dialog-gutter) * 0.875);
    min-width: 0;
    padding: calc(var(--dialog-gutter) * 0.56) calc(var(--dialog-gutter) * 0.875);
    overflow: hidden;
    background: radial-gradient(ellipse at left center, rgb(var(--signal) / 0.12), transparent 73%);
  }
  .readout::before {
    content: "";
    position: absolute;
    z-index: -1;
    inset: 0;
    background: radial-gradient(circle, rgb(var(--signal) / 0.25) 1px, transparent 1.2px) 0 0 / 9px 9px;
    mask-image: linear-gradient(90deg, #000, transparent 85%);
    opacity: 0.5;
  }
  .readout::after {
    content: "";
    position: absolute;
    z-index: -1;
    inset: 0;
    background: linear-gradient(105deg, transparent 28%, rgb(var(--signal) / 0.21) 50%, transparent 66%);
    opacity: 0;
  }
  .instrument:not(.completed) .readout::after { animation: scan-pass 3.5s linear infinite; }
  .success .readout::after {
    animation: signal-pass 820ms var(--ease-out) both;
  }
  .radar {
    position: relative;
    isolation: isolate;
    display: grid;
    place-items: center;
    flex: none;
    width: var(--radar-size);
    height: var(--radar-size);
    border-radius: 50%;
    color: rgb(var(--signal));
    background: repeating-radial-gradient(circle closest-side, transparent 0 31%, rgb(var(--signal) / 0.15) 33% 35%, transparent 37% 45%);
  }
  .meter, .sweep {
    position: absolute;
    inset: 0;
    border-radius: 50%;
  }
  .meter {
    padding: clamp(2px, 0.115vw, 3px);
    background: conic-gradient(rgb(var(--signal)) var(--cleaner-reading), rgb(var(--signal) / 0.22) 0);
    mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
    mask-composite: exclude;
    transition: --cleaner-reading 360ms var(--ease-out);
  }
  .sweep {
    inset: 9%;
    background: conic-gradient(transparent 72%, rgb(var(--signal) / 0.44) 100%);
    animation: sweep 1.7s linear infinite;
  }
  .radar-icon {
    z-index: 1;
    display: grid;
    place-items: center;
    width: 56%;
    height: 56%;
    border: 1px solid rgb(var(--signal) / 0.29);
    border-radius: 50%;
    background: color-mix(in srgb, rgb(var(--signal)) 10%, var(--input-fill));
  }
  .radar-icon :global(svg) { width: 65%; height: 65%; }
  .success .radar::after {
    content: "";
    position: absolute;
    inset: 9%;
    border: 1px solid rgb(var(--signal) / 0.72);
    border-radius: 50%;
    animation: ripple 680ms var(--ease-out) both;
  }
  .success .radar-icon :global(svg) { animation: settle 320ms var(--ease-out) both; }
  .readout-content { min-width: 0; }
  .readout-label { display: block; color: var(--text-2); overflow-wrap: anywhere; }
  .reading { display: block; color: var(--text-1); font-size: clamp(29px, 15px + 1.1vw, 43px); line-height: 1.06; letter-spacing: -0.025em; font-variant-numeric: tabular-nums; }
  .progress { display: flex; flex-wrap: wrap; align-items: baseline; gap: 2px 9px; }
  .step { color: var(--text-3); font-size: var(--dialog-small); font-variant-numeric: tabular-nums; }

  .totals { display: flex; flex-wrap: wrap; margin: 0; border-top: 1px solid rgb(var(--signal) / 0.17); }
  .totals > div { flex: 1 1 106px; display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 0 8px; padding: 6px calc(var(--dialog-gutter) * 0.75); }
  .totals > div + div { border-inline-start: 1px solid rgb(var(--signal) / 0.17); }
  dt { color: var(--text-2); font-size: var(--dialog-small); }
  .category-result { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
  dd, .breakdown strong { flex: none; margin: 0; font-weight: 700; font-variant-numeric: tabular-nums; }
  .notice { display: flex; align-items: flex-start; gap: 7px; margin-top: 9px; color: var(--text-2); font-size: var(--dialog-small); }
  .notice :global(svg) { flex: none; margin-top: 2px; color: var(--warn); }

  details { margin-top: 9px; }
  summary {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    min-height: calc(var(--dialog-gutter) * 1.7);
    border-radius: 5px;
    padding: 1px 6px;
    list-style: none;
    color: var(--text-2);
  }
  summary::-webkit-details-marker { display: none; }
  summary:hover { background: var(--hover); color: var(--text-1); }
  details[open] summary :global(svg) { transform: rotate(90deg); }
  .breakdown { margin: 4px 0 0; padding: 0 6px; list-style: none; }
  .breakdown li { padding: 6px 3px; }
  .breakdown li + li { border-top: 1px solid var(--divider); }
  .category-result span { min-width: 0; overflow-wrap: anywhere; }
  .category-notes { display: flex; flex-wrap: wrap; gap: 2px 12px; margin-top: 2px; font-size: var(--dialog-small); color: var(--text-2); }
  .in-use { display: inline-flex; align-items: center; gap: 4px; color: var(--warn); }
  .in-use :global(svg) { flex: none; }

  .actions { display: flex; flex-wrap: wrap; justify-content: end; gap: 7px; padding: 8px var(--dialog-gutter) calc(var(--dialog-gutter) * 0.625); border-top: 1px solid var(--divider); }
  .actions .btn { height: clamp(32px, 20px + 0.9375vw, 44px); font-size: var(--dialog-text); }
  .actions .primary { min-width: 62px; }

  @keyframes appear { from { opacity: 0; transform: translateY(3px); } }
  @keyframes sweep { to { transform: rotate(1turn); } }
  @keyframes ripple { from { opacity: 0.85; transform: scale(0.75); } to { opacity: 0; transform: scale(1.9); } }
  @keyframes scan-pass {
    0% { opacity: 0; transform: translateX(-90%); }
    20% { opacity: 0.46; }
    72%, 100% { opacity: 0; transform: translateX(85%); }
  }
  @keyframes signal-pass { from { opacity: 0.8; transform: translateX(-80%); } to { opacity: 0; transform: translateX(80%); } }
  @keyframes settle { from { opacity: 0.5; transform: scale(0.78); } }
  @media (prefers-reduced-motion: reduce) {
    dialog, .sweep, .success .radar-icon :global(svg) { animation: none; }
    .sweep, .readout::after, .success .radar::after { display: none; }
    .instrument, .meter { transition: none; }
  }
</style>

