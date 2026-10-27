<script lang="ts">
  // Files by extension in the folder the treemap shows, each in the colour of its kind, as in the treemap.
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { formatSize, sizeOf } from "./api";
  import { settings } from "../../../lib/settings.svelte";
  import { categoryColor, categoryLabel, categoryOf } from "./palette";
  import { diskState as d } from "./state.svelte";

  const stats = $derived(d.extensions ?? []);
  const total = $derived(stats.reduce((sum, stat) => sum + sizeOf(stat, d.sizeMode), 0));
  const where = $derived(d.trail.at(-1)?.name ?? "");
</script>

<div class="types">
  <p class="where">In <strong title={where}>{where}</strong></p>
  {#if d.extensions === null}
    <p class="empty"><LoaderCircle size={14} class="spin" /> Counting file types…</p>
  {:else if !stats.length}
    <p class="empty">No files here.</p>
  {:else}
    <table>
      <thead>
        <tr><th scope="col">Type</th><th scope="col" class="share-col"><span class="sr">Share</span></th><th scope="col">{d.sizeMode === "alloc" ? "Size on disk" : "Size"}</th><th scope="col">Files</th></tr>
      </thead>
      <tbody>
        {#each stats as stat (stat.other ? "*other" : stat.ext)}
          {@const size = sizeOf(stat, d.sizeMode)}
          {@const part = total ? size / total : 0}
          {@const kind = stat.other ? "other" : categoryOf(stat.ext)}
          {@const color = categoryColor(kind, settings.dark)}
          <tr>
            <td class="type">
              <span class="swatch" style:background={color} title={categoryLabel(kind)} aria-hidden="true"></span>
              {#if stat.other}Other types{:else if stat.ext}.{stat.ext}{:else}No extension{/if}
            </td>
            <td class="share" aria-label="{Math.round(part * 100)}%"><span class="bar"><span style:width="{part * 100}%" style:background={color}></span></span><em>{Math.round(part * 100)}%</em></td>
            <td class="num" title="Size: {formatSize(stat.size)} · On disk: {formatSize(stat.alloc)}">{formatSize(size)}</td>
            <td class="num dim">{stat.count.toLocaleString()}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .types { flex: 1; min-height: 0; overflow: auto; padding: 8px 10px 10px; }
  .where { overflow: hidden; margin-bottom: 8px; color: var(--text-3); font-size: var(--fluid-xs); white-space: nowrap; text-overflow: ellipsis; }
  .where strong { color: var(--text-2); font-weight: 600; }
  .empty { display: flex; align-items: center; gap: 8px; padding: 10px 2px; color: var(--text-3); font-size: var(--fluid-xs); }
  table { width: 100%; border-collapse: collapse; font-size: var(--fluid-xs); }
  th { padding: 5px 6px; border-bottom: 1px solid var(--glass-border); color: var(--text-3); font-weight: 600; text-align: right; }
  th:first-child { text-align: left; }
  td { padding: 6px; border-bottom: 1px solid rgb(255 255 255 / 0.04); }
  .type { display: flex; align-items: center; gap: 8px; color: var(--text-1); white-space: nowrap; }
  .swatch { flex: none; width: 11px; height: 11px; border-radius: 3px; }
  .share { width: 38%; }
  .share-col { width: 38%; }
  .share { position: relative; }
  .share .bar { display: block; height: 8px; margin-right: 38px; border-radius: 999px; background: var(--hover); overflow: hidden; }
  .share .bar span { display: block; height: 100%; border-radius: inherit; }
  .share em { position: absolute; top: 50%; right: 6px; translate: 0 -50%; color: var(--text-3); font-size: 10.5px; font-style: normal; font-variant-numeric: tabular-nums; }
  .num { color: var(--text-1); text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .num.dim { color: var(--text-3); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
