<script lang="ts">
  import PageHeader from "../../lib/components/PageHeader.svelte";
  import type { PageDef } from "../pages/registry";

  let { page }: { page: PageDef } = $props();
</script>

<PageHeader title={page.label} subtitle={page.description} />
<p class="status" role="status">
  <span class="track" aria-hidden="true"></span>
  <span>Loading page…</span>
</p>

<div class="preview" class:panes={page.fill} aria-hidden="true">
  {#if page.fill}
    <div class="placeholder list">
      {#each [0, 1, 2, 3] as row (row)}
        <div class="row">
          <span class="square"></span>
          <span class="line"></span>
        </div>
      {/each}
    </div>
    <div class="placeholder details">
      <span class="line title"></span>
      <span class="line"></span>
      <span class="line short"></span>
    </div>
  {:else}
    {#each [0, 1, 2] as tile (tile)}
      <div class="placeholder card">
        <span class="square"></span>
        <span class="line title"></span>
        <span class="line"></span>
        <span class="line short"></span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-bottom: 14px;
    color: var(--text-2);
    font-size: 12px;
  }

  .track {
    width: 38px;
    height: 3px;
    overflow: hidden;
    border-radius: 999px;
    background: rgb(var(--accent-rgb) / 0.22);
  }

  .track::after {
    display: block;
    width: 44%;
    height: 100%;
    border-radius: inherit;
    background: rgb(var(--accent-soft-rgb));
    transform: translateX(65%);
    content: "";
  }

  .preview {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 200px), 1fr));
    gap: 12px;
  }

  .preview.panes {
    grid-template-columns: minmax(135px, 1fr) minmax(0, 1.6fr);
    min-height: 245px;
  }

  .placeholder {
    display: grid;
    align-content: start;
    gap: 10px;
    padding: 16px;
    border: 1px solid var(--glass-border);
    border-radius: var(--radius-md);
    background: var(--bg-solid-panel);
  }

  .list {
    gap: 0;
    padding-block: 6px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
  }

  .row + .row { border-top: 1px solid var(--glass-border); }

  .square,
  .line {
    display: block;
    background: rgb(var(--accent-soft-rgb) / 0.12);
  }

  .square {
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
  }

  .card .square {
    width: 38px;
    height: 38px;
    margin-bottom: 4px;
  }

  .line {
    width: 80%;
    max-width: 320px;
    height: 8px;
    border-radius: 4px;
  }

  .title {
    width: 52%;
    height: 12px;
  }

  .short { width: 62%; }
  .row:nth-child(even) .line { width: 58%; }

  @media (prefers-reduced-motion: no-preference) {
    .track::after { animation: sweep 950ms var(--ease-in-out) infinite; }
    .preview { animation: preview-in 160ms var(--ease-out) 70ms both; }
  }

  @keyframes sweep {
    from { transform: translateX(-100%); }
    to { transform: translateX(230%); }
  }

  @keyframes preview-in {
    from { opacity: 0; }
  }
</style>
