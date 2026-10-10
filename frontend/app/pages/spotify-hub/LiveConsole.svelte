<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Copy from "@lucide/svelte/icons/copy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import SquareStop from "@lucide/svelte/icons/square-stop";
  import Terminal from "@lucide/svelte/icons/terminal";
  import { toast } from "../../../lib/toast.svelte";
  import type { SpotifyHubAction } from "./api";
  import { spotifyHubState as hub } from "./state.svelte";

  let { action }: { action: SpotifyHubAction } = $props();
  const uid = $props.id();
  const consoleId = `${uid}-spotify-console`;

  const buffer = $derived(hub.consoles[action]);
  const lines = $derived(buffer.lines);
  const output = $derived(lines.join("\n"));
  const active = $derived(hub.activeAction === action);
  const stage = $derived(hub.stageLabel(action));

  let scroller = $state<HTMLDivElement>();
  let pinned = $state(true);

  function onScroll() {
    if (!scroller) return;
    pinned = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 24;
  }

  $effect(() => {
    void lines.length;
    if (scroller && pinned) scroller.scrollTop = scroller.scrollHeight;
  });

  async function copy() {
    try {
      await navigator.clipboard.writeText(output);
      toast.success("Console output copied.");
    } catch {
      toast.error("Could not copy the console output.");
    }
  }

  function jumpToLatest() {
    pinned = true;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }
</script>

<div class="console">
  <div class="toolbar">
    <button
      class="toggle"
      aria-expanded={buffer.open}
      aria-controls={consoleId}
      onclick={() => hub.toggleConsole(action)}
    >
      <Terminal size={14} />
      <span>Live console</span>
      {#if lines.length}<span class="count">{lines.length}{buffer.dropped ? "+" : ""}</span>{/if}
      <ChevronDown size={14} class={buffer.open ? "flip" : undefined} />
    </button>

    {#if active}
      <span class="phase"><LoaderCircle size={13} class="spin" /> {stage ?? "Running"}…</span>
      {#if lines.length}
        <button class="icon-btn copy" title="Copy output" aria-label="Copy console output" onclick={copy}><Copy size={13} /></button>
      {/if}
      <button class="btn small danger" disabled={hub.stopping || !hub.activeJob} onclick={() => hub.cancel()}>
        <SquareStop size={13} />
        {hub.stopping ? "Stopping…" : "Stop"}
      </button>
    {:else if lines.length}
      <button class="btn small ghost" onclick={copy}><Copy size={13} /> Copy</button>
    {/if}
  </div>

  <div id={consoleId} class="collapse" class:open={buffer.open} aria-hidden={!buffer.open} inert={!buffer.open}>
    <div class="inner">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard users need to focus the overflow region) -->
      <div
        class="scroll"
        bind:this={scroller}
        onscroll={onScroll}
        role="region"
        aria-label="Scrollable live console output"
        tabindex="0"
      >
        <div role="log" aria-live="polite" aria-atomic="false" aria-busy={active}>
          {#if buffer.dropped}
            <p class="trimmed">…{buffer.dropped.toLocaleString()} earlier lines hidden</p>
          {/if}
          {#if lines.length}
            <pre class="selectable">{output}</pre>
          {:else if active}
            <p class="empty">Waiting for output…</p>
          {:else}
            <p class="empty">Output will appear here.</p>
          {/if}
        </div>
      </div>
      {#if !pinned && active}
        <button class="jump btn small" onclick={jumpToLatest}>Jump to latest</button>
      {/if}
      {#if action === "purgeAll" && active}
        <p class="stop-note">Stop is best-effort between stages; completed removals cannot be undone.</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .console { margin-top: 14px; border-top: 1px solid rgb(255 255 255 / 0.065); }

  .toolbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-height: 38px; padding-top: 8px; }

  .toggle,
  .phase { display: inline-flex; align-items: center; gap: 7px; color: var(--text-2); font-size: 11.5px; }

  .toggle:hover { color: var(--text-1); }
  .toggle :global(svg:last-child) { transition: transform var(--dur-med) var(--ease-out); }
  .toggle :global(svg.flip) { transform: rotate(180deg); }

  .count { padding: 1px 6px; border-radius: 999px; background: rgb(255 255 255 / 0.07); color: var(--text-3); font-size: 10.5px; font-variant-numeric: tabular-nums; }

  .phase { margin-left: auto; color: var(--text-3); }
  .phase + .btn { margin-left: 0; }
  .phase + .copy { margin-left: 0; }
  .copy { width: 26px; height: 26px; }
  .toolbar > .btn { margin-left: auto; }

  .collapse { display: grid; grid-template-rows: 0fr; transition: grid-template-rows var(--dur-med) var(--ease-out); }

  .collapse.open { grid-template-rows: 1fr; }
  .inner { position: relative; min-height: 0; overflow: hidden; }

  .scroll { max-height: 210px; margin-top: 3px; padding: 10px 11px; overflow: auto; border: 1px solid rgb(255 255 255 / 0.055); border-radius: var(--radius-sm); background: rgb(0 0 0 / 0.3); box-shadow: inset 0 1px 3px rgb(0 0 0 / 0.35); }

  pre { margin: 0; color: var(--text-2); font-family: var(--font-mono); font-size: 10.5px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }

  .empty,
  .trimmed,
  .stop-note { color: var(--text-3); font-size: 10.5px; }

  .empty { font-family: var(--font-mono); }
  .trimmed { margin-bottom: 5px; font-style: italic; }
  .stop-note { margin-top: 7px; color: rgb(245 188 95 / 0.78); }
  .jump { position: absolute; right: 10px; bottom: 10px; }
</style>
