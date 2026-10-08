<script lang="ts">
  import type { Component } from "svelte";

  // One number of the page summary. Wide enough for its own words, so every
  // tile reads on its own and the row wraps instead of squeezing.
  let {
    label,
    value,
    icon,
    tone = "quiet",
    title = "",
  }: {
    label: string;
    value: string;
    /** A Lucide icon: better than the dot when the number is not a state. */
    icon?: Component<{ size?: number | string; strokeWidth?: number | string }>;
    tone?: "quiet" | "ok" | "warn" | "danger" | "accent";
    title?: string;
  } = $props();
</script>

<div class="stat {tone}" {title}>
  <span class="label">
    {#if icon}
      <icon size={13} strokeWidth={2}></icon>
    {:else}
      <span class="dot" aria-hidden="true"></span>
    {/if}
    {label}
  </span>
  <strong>{value}</strong>
</div>

<style>
  .stat {
    display: grid;
    gap: 2px;
    min-width: 118px;
    padding: 8px 13px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 11px;
    background: linear-gradient(180deg, rgb(255 255 255 / 0.035), rgb(255 255 255 / 0.012));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.05);
  }

  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    color: var(--text-3);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .label :global(svg) {
    flex: none;
    color: rgb(var(--accent-soft-rgb) / 0.8);
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--idle);
  }

  .stat.ok .dot {
    background: var(--ok);
    box-shadow: 0 0 8px var(--ok-glow);
  }

  .stat.warn .dot {
    background: #f5bc5f;
    box-shadow: 0 0 8px rgb(245 188 95 / 0.45);
  }

  .stat.danger .dot {
    background: var(--danger);
    box-shadow: 0 0 8px rgb(229 72 77 / 0.5);
  }

  .stat.accent .dot {
    background: var(--accent);
    box-shadow: 0 0 8px var(--accent-glow);
  }

  strong {
    color: var(--text-1);
    font-size: 17px;
    font-weight: 600;
    line-height: 1.15;
    letter-spacing: -0.01em;
    font-variant-numeric: tabular-nums;
  }

  .stat.ok strong {
    color: #a8e8c8;
  }

  .stat.warn strong {
    color: rgb(245 188 95 / 0.95);
  }

  .stat.danger strong {
    color: rgb(255 145 145 / 0.95);
  }
</style>
