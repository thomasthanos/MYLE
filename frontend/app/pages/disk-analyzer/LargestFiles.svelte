<script lang="ts">
  // The largest files of the whole scan, with a name filter.
  import Cloud from "@lucide/svelte/icons/cloud";
  import File from "@lucide/svelte/icons/file";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import { contextActions } from "../../../lib/context-menu";
  import { formatSize, sizeOf, type FileView } from "./api";
  import { itemActions } from "./actions";
  import { colorOf } from "./palette";
  import { visibleRange } from "./rows";
  import { diskState as d } from "./state.svelte";

  const ROW = 40;
  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(400);
  let text = $state(d.filter);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const files = $derived(d.topFiles ?? []);
  const range = $derived(visibleRange(scrollTop, height, ROW, files.length));
  const focusedIndex = $derived(files.findIndex((file) => file.id === d.focused));
  const largest = $derived(files.length ? sizeOf(files[0], d.sizeMode) : 0);

  function onInput() {
    clearTimeout(timer);
    timer = setTimeout(() => d.setFilter(text), 250);
  }

  function open(file: FileView) {
    d.setView("folders");
    void d.showInTree(file.id);
    void d.zoomTo(file.parent);
  }

  function moveTo(index: number) {
    const file = files[index];
    if (!file || !viewport) return;
    d.select(file.id);
    const top = index * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + height) viewport.scrollTop = top + ROW - height;
  }

  function onKeydown(e: KeyboardEvent) {
    const at = focusedIndex;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      moveTo(Math.max(0, Math.min(files.length - 1, at + (e.key === "ArrowDown" ? 1 : -1))));
    } else if (e.key === "Enter" && files[at]) {
      e.preventDefault();
      open(files[at]);
    } else if (e.key === "Delete") {
      e.preventDefault();
      void d.recycle();
    }
  }

  function rowActions(target: HTMLElement) {
    const element = target.closest<HTMLElement>("[data-node]");
    const id = element ? Number(element.dataset.node) : d.focused;
    const file = files.find((f) => f.id === id);
    return file ? itemActions(file.id, { kind: file.kind, locked: file.protected }, "files") : [];
  }
</script>

<div class="files">
  <label class="search">
    <Search size={14} />
    <input class="input" type="search" placeholder="Filter by name" aria-label="Filter the largest files by name" bind:value={text} oninput={onInput} />
  </label>
  {#if d.topFiles === null}
    <p class="empty"><LoaderCircle size={14} class="spin" /> Finding the largest files…</p>
  {:else if !files.length}
    <p class="empty">{d.filter ? "No file matches that name." : "No files here."}</p>
  {:else}
    <div
      class="viewport"
      bind:this={viewport}
      bind:clientHeight={height}
      onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
      onkeydown={onKeydown}
      tabindex="0"
      role="listbox"
      aria-label="Largest files"
      aria-multiselectable="true"
      aria-activedescendant={focusedIndex >= 0 ? `da-file-${files[focusedIndex].id}` : undefined}
      {@attach contextActions(rowActions)}
    >
      <div class="spacer" style:height="{files.length * ROW}px">
        {#each files.slice(range.first, range.last) as file, i (file.id)}
          {@const size = sizeOf(file, d.sizeMode)}
          <!-- svelte-ignore a11y_click_events_have_key_events (the list handles the keys, aria-activedescendant points at the row) -->
          <div
            class="row"
            class:selected={d.selection.has(file.id)}
            class:focused={d.focused === file.id}
            style:transform="translateY({(range.first + i) * ROW}px)"
            id="da-file-{file.id}"
            role="option"
            aria-selected={d.selection.has(file.id)}
            tabindex="-1"
            data-node={file.id}
            title={file.path}
            onclick={(e) => d.select(file.id, e.ctrlKey || e.metaKey ? "toggle" : "only")}
            ondblclick={() => open(file)}
          >
            <span class="swatch" style:background={colorOf(file.rank)}>{#if file.kind === "cloud"}<Cloud size={12} />{:else}<File size={12} />{/if}</span>
            <span class="text">
              <span class="name">{file.name}</span>
              <span class="path">{file.path}</span>
            </span>
            <span class="bar" aria-hidden="true"><span style:width="{largest ? (size / largest) * 100 : 0}%"></span></span>
            <span class="size">{formatSize(size)}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .files { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: 8px; padding-top: 8px; }
  .search { display: flex; align-items: center; gap: 8px; margin: 0 10px; color: var(--text-3); }
  .search .input { flex: 1; min-width: 0; height: 30px; }
  .empty { display: flex; align-items: center; gap: 8px; padding: 18px 12px; color: var(--text-3); font-size: var(--fluid-xs); }
  .viewport { position: relative; flex: 1; min-height: 0; overflow: auto; outline: none; }
  .viewport:focus-visible { box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.45); }
  .spacer { position: relative; }
  .row { position: absolute; inset: 0 0 auto; display: grid; grid-template-columns: 22px minmax(0, 1fr) 90px 76px; align-items: center; gap: 10px; height: 40px; padding: 0 10px; font-size: var(--fluid-xs); user-select: none; }
  .row:hover { background: var(--hover); }
  .row.selected { background: var(--selected); }
  .viewport:focus-within .row.focused { box-shadow: inset 2px 0 0 var(--accent); }
  .swatch { display: grid; place-items: center; width: 22px; height: 22px; border-radius: 6px; color: rgb(12 14 20 / 0.75); }
  .text { display: grid; min-width: 0; line-height: 1.3; }
  .name, .path { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .name { color: var(--text-1); }
  .path { color: var(--text-3); font-size: 11px; }
  .bar { height: 6px; border-radius: 999px; background: rgb(255 255 255 / 0.05); overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: inherit; background: rgb(var(--accent-rgb) / 0.55); }
  .size { justify-self: end; color: var(--text-1); font-variant-numeric: tabular-nums; }
  @container (max-width: 520px) { .row { grid-template-columns: 22px minmax(0, 1fr) 76px; } .bar { display: none; } }
</style>
