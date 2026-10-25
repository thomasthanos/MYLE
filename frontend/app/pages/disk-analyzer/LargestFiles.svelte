<script lang="ts">
  // The largest files of the whole scan, with smart classification, size filtering and sort order.
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
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

  const ROW = 52;
  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(400);
  let text = $state(d.filter);
  let timer: ReturnType<typeof setTimeout> | undefined;

  // Sorting & Filtering controls
  let sortAsc = $state(false); // false = Largest first, true = Smallest first
  let sizeFilter = $state<"all" | "huge" | "large" | "medium">("all");

  const rawFiles = $derived(d.topFiles ?? []);

  const files = $derived.by(() => {
    let list = rawFiles;
    if (sizeFilter === "huge") {
      list = list.filter((f) => sizeOf(f, d.sizeMode) >= 5 * 1024 ** 3);
    } else if (sizeFilter === "large") {
      list = list.filter((f) => {
        const s = sizeOf(f, d.sizeMode);
        return s >= 1024 ** 3 && s < 5 * 1024 ** 3;
      });
    } else if (sizeFilter === "medium") {
      list = list.filter((f) => sizeOf(f, d.sizeMode) < 1024 ** 3);
    }
    if (sortAsc) {
      return [...list].reverse();
    }
    return list;
  });

  const range = $derived(visibleRange(scrollTop, height, ROW, files.length));
  const focusedIndex = $derived(files.findIndex((file) => file.id === d.focused));
  const largest = $derived(rawFiles.length ? sizeOf(rawFiles[0], d.sizeMode) : 0);

  interface SmartFileInfo {
    tag: string;
    tagColor: string;
    advice: string;
  }

  function classifyFile(name: string, path: string): SmartFileInfo | null {
    const lowerName = name.toLowerCase();
    const lowerPath = path.toLowerCase();

    if (lowerName === "pagefile.sys") {
      return {
        tag: "Windows Virtual RAM",
        tagColor: "#3b82f6",
        advice: "Αρχείο συστήματος (απαραίτητο)",
      };
    }
    if (lowerName === "hiberfil.sys") {
      return {
        tag: "Windows Hibernation",
        tagColor: "#f59e0b",
        advice: "Αδρανοποίηση (powercfg -h off για διαγραφή)",
      };
    }
    if (lowerName === "swapfile.sys") {
      return {
        tag: "Windows App Swap",
        tagColor: "#3b82f6",
        advice: "Εικονική μνήμη εφαρμογών Windows",
      };
    }
    if (lowerPath.includes("mobilesync\\backup")) {
      return {
        tag: "iPhone / iPad Backup",
        tagColor: "#a855f7",
        advice: "Τοπικό Apple Backup (ασφαλές για διαγραφή αν είναι παλιό)",
      };
    }
    if (lowerPath.includes("bluestacks") || lowerName.endsWith(".vhdx") || lowerName.endsWith(".vmdk")) {
      return {
        tag: "Virtual Disk / Emulator",
        tagColor: "#06b6d4",
        advice: "Εικονικός δίσκος BlueStacks / VM",
      };
    }
    if (lowerPath.includes("vortex\\downloads") || lowerPath.includes("modorganizer")) {
      return {
        tag: "Mod Installer Archive",
        tagColor: "#10b981",
        advice: "Αρχείο λήψης mod (διαγράφεται αν έχει ήδη εγκατασταθεί)",
      };
    }
    if (lowerPath.includes("riot games") || lowerPath.includes("steam") || lowerPath.includes("epic games")) {
      return {
        tag: "Game Asset",
        tagColor: "#ec4899",
        advice: "Αρχείο παιχνιδιού (απαραίτητο)",
      };
    }
    if (lowerPath.includes("content_cache") || lowerPath.includes("dxcache") || lowerPath.includes("\\temp\\")) {
      return {
        tag: "Cache / Temp",
        tagColor: "#64748b",
        advice: "Προσωρινό αρχείο cache (ασφαλές)",
      };
    }
    if (lowerName.endsWith(".iso") || lowerName.endsWith(".img")) {
      return {
        tag: "Disk Image (ISO)",
        tagColor: "#3b82f6",
        advice: "Αρχείο εγκατάστασης ISO",
      };
    }
    return null;
  }

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
  <!-- Search & Filter Controls Bar -->
  <div class="controls-bar">
    <label class="search">
      <Search size={14} />
      <input class="input" type="search" placeholder="Filter by name..." aria-label="Filter the files by name" bind:value={text} oninput={onInput} />
    </label>

    <div class="filter-actions">
      <!-- Sort Order Toggle -->
      <button
        type="button"
        class="sort-btn"
        class:active={sortAsc}
        onclick={() => sortAsc = !sortAsc}
        title={sortAsc ? "Εμφάνιση: Μικρά πρώτα (πάτησε για Μεγάλα πρώτα)" : "Εμφάνιση: Μεγάλα πρώτα (πάτησε για Μικρά πρώτα)"}
      >
        <ArrowUpDown size={13} />
        <span>{sortAsc ? "Μικρά πρώτα" : "Μεγάλα πρώτα"}</span>
      </button>

      <!-- Quick Size Filters -->
      <div class="size-filters" role="group" aria-label="Size filter">
        <button type="button" class="size-pill" class:active={sizeFilter === "all"} onclick={() => sizeFilter = "all"}>Όλα</button>
        <button type="button" class="size-pill" class:active={sizeFilter === "huge"} onclick={() => sizeFilter = "huge"}>&gt; 5 GB</button>
        <button type="button" class="size-pill" class:active={sizeFilter === "large"} onclick={() => sizeFilter = "large"}>1–5 GB</button>
        <button type="button" class="size-pill" class:active={sizeFilter === "medium"} onclick={() => sizeFilter = "medium"}>&lt; 1 GB</button>
      </div>
    </div>
  </div>

  {#if d.topFiles === null}
    <p class="empty"><LoaderCircle size={14} class="spin" /> Finding the largest files…</p>
  {:else if !files.length}
    <p class="empty">{d.filter ? "No file matches that name." : "No files match the selected filter."}</p>
  {:else}
    <div
      class="viewport"
      bind:this={viewport}
      bind:clientHeight={height}
      onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
      onkeydown={onKeydown}
      tabindex="0"
      role="listbox"
      aria-label="Files list"
      aria-multiselectable="true"
      aria-activedescendant={focusedIndex >= 0 ? `da-file-${files[focusedIndex].id}` : undefined}
      {@attach contextActions(rowActions)}
    >
      <div class="spacer" style:height="{files.length * ROW}px">
        {#each files.slice(range.first, range.last) as file, i (file.id)}
          {@const size = sizeOf(file, d.sizeMode)}
          {@const info = classifyFile(file.name, file.path)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
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
            onclick={(e) => d.select(file.id, e.ctrlKey || e.metaKey ? "toggle" : "only")}
            ondblclick={() => open(file)}
          >
            <span class="swatch" style:background={colorOf(file.rank)}>
              {#if file.kind === "cloud"}<Cloud size={13} />{:else}<File size={13} />{/if}
            </span>
            <div class="text">
              <div class="name-line">
                <span class="name">{file.name}</span>
                {#if info}
                  <span class="smart-badge" style:color={info.tagColor} style:border-color="{info.tagColor}40" style:background="{info.tagColor}18">
                    {info.tag}
                  </span>
                {/if}
              </div>
              <span class="path">
                {#if info?.advice}
                  <strong class="advice">{info.advice}</strong> ·
                {/if}
                {file.path}
              </span>
            </div>
            <span class="bar" aria-hidden="true">
              <span style:width="{largest ? (size / largest) * 100 : 0}%"></span>
            </span>
            <span class="size">{formatSize(size)}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .files { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: 8px; padding-top: 6px; }

  .controls-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 0 10px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 180px;
    color: var(--text-3);
  }
  .search .input { flex: 1; min-width: 0; height: 32px; border-radius: 8px; }

  .filter-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .sort-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border-radius: 7px;
    border: 1px solid var(--btn-border);
    background: var(--chip-fill);
    color: var(--text-2);
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .sort-btn:hover {
    background: var(--chip-fill-hover);
    color: var(--text-1);
  }
  .sort-btn.active {
    background: var(--selected);
    border-color: rgba(59, 130, 246, 0.5);
    color: var(--text-1);
  }

  .size-filters {
    display: inline-flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--input-fill);
    border: 1px solid var(--btn-border);
    gap: 2px;
  }
  .size-pill {
    padding: 3px 8px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-3);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .size-pill:hover {
    color: var(--text-1);
  }
  .size-pill.active {
    background: var(--selected);
    color: var(--text-1);
    box-shadow: var(--btn-sheen);
  }

  .empty { display: flex; align-items: center; gap: 8px; padding: 18px 12px; color: var(--text-3); font-size: var(--fluid-xs); }
  .viewport { position: relative; flex: 1; min-height: 0; overflow: auto; outline: none; }
  .viewport:focus-visible { box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.45); }
  .spacer { position: relative; }

  .row {
    position: absolute;
    inset: 0 0 auto;
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) 90px 80px;
    align-items: center;
    gap: 12px;
    height: 52px;
    padding: 0 12px;
    font-size: var(--fluid-xs);
    user-select: none;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    transition: background 0.1s;
  }
  .row:hover { background: var(--hover); }
  .row.selected { background: var(--selected); }
  .viewport:focus-within .row.focused { box-shadow: inset 3px 0 0 var(--accent); }

  .swatch {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    color: rgb(12 14 20 / 0.85);
    flex-shrink: 0;
  }

  .text {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
    gap: 2px;
  }
  .name-line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    color: var(--text-1);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .smart-badge {
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    border: 1px solid;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .path {
    overflow: hidden;
    color: var(--text-3);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .advice {
    color: #e2e8f0;
    font-weight: 500;
  }

  .bar { height: 6px; border-radius: 999px; background: rgb(255 255 255 / 0.05); overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: inherit; background: linear-gradient(90deg, var(--accent), #38bdf8); }
  .size { justify-self: end; color: var(--text-1); font-weight: 600; font-variant-numeric: tabular-nums; font-size: 12px; }

  @container (max-width: 540px) {
    .row { grid-template-columns: 24px minmax(0, 1fr) 76px; }
    .bar { display: none; }
  }
</style>
