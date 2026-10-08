<script lang="ts">
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import type { Preview } from "./api";
  import { formatBytes } from "./state.svelte";

  let { preview }: { preview: Preview } = $props();
</script>

<div class="preview surface">
  <p class="summary">
    <strong>{preview.fileCount.toLocaleString()}</strong> files · <strong>{formatBytes(preview.totalBytes)}</strong> backed up ·
    <strong>{preview.excludedTotal.toLocaleString()}</strong> left out
    {#if preview.envFiles.length}· includes {preview.envFiles.join(", ")}{/if}
  </p>
  {#if preview.skipped.length}
    <p class="skipped">
      Can't be zipped: {preview.skipped.map((item) => `${item.path} (${item.reason})`).join(", ")}
    </p>
  {/if}
  {#if preview.excluded.length}
    <ul>
      {#each preview.excluded as item (item.path)}
        <li>
          {#if item.isDir}<Folder size={12} />{:else}<File size={12} />{/if}
          <span class="path selectable">{item.path}{item.isDir ? "/" : ""}</span>
          <span class="rule">{item.rule}</span>
        </li>
      {/each}
    </ul>
    {#if preview.excludedTotal > preview.excluded.length}
      <p class="more">and {(preview.excludedTotal - preview.excluded.length).toLocaleString()} more</p>
    {/if}
  {/if}
</div>

<style>
  .preview { display: grid; gap: 6px; width: 100%; padding: 9px 10px; }
  .summary { color: var(--text-2); font-size: 11.5px; line-height: 1.5; overflow-wrap: anywhere; }
  .summary strong { color: var(--text-1); font-variant-numeric: tabular-nums; }
  .skipped { color: rgb(245 188 95 / 0.85); font-size: 11px; overflow-wrap: anywhere; }
  ul { display: grid; gap: 1px; max-height: 180px; margin: 0; padding: 0; overflow: auto; list-style: none; }
  li { display: flex; align-items: center; gap: 6px; min-width: 0; color: var(--text-3); font-size: 11px; }
  li :global(svg) { flex: none; }
  .path { min-width: 0; overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
  .rule { margin-left: auto; flex: none; padding: 0 5px; border-radius: 4px; background: rgb(255 255 255 / 0.05); font-family: var(--font-mono); font-size: 10px; }
  .more { color: var(--text-3); font-size: 10.5px; }
</style>
