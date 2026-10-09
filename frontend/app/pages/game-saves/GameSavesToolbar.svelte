<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import DatabaseZap from "@lucide/svelte/icons/database-zap";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ScanSearch from "@lucide/svelte/icons/scan-search";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import Popover from "../../../lib/components/Popover.svelte";
  import type { OperationStage } from "./api";
  import { formatBytes, gameSavesState as gameSaves, type GameSavesFilter } from "./state.svelte";

  /** One click each, with how many games it shows; an empty one hides. */
  const filterOptions: { value: GameSavesFilter; label: string; hint: string }[] = [
    { value: "all", label: "All", hint: "Every game" },
    { value: "notBackedUp", label: "First backup needed", hint: "Saves that have no backup yet" },
    { value: "changed", label: "New progress", hint: "Saves that changed since their last backup" },
    { value: "backedUp", label: "Backed up", hint: "Saves whose backup is up to date" },
    { value: "problems", label: "Needs attention", hint: "Saves that could not be read, or need a restore location" },
  ];
  const shownFilters = $derived(
    filterOptions.filter((option) => option.value === "all" || option.value === gameSaves.filter || gameSaves.statusCounts[option.value] > 0),
  );
  const stageLabels: Record<OperationStage, string> = {
    preparing: "Preparing…",
    scanning: "Scanning save locations…",
    updatingDatabase: "Updating the game database…",
    creatingSafetyBackup: "Creating a safety backup…",
    backingUp: "Backing up saves…",
    waitingForOneDrive: "Starting OneDrive to download online-only saves…",
    restoring: "Restoring saves…",
    finishing: "Finishing…",
  };
  const progress = $derived(
    gameSaves.operation?.total
      ? Math.max(0, Math.min(100, (gameSaves.operation.done / gameSaves.operation.total) * 100))
      : null,
  );

  /** After this long the engine is still at it: say why that can happen. */
  const SLOW_AFTER_S = 20;

  // The engine reports nothing while it scans or copies, so show that time
  // passes: seconds so far, and a hint once it gets slow.
  let now = $state(0);
  $effect(() => {
    const operation = gameSaves.operation;
    if (!operation) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const elapsed = $derived(Math.max(0, Math.round((now - (gameSaves.operation?.startedAt ?? now)) / 1000)));

  function formatElapsed(seconds: number) {
    return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
  }
</script>

<div class="toolbar surface">
  <div class="main-row">
    <label class="search">
      <Search size={15} />
      <input
        class="input"
        type="search"
        value={gameSaves.query}
        placeholder="Search games…"
        aria-label="Search games"
        autocomplete="off"
        spellcheck="false"
        oninput={(event) => gameSaves.setQuery(event.currentTarget.value)}
      />
      {#if gameSaves.query}
        <button class="clear icon-btn" aria-label="Clear search" onclick={() => gameSaves.setQuery("")}><X size={13} /></button>
      {/if}
    </label>

    <div class="filters" role="group" aria-label="Show">
      {#each shownFilters as option (option.value)}
        <button
          type="button"
          class="chip"
          class:active={gameSaves.filter === option.value}
          aria-pressed={gameSaves.filter === option.value}
          title={option.hint}
          onclick={() => gameSaves.setFilter(option.value)}
        >
          {option.label} <span class="count">{gameSaves.statusCounts[option.value]}</span>
        </button>
      {/each}
    </div>

    <span class="spacer"></span>

    <button class="btn" disabled={gameSaves.locked || gameSaves.discovering} onclick={() => gameSaves.scanResult ? gameSaves.refresh() : gameSaves.scan()}>
      <RefreshCw size={15} /> Refresh saves
    </button>
    <Popover align="end">
      {#snippet trigger({ toggle, open })}
        <button class="btn" aria-label="More Game Saves actions" aria-expanded={open} disabled={gameSaves.locked} onclick={toggle}><Ellipsis size={16} /> More</button>
      {/snippet}
      {#snippet children({ close })}
        <div class="more-menu surface">
          <button class="btn ghost" disabled={gameSaves.locked} onclick={() => { close(); void gameSaves.scan(); }}><ScanSearch size={15} /> Find new games</button>
          <p>Check every game in the database.</p>
          <button class="btn ghost" disabled={gameSaves.locked} onclick={() => { close(); void gameSaves.updateDatabase(); }}><DatabaseZap size={15} /> Update game database</button>
          <p>Download the latest save locations.</p>
          <label class="menu-toggle">
            <input type="checkbox" class="switch" role="switch" checked={gameSaves.steamCovers} onchange={(event) => gameSaves.setSteamCovers(event.currentTarget.checked)} />
            Covers from Steam
          </label>
          <p>Games whose cover is not on this PC get it from Steam, which then sees which games you have.</p>
        </div>
      {/snippet}
    </Popover>
  </div>

  <div class="action-row">
    <label class="select-all" title={gameSaves.allVisibleSelected ? "Untick every game in the list" : "Tick every game in the list, to back up or restore them together"}>
      <input
        type="checkbox"
        class="check"
        checked={gameSaves.allVisibleSelected}
        {@attach (box) => { box.indeterminate = gameSaves.someVisibleSelected; }}
        disabled={!gameSaves.selectableGames.length || gameSaves.selectionLocked}
        onchange={(event) => {
          event.currentTarget.checked = gameSaves.allVisibleSelected;
          gameSaves.toggleAllVisible();
        }}
      />
      <span>
        {#if gameSaves.selectedGames.length}
          <strong>{gameSaves.selectedGames.length} selected</strong> of {gameSaves.visibleGames.length}
        {:else}
          Select all {gameSaves.visibleGames.length} {gameSaves.visibleGames.length === 1 ? "game" : "games"}
        {/if}
      </span>
    </label>
    {#if gameSaves.selectedGames.length}
      <span class="selection">{formatBytes(gameSaves.selectedBytes)}</span>
      <button class="link-btn" disabled={gameSaves.selectionLocked} onclick={() => gameSaves.clearSelection()}>Clear</button>
    {/if}
    {#if gameSaves.discovering}
      <span class="discovering" title="Looking for newly installed games. Your current list stays usable.">
        <LoaderCircle size={12} class="spin" /> Looking for new games…
        <button class="link" onclick={() => gameSaves.stopDiscovery()}>Stop</button>
      </span>
    {:else if gameSaves.operation?.background}
      <span class="discovering"><LoaderCircle size={12} class="spin" /> Checking for changed saves…</span>
    {/if}
    <span class="spacer"></span>
    {#if gameSaves.tab === "pc"}
      {#if gameSaves.pendingGames.length}
        <!-- One click for what needs it; the arrow narrows it down. -->
        <div class="split" class:secondary={gameSaves.selectedGames.length > 0}>
          <button
            class="btn split-main"
            class:primary={!gameSaves.selectedGames.length}
            disabled={gameSaves.locked}
            title="Back up every game that needs its first backup or has new progress"
            onclick={() => gameSaves.backupPending("both")}
          >
            <Archive size={15} /> Back up what's new ({gameSaves.pendingGames.length})
          </button>
          <Popover align="end">
            {#snippet trigger({ toggle, open })}
              <button class="btn split-arrow" class:primary={!gameSaves.selectedGames.length} aria-label="More ways to back up" aria-expanded={open} disabled={gameSaves.locked} onclick={toggle}>
                <ChevronDown size={15} />
              </button>
            {/snippet}
            {#snippet children({ close })}
              <div class="more-menu surface">
                <button class="btn ghost" disabled={!gameSaves.newGames.length} onclick={() => { close(); void gameSaves.backupPending("new"); }}>
                  <Archive size={15} /> Only first backups ({gameSaves.newGames.length})
                </button>
                <button class="btn ghost" disabled={!gameSaves.changedGames.length} onclick={() => { close(); void gameSaves.backupPending("changed"); }}>
                  <Archive size={15} /> Only new progress ({gameSaves.changedGames.length})
                </button>
                <button class="btn ghost" disabled={gameSaves.selectionLocked} onclick={() => { close(); gameSaves.selectPending(); }}>
                  <ListChecks size={15} /> Select them, to choose one by one
                </button>
              </div>
            {/snippet}
          </Popover>
        </div>
      {:else if gameSaves.scanResult && !gameSaves.selectedGames.length}
        <span class="all-done"><CircleCheck size={14} /> Every save is backed up</span>
      {/if}
      {#if gameSaves.selectedGames.length}
        <button class="btn primary action" disabled={gameSaves.locked} onclick={() => gameSaves.backupSelected()}>
          <Archive size={15} /> Back up selected ({gameSaves.selectedGames.length})
        </button>
      {/if}
    {:else}
      <button class="btn primary action" disabled={!gameSaves.selectedGames.length || gameSaves.locked} onclick={() => gameSaves.openRestorePicker()}>
        <RotateCcw size={15} /> Review restore ({gameSaves.selectedGames.length})
      </button>
    {/if}
  </div>

  {#if gameSaves.operation && !gameSaves.operation.background}
    <div class="operation" aria-live="polite">
      <LoaderCircle size={14} class="spin" />
      <span class="operation-text">
        <strong>{stageLabels[gameSaves.operation.stage]}</strong>
        {#if gameSaves.operation.current}<small>{gameSaves.operation.current}</small>{/if}
        {#if gameSaves.operation.note}<small>{gameSaves.operation.note}</small>{/if}
        {#if elapsed >= SLOW_AFTER_S && !gameSaves.operation.total}
          <small class="slow">
            Still working. Many games, or saves in OneDrive that are online-only (they download first), make this take
            longer.
          </small>
        {/if}
      </span>
      {#if gameSaves.operation.total}
        <span class="numbers">{gameSaves.operation.done}/{gameSaves.operation.total}</span>
      {/if}
      {#if elapsed >= 2}
        <span class="numbers">{formatElapsed(elapsed)}</span>
      {/if}
      <button class="btn small" disabled={gameSaves.cancelling} onclick={() => gameSaves.cancel()}>
        {gameSaves.cancelling ? "Cancelling…" : "Cancel"}
      </button>
      <span class="progress" class:indeterminate={progress === null} aria-hidden="true">
        <span style:width={progress === null ? undefined : `${progress}%`}></span>
      </span>
    </div>
  {/if}
</div>

<style>
  .toolbar {
    display: grid;
    gap: 9px;
    margin-bottom: 12px;
    padding: 12px;
    position: sticky;
    top: -18px;
    z-index: 18;
    background: var(--bar-fill);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.055), 0 14px 26px -22px rgb(0 0 0 / 0.94);
  }

  .main-row,
  .action-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 7px;
  }

  .action-row { padding-top: 10px; border-top: 1px solid rgb(255 255 255 / .055); }
  .more-menu { display: grid; min-width: 255px; padding: 7px; background: var(--bg-2, #1b2030); box-shadow: 0 12px 32px rgb(0 0 0 / .35); }
  .more-menu .btn { justify-content: flex-start; }
  .more-menu p { margin: 0 10px 10px; color: var(--text-3); font-size: 11px; }
  .menu-toggle { display: flex; align-items: center; gap: 10px; padding: 8px 10px 4px; border-top: 1px solid rgb(255 255 255 / 0.06); color: var(--text-1); font-size: 12.5px; cursor: pointer; }

  .search {
    position: relative;
    display: flex;
    align-items: center;
    flex: 1 1 230px;
    min-width: 160px;
  }

  .search > :global(svg) {
    position: absolute;
    left: 11px;
    z-index: 1;
    color: var(--text-3);
    pointer-events: none;
  }

  .search .input {
    width: 100%;
    padding-right: 31px;
    padding-left: 33px;
  }

  .search input::-webkit-search-cancel-button {
    display: none;
  }

  .clear {
    position: absolute;
    right: 3px;
    width: 28px;
    height: 28px;
  }

  .spacer {
    flex: 1;
  }

  .filters { display: flex; flex-wrap: wrap; gap: 5px; }
  .filters .chip { height: 30px; padding: 0 11px; font-size: 12px; }

  .select-all { display: inline-flex; align-items: center; gap: 9px; padding: 4px 8px 4px 4px; border-radius: 8px; color: var(--text-2); font-size: 12px; cursor: pointer; }
  .select-all:hover { background: rgb(255 255 255 / 0.03); }
  .select-all:has(input:disabled) { cursor: default; opacity: 0.55; }
  .select-all strong { color: var(--text-1); font-weight: 600; }
  /* Some ticked, not all: a dash in the box. */
  .select-all .check:indeterminate { border-color: transparent; background: var(--accent-grad); }
  .select-all .check:indeterminate::after { content: ""; position: absolute; left: 4px; top: 7px; width: 7px; height: 2px; border-radius: 1px; background: #fff; }

  .selection {
    color: var(--text-3);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  .link-btn { padding: 3px 7px; border-radius: 6px; color: rgb(var(--accent-soft-rgb)); font-size: 11.5px; font-weight: 550; }
  .link-btn:hover { background: var(--hover); color: var(--text-1); }

  .split { display: inline-flex; align-items: stretch; }
  .split-main, .split-arrow { background: rgb(255 255 255 / 0.05); filter: none; scale: 1; translate: none; }
  .split-main.primary, .split-arrow.primary { background: var(--accent); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.18); }
  .split:has(> .split-main:not(:disabled)):hover .split-main,
  .split:has(> .split-main:not(:disabled)):hover .split-arrow { background: rgb(255 255 255 / 0.09); }
  .split:has(> .split-main:not(:disabled)):hover .split-main.primary,
  .split:has(> .split-main:not(:disabled)):hover .split-arrow.primary { background: color-mix(in srgb, var(--accent), white 8%); }
  .split-main { border-top-right-radius: 0; border-bottom-right-radius: 0; border-right: 0; }
  .split-arrow { width: 34px; padding: 0; justify-content: center; border-left: 1px solid rgb(255 255 255 / 0.18); border-top-left-radius: 0; border-bottom-left-radius: 0; }
  .split :global(.popover) { display: flex; }
  .all-done { display: inline-flex; align-items: center; gap: 6px; color: #8fd9b6; font-size: 12px; }

  .discovering {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: rgb(var(--accent-soft-rgb) / 0.75);
    font-size: 11px;
  }

  .discovering .link {
    margin-left: 3px;
    padding: 0;
    color: rgb(var(--accent-soft-rgb) / 0.95);
    font-size: 11px;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .action {
    min-width: 126px;
  }

  .operation {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 7px 8px 9px;
    border-radius: 9px;
    background: rgb(var(--accent-rgb) / 0.045);
    color: var(--accent);
  }

  .operation-text {
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .operation-text strong {
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  .operation-text small {
    overflow: hidden;
    color: var(--text-3);
    font-size: 10.5px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .numbers {
    margin-left: auto;
    color: var(--text-2);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }

  .progress {
    position: absolute;
    right: 8px;
    bottom: 3px;
    left: 8px;
    height: 2px;
    overflow: hidden;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.06);
  }

  .numbers + .numbers {
    margin-left: 0;
  }

  .operation-text small.slow {
    color: rgb(245 188 95 / 0.8);
    white-space: normal;
  }

  .progress > span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent-grad);
    transition: width var(--dur-med) var(--ease-out);
  }

  /* The engine gives no count while it scans: a moving band, not a bar
     frozen at zero. */
  .progress.indeterminate > span {
    width: 30%;
    animation: sweep 1.3s var(--ease-in-out) infinite;
  }

  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  button:disabled {
    opacity: 0.45;
    pointer-events: none;
  }

  @container (max-width: 720px) {
    .toolbar {
      position: relative;
      top: auto;
    }

    .main-row .spacer {
      display: none;
    }

    .operation { flex-wrap: wrap; }
    .operation-text { flex-direction: column; align-items: flex-start; gap: 3px; }
    .operation-text small { max-width: 100%; white-space: normal; overflow-wrap: anywhere; }
    .action-row .spacer { display: none; }
    .action { margin-left: auto; }
    .selection { border: 0; padding-left: 0; }
  }
</style>
