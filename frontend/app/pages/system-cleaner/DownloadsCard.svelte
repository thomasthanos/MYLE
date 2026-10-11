<script lang="ts">
  import FolderDown from "@lucide/svelte/icons/folder-down";
  import { formatSize } from "./api";
  import type { Kept } from "./downloads";
  import { cleanerState } from "./state.svelte";

  let { embedded = false }: { embedded?: boolean } = $props();

  const preview = $derived(cleanerState.downloads);
  const when = (seconds: number) => new Date(seconds * 1000).toLocaleDateString();
  const why = (k: Kept) =>
    k.reason === "document" ? "Document" : k.reason === "recent" ? `Modified ${when(k.changed)}, in the last 7 days` : "Link or junction";
</script>

{#if preview}
  <section class="downloads" class:surface={!embedded} class:embedded>
    <header>
      {#if !embedded}<span class="icon"><FolderDown size={16} /></span>{/if}
      <span class="text">
        {#if !embedded}<strong>Downloads folder, in detail</strong>{/if}
        <small>{preview.folder} · {preview.items.length} to delete ({formatSize(preview.total)}), permanently · {preview.kept.length} kept</small>
      </span>
    </header>
    {#if preview.items.length}
      <details class="kept">
        <summary>{preview.items.length} to delete</summary>
        <ul class="list">
          {#each preview.items as item (item.path)}
            <li><span class="row"><span class="name" title={item.path}>{item.name}{item.isDir ? "\\" : ""}</span><span class="date">{when(item.changed)} · {formatSize(item.size)}</span></span></li>
          {/each}
        </ul>
      </details>
    {/if}
    {#if preview.kept.length}
      <details class="kept">
        <summary>{preview.kept.length} kept, and why</summary>
        <ul class="list">
          {#each preview.kept as k (k.name)}
            <li><span class="row"><span class="name">{k.name}{k.isDir ? "\\" : ""}</span><span class="date">{why(k)}</span></span></li>
          {/each}
        </ul>
      </details>
    {/if}
  </section>
{/if}

<style>
  .downloads.embedded { padding: 10px 0 0; margin-top: 0; }
  .embedded .text small { overflow-wrap: anywhere; }
  .downloads {
    display: grid;
    gap: var(--fluid-gap-2);
    margin-top: var(--fluid-gap-3);
    padding: var(--fluid-gap-3) var(--fluid-gap-4);
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: var(--fluid-icon);
    height: var(--fluid-icon);
    border-radius: 9px;
    background: rgb(var(--accent-rgb) / 0.14);
    color: rgb(var(--accent-rgb));
  }

  .text {
    display: grid;
    flex: 1;
    gap: 2px;
  }

  .text strong {
    font-size: var(--fluid-md);
  }

  .text small,
  .date {
    color: var(--text-3);
    font-size: var(--fluid-xs);
  }



  .list {
    max-height: 260px;
    margin: 0;
    padding: 4px;
    overflow: auto;
    border: 1px solid var(--btn-border);
    border-radius: 10px;
    list-style: none;
  }



  .row {
    display: flex;
    gap: 10px;
    justify-content: space-between;
    padding: 4px 6px;
    font-size: var(--fluid-sm);
  }

  .kept summary {
    color: var(--text-2);
    font-size: var(--fluid-xs);
    cursor: pointer;
  }

  .kept .list {
    margin-top: 6px;
  }




  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

</style>
