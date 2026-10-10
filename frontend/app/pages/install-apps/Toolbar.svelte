<script lang="ts">
  import ArrowDownUp from "@lucide/svelte/icons/arrow-down-up";
  import Boxes from "@lucide/svelte/icons/boxes";
  import Check from "@lucide/svelte/icons/check";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import List from "@lucide/svelte/icons/list";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import Popover from "../../../lib/components/Popover.svelte";
  import { appsState, type Filter, type Sort } from "./state.svelte";

  const filters: { id: Filter; label: string; dot?: string }[] = [
    { id: "all", label: "All" },
    { id: "installed", label: "Installed", dot: "installed" },
    { id: "updates", label: "Updates", dot: "update" },
    { id: "missing", label: "Not installed", dot: "missing" },
  ];
  const sorts: { id: Sort; label: string }[] = [
    { id: "category", label: "Category" },
    { id: "az", label: "Name A–Z" },
    { id: "za", label: "Name Z–A" },
    { id: "status", label: "Status" },
  ];

  const c = $derived(appsState.counts);
  const count = (f: Filter) => (f === "all" ? c.all : f === "installed" ? c.installed : f === "updates" ? c.updates : c.missing);
  const sortLabel = $derived(sorts.find((s) => s.id === appsState.sort)?.label);
  const activePack = $derived(appsState.packs.find((p) => p.id === appsState.activePack));
  const total = $derived(appsState.shownCount);
</script>

<div class="toolbar">
  <div class="row">
    <label class="search">
      <span class="search-icon"><Search size={15} /></span>
      <input
        class="input"
        type="search"
        placeholder="Search apps or the winget catalog…"
        spellcheck="false"
        autocomplete="off"
        value={appsState.query}
        oninput={(e) => appsState.setQuery(e.currentTarget.value)}
      />
      {#if appsState.query}
        <button class="clear icon-btn" aria-label="Clear search" onclick={() => appsState.setQuery("")}>
          <X size={14} />
        </button>
      {/if}
    </label>

    <Popover align="end">
      {#snippet trigger({ toggle, open })}
        <button class="btn" class:open aria-haspopup="menu" aria-expanded={open} onclick={toggle}>
          <ArrowDownUp size={15} />
          {sortLabel}
        </button>
      {/snippet}
      {#snippet children({ close })}
        <div class="menu" role="menu">
          <div class="menu-label">Sort by</div>
          {#each sorts as s (s.id)}
            <button
              class="menu-item"
              class:active={appsState.sort === s.id}
              role="menuitemradio"
              aria-checked={appsState.sort === s.id}
              onclick={() => {
                appsState.setSort(s.id);
                close();
              }}
            >
              {s.label}
              {#if appsState.sort === s.id}<span class="hint"><Check size={14} /></span>{/if}
            </button>
          {/each}
        </div>
      {/snippet}
    </Popover>

    <div class="segmented" role="group" aria-label="View">
      <button
        class="icon-btn"
        class:active={appsState.view === "grid"}
        aria-pressed={appsState.view === "grid"}
        title="Grid view"
        onclick={() => appsState.setView("grid")}
      >
        <LayoutGrid size={16} />
      </button>
      <button
        class="icon-btn"
        class:active={appsState.view === "list"}
        aria-pressed={appsState.view === "list"}
        title="List view"
        onclick={() => appsState.setView("list")}
      >
        <List size={16} />
      </button>
    </div>

    <Popover align="end">
      {#snippet trigger({ toggle, open })}
        <button class="btn" class:open aria-haspopup="menu" aria-expanded={open} onclick={toggle}>
          <Boxes size={15} />
          App packs
        </button>
      {/snippet}
      {#snippet children({ close })}
        <div class="menu packs" role="menu">
          <div class="menu-label">Choose a pack</div>
          {#each appsState.packs as pack (pack.id)}
            <button
              class="menu-item pack"
              class:active={appsState.activePack === pack.id}
              role="menuitem"
              onclick={() => {
                appsState.applyPack(pack.id);
                close();
              }}
            >
              <span class="pack-text">
                <span class="pack-name">{pack.name}</span>
                <span class="pack-desc">{pack.description}</span>
              </span>
              <span class="hint">{pack.apps.length}</span>
            </button>
          {/each}
          {#if activePack}
            <button
              class="menu-item"
              role="menuitem"
              onclick={() => {
                appsState.applyPack(null);
                close();
              }}
            >
              Show all apps
            </button>
          {/if}
        </div>
      {/snippet}
    </Popover>

  </div>

  <div class="row">
    <div class="chips" role="group" aria-label="Filter">
      {#each filters as f (f.id)}
        <button class="chip" class:active={appsState.filter === f.id} aria-pressed={appsState.filter === f.id} onclick={() => appsState.setFilter(f.id)}>
          {#if f.dot}<span class="dot {f.dot}"></span>{/if}
          {f.label}
          <span class="count">{count(f.id)}</span>
        </button>
      {/each}
      {#if activePack}
        <button class="chip active" title="Show all apps" onclick={() => appsState.applyPack(null)}>
          Pack: {activePack.name}
          <X size={12} />
        </button>
      {/if}
    </div>

    <div class="meta">
      {#if appsState.checking}
        <span class="checking"><LoaderCircle size={14} class="spin" /> Checking installed apps…</span>
      {/if}
      <span class="badge">{total} {total === 1 ? "app" : "apps"}</span>
    </div>
  </div>
</div>

<style>
  .toolbar { display: grid; gap: 12px; margin-bottom: 22px; }

  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }

  .search { position: relative; flex: 1 1 260px; display: flex; align-items: center; }

  .search .input { width: 100%; padding-left: 34px; padding-right: 34px; }

  .search input::-webkit-search-cancel-button { display: none; }

  .search-icon { position: absolute; left: 12px; display: grid; color: var(--text-3); pointer-events: none; }

  .clear { position: absolute; right: 3px; width: 28px; height: 28px; }

  .btn.open { background: rgb(255 255 255 / 0.1); }

  .segmented { display: flex; gap: 2px; padding: 1px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 10px; background: rgb(0 0 0 / 0.15); }

  .segmented .icon-btn { width: 32px; height: 28px; }

  .chips { display: flex; flex-wrap: wrap; gap: 6px; }

  .meta { display: flex; align-items: center; gap: 12px; margin-left: auto; }

  .checking { display: inline-flex; align-items: center; gap: 6px; color: var(--text-3); font-size: 12px; }

  .badge { padding: 3px 10px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 999px; background: rgb(255 255 255 / 0.04); color: var(--text-2); font-size: 12px; font-variant-numeric: tabular-nums; }

  .packs { min-width: 260px; }

  .pack-text { display: grid; }

  .pack-name { font-weight: 600; }

  .pack-desc { color: var(--text-3); font-size: 12px; }
</style>
