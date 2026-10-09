<script lang="ts">
  // A login's 2FA code, counting down to the next one. The key stays in the
  // app; only the code comes here, asked again when it runs out.
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { onDestroy, onMount } from "svelte";
  import { passwordsApi as api, type TotpCode } from "./api";
  import { passwords as p } from "./state.svelte";

  let { id }: { id: string } = $props();

  let current = $state<TotpCode | null>(null);
  let failed = $state(false);
  /** Seconds left, counted here between the app's answers. */
  let left = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;

  async function load() {
    try {
      current = await api.totp(id);
      left = current.remaining;
      failed = false;
    } catch {
      failed = true;
    }
  }

  onMount(() => {
    void load();
    timer = setInterval(() => {
      if (!current) return;
      left -= 1;
      if (left <= 0) void load();
    }, 1000);
  });
  onDestroy(() => clearInterval(timer));

  /** "123 456", or "1234 5678" for eight digits. */
  const shown = $derived(current ? current.code.replace(/^(\d{3,4})(\d{3,4})$/, "$1 $2") : "··· ···");
  const fraction = $derived(current ? Math.max(0, left) / current.period : 1);
  const soon = $derived(left <= 5);
</script>

<div class="row">
  <dt>2FA code</dt>
  <dd class="code" class:soon aria-live="polite">
    {#if failed}<span class="failed">Could not make the code</span>{:else}{shown}{/if}
  </dd>
  <span class="ring" class:soon title="{Math.max(0, left)} seconds left" aria-hidden="true">
    <svg viewBox="0 0 20 20"><circle class="track" cx="10" cy="10" r="8" /><circle class="left" cx="10" cy="10" r="8" style:stroke-dashoffset={50.27 * (1 - fraction)} /></svg>
    <small>{Math.max(0, left)}</small>
  </span>
  <button class="icon-btn" class:done={p.copied === `${id}:totp`} title="Copy" aria-label="Copy 2FA code" disabled={!current} onclick={() => p.copy(id, "totp")}>
    {#if p.copied === `${id}:totp`}<Check size={14} />{:else}<Copy size={14} />{/if}
  </button>
</div>

<style>
  .icon-btn.done {
    color: var(--ok);
  }

  /* Lays out like the entry's other rows (EntryView's .row). */
  .row {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 6px;
    min-height: 42px;
    padding: 0 4px 0 12px;
    border-radius: 10px;
  }

  .row:hover {
    background: rgb(255 255 255 / 0.035);
  }

  dt {
    color: var(--text-3);
    font-size: 12px;
  }

  dd {
    margin: 0;
  }

  .code {
    color: #c9d0ff;
    font-family: var(--font-mono);
    font-size: 17px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.08em;
    transition: color var(--dur-med);
  }

  .code.soon {
    color: #ffcf8f;
  }

  .failed {
    color: var(--text-3);
    font-family: var(--font-sans);
    font-size: 12px;
    letter-spacing: 0;
  }

  .ring {
    position: relative;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
  }

  .ring svg {
    position: absolute;
    inset: 0;
    transform: rotate(-90deg);
  }

  circle {
    fill: none;
    stroke-width: 2.2;
  }

  .track {
    stroke: rgb(255 255 255 / 0.09);
  }

  .left {
    stroke: rgb(var(--accent-rgb));
    stroke-dasharray: 50.27;
    stroke-linecap: round;
    transition: stroke-dashoffset 1s linear, stroke var(--dur-med);
  }

  .ring.soon .left {
    stroke: #ffb35c;
  }

  .ring small {
    color: var(--text-3);
    font-size: 9.5px;
    font-variant-numeric: tabular-nums;
  }

  /* A narrow panel: the label above the code, as in the other rows. */
  @container entry (max-width: 420px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto auto;
      padding-top: 6px;
      padding-bottom: 6px;
    }

    dt {
      grid-column: 1 / -1;
    }
  }

  @media (max-height: 1000px) {
    .row {
      min-height: 36px;
    }
  }
</style>
