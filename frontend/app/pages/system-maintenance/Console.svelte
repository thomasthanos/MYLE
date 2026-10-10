<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Copy from "@lucide/svelte/icons/copy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import SquareStop from "@lucide/svelte/icons/square-stop";
  import Terminal from "@lucide/svelte/icons/terminal";
  import { toast } from "../../../lib/toast.svelte";
  import type { MaintenanceCard } from "./api";
  import { maintenanceState as s } from "./state.svelte";

  let { card }: { card: MaintenanceCard } = $props();

  const buffer = $derived(s.consoles[card.id]);
  const lines = $derived(buffer?.lines ?? []);
  const text = $derived(lines.join("\n"));
  const open = $derived(buffer?.open ?? false);
  const active = $derived(card.actions.find((a) => s.isRunning(a.id)));
  const status = $derived(s.statusOf(card));

  let box = $state<HTMLDivElement>();
  let pinned = $state(true);

  function onScroll() {
    if (!box) return;
    // A little slack: the scroll position rarely lands exactly at the end.
    pinned = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
  }

  // Runs after the new lines are in the DOM.
  $effect(() => {
    void lines.length;
    if (box && pinned) box.scrollTop = box.scrollHeight;
  });

  function jump() {
    pinned = true;
    if (box) box.scrollTop = box.scrollHeight;
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Output copied.");
    } catch {
      toast.error("Could not copy the output.");
    }
  }
</script>

<div class="console" class:expanded={open}>
  <div class="bar">
    <button class="toggle" aria-expanded={open} onclick={() => s.toggleConsole(card.id)}>
      <Terminal size={14} />
      <span>Output</span>
      {#if lines.length}<span class="count">{lines.length}{buffer?.dropped ? "+" : ""}</span>{/if}
      <ChevronDown size={14} class={open ? "flip" : undefined} />
    </button>

    {#if active}
      <span class="phase">
        <LoaderCircle size={13} class="spin" />
        {status.label}
      </span>
      {#if active.cancellable}
        <button class="btn small danger" disabled={s.stopping} onclick={() => s.cancel(card, active)}>
          <SquareStop size={13} />
          {s.stopping ? "Stopping…" : "Stop"}
        </button>
      {/if}
    {:else if lines.length}
      <button class="btn small ghost" onclick={copy}>
        <Copy size={13} />
        Copy
      </button>
    {/if}
  </div>

  <div class="wrap" class:open>
    <div>
      <div class="scroll" bind:this={box} onscroll={onScroll}>
        {#if buffer?.dropped}
          <p class="trimmed">…{buffer.dropped.toLocaleString()} earlier lines hidden</p>
        {/if}
        {#if lines.length}
          <pre class="selectable">{text}</pre>
        {:else if active}
          <p class="idle">Waiting for the first output. Some tools print nothing until they finish.</p>
        {:else}
          <p class="idle">Output appears here once it starts.</p>
        {/if}
      </div>
      {#if !pinned && active}
        <button class="jump btn small" onclick={jump}>Jump to latest</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .console { margin-top: 2px; overflow: hidden; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 10px; background: rgb(0 0 0 / 0.1); }

  .bar { display: flex; align-items: center; gap: 10px; min-height: 38px; padding: 5px 8px 5px 10px; transition: border-color var(--dur-fast); }

  .console.expanded .bar { border-bottom: 1px solid rgb(255 255 255 / 0.055); }

  .toggle { display: flex; align-items: center; gap: 7px; color: var(--text-2); font-size: 12.5px; font-weight: 500; }

  .toggle:hover { color: var(--text-1); }

  .toggle :global(.flip) { transform: rotate(180deg); }

  .toggle :global(svg:last-child) { transition: transform var(--dur-med) var(--ease-out); }

  .count { padding: 1px 6px; border-radius: 999px; background: rgb(255 255 255 / 0.07); color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; }

  .phase { display: flex; align-items: center; gap: 6px; margin-left: auto; color: var(--text-3); font-size: 11.5px; }

  .bar .btn { margin-left: auto; }

  .phase + .btn { margin-left: 0; }

  /* Collapses without needing to know the content height. */
  .wrap { display: grid; grid-template-rows: 0fr; transition: grid-template-rows var(--dur-med) var(--ease-out); }

  .wrap.open { grid-template-rows: 1fr; }

  .wrap > div { position: relative; min-height: 0; overflow: hidden; }

  .scroll { max-height: 260px; padding: 12px 14px; overflow: auto; background: rgb(5 8 14 / 0.48); box-shadow: inset 0 2px 5px rgb(0 0 0 / 0.22); }

  pre { margin: 0; color: var(--text-2); font-family: var(--font-mono); font-size: 11.5px; line-height: 1.55; white-space: pre-wrap; word-break: break-word; }

  .idle { color: var(--text-3); font-family: var(--font-mono); font-size: 11.5px; }

  .trimmed { margin-bottom: 6px; color: var(--text-3); font-size: 11px; font-style: italic; }

  .jump { position: absolute; right: 12px; bottom: 10px; }

  :global(:root.solid) .wrap { transition: none; }
</style>
