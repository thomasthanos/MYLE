<script lang="ts">
  import { onMount } from "svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChartPie from "@lucide/svelte/icons/chart-pie";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Files from "@lucide/svelte/icons/files";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Info from "@lucide/svelte/icons/info";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import List from "@lucide/svelte/icons/list";
  import Network from "@lucide/svelte/icons/network";
  import RadarIcon from "@lucide/svelte/icons/radar";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Usb from "@lucide/svelte/icons/usb";
  import X from "@lucide/svelte/icons/x";
  import Zap from "@lucide/svelte/icons/zap";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { formatSize, sizeOf, type Drive } from "./api";
  import FileTypes from "./FileTypes.svelte";
  import FolderTree from "./FolderTree.svelte";
  import LargestFiles from "./LargestFiles.svelte";
  import { diskState as d, type View } from "./state.svelte";
  import Treemap from "./Treemap.svelte";

  onMount(() => void d.load());

  let layoutMode = $state<"split" | "treemap" | "list">("split");

  const tabs: { id: View; label: string; icon: typeof Files }[] = [
    { id: "folders", label: "Folders", icon: FolderTreeIcon },
    { id: "files", label: "Largest files", icon: Files },
    { id: "types", label: "File types", icon: ChartPie },
  ];

  const scanning = $derived(d.phase === "scanning");
  const drivePicked = $derived(!!d.targetDrive);
  const folderPicked = $derived(!!d.target && !drivePicked);
  const modeLabel = $derived(d.sizeMode === "alloc" ? "on disk" : "in size");

  function driveIcon(drive: Drive) {
    return drive.kind === "removable" ? Usb : drive.kind === "network" ? Network : HardDrive;
  }

  function driveTitle(drive: Drive): string {
    const used = drive.total - drive.free;
    return `${drive.letter}: ${drive.label || "Local Disk"} (${drive.filesystem})\n${formatSize(used)} used · ${formatSize(drive.free)} free of ${formatSize(drive.total)}`;
  }

  function onTabKey(e: KeyboardEvent, index: number) {
    const next =
      e.key === "ArrowRight" ? (index + 1) % tabs.length : e.key === "ArrowLeft" ? (index - 1 + tabs.length) % tabs.length : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    d.setView(tabs[next].id);
    document.getElementById(`da-tab-${tabs[next].id}`)?.focus();
  }

  /** Backspace or Alt+← goes one folder up in the treemap, unless typing. */
  function onKeydown(e: KeyboardEvent) {
    if (d.phase !== "ready" || e.defaultPrevented) return;
    const typing = e.target instanceof HTMLElement && e.target.closest("input, textarea, [contenteditable='true']");
    if (typing || document.querySelector('[aria-modal="true"]')) return;
    if (e.key === "Backspace" || (e.altKey && e.key === "ArrowLeft")) {
      e.preventDefault();
      d.zoomOut();
    }
  }

  function seconds(ms: number): string {
    return ms < 10_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms / 1000)} s`;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<PageHeader title="Disk Analyzer" subtitle="See what is filling a drive or a folder, then move what you no longer need to the Recycle Bin." />

<!-- WizTree Primary Control Bar -->
<section class="toolbar-box surface">
  <div class="toolbar-left">
    <label class="drive-selector-label" for="wiz-drive-select">
      <HardDrive size={16} class="selector-icon" />
      <span>Select:</span>
    </label>
    <div class="select-container">
      <select
        id="wiz-drive-select"
        class="wiz-select"
        value={d.target ?? ""}
        disabled={d.busy}
        onchange={(e) => {
          const val = e.currentTarget.value;
          if (val === "__custom__") {
            void d.pickFolder();
          } else if (val) {
            d.choose(val);
          }
        }}
      >
        {#each d.drives as drive (drive.root)}
          {@const free = formatSize(drive.free)}
          {@const total = formatSize(drive.total)}
          {@const pct = drive.total ? Math.round(((drive.total - drive.free) / drive.total) * 100) : 0}
          <option value={drive.root}>
            ({drive.letter}:) {drive.label || "Local Disk"} [{drive.filesystem}] — {free} free of {total} ({pct}% used)
          </option>
        {/each}
        {#if folderPicked}
          <option value={d.target}>
            Folder: {d.target}
          </option>
        {/if}
        <option value="__custom__">Choose a folder…</option>
      </select>
      <ChevronDown size={14} class="select-arrow" />
    </div>

    {#if scanning}
      <button type="button" class="btn danger scan-action-btn" onclick={() => d.cancel()}>
        <X size={15} />
        <span>Cancel</span>
      </button>
    {:else}
      <button
        type="button"
        class="btn primary glow scan-action-btn"
        disabled={!d.target || d.busy}
        onclick={() => d.scan()}
        title="Start high-speed drive scan"
      >
        <Zap size={15} />
        <span>Scan</span>
      </button>
    {/if}

    <button
      type="button"
      class="btn folder-pick-btn"
      disabled={d.busy}
      onclick={() => d.pickFolder()}
      title="Choose a specific directory to scan"
    >
      <FolderOpen size={15} />
      <span>Folder…</span>
    </button>
  </div>

  <div class="toolbar-right">
    <div class="mode" role="radiogroup" aria-label="Sizes">
      <button
        type="button"
        role="radio"
        aria-checked={d.sizeMode === "size"}
        class:active={d.sizeMode === "size"}
        disabled={d.phase === "deleting"}
        title="What the files hold"
        onclick={() => d.setSizeMode("size")}
      >
        Size
      </button>
      <button
        type="button"
        role="radio"
        aria-checked={d.sizeMode === "alloc"}
        class:active={d.sizeMode === "alloc"}
        disabled={d.phase === "deleting"}
        title="What the files take on the disk"
        onclick={() => d.setSizeMode("alloc")}
      >
        Size on disk
      </button>
    </div>
  </div>
</section>

<!-- Visual Quick Drive Cards Ribbon (WizTree Style) -->
<section class="drive-ribbon surface" role="group" aria-label="Available storage drives">
  <div class="ribbon-label">Drives:</div>
  <div class="ribbon-drives">
    {#each d.drives as drive (drive.root)}
      {@const usedRatio = drive.total ? (drive.total - drive.free) / drive.total : 0}
      {@const pct = Math.round(usedRatio * 100)}
      {@const Icon = driveIcon(drive)}
      {@const isSelected = d.target?.toLowerCase() === drive.root.toLowerCase()}
      <button
        type="button"
        class="drive-card"
        class:selected={isSelected}
        class:scanning-this={isSelected && scanning}
        aria-pressed={isSelected}
        disabled={d.busy && !isSelected}
        title={driveTitle(drive)}
        onclick={() => {
          if (!d.busy) d.choose(drive.root);
        }}
        ondblclick={() => {
          if (!d.busy) {
            d.choose(drive.root);
            void d.scan();
          }
        }}
      >
        <div class="card-head">
          <span class="drive-badge">
            <Icon size={15} class="badge-icon" />
            <strong>({drive.letter}:)</strong>
          </span>
          <span class="drive-vol-label" title={drive.label || "Local Disk"}>{drive.label || "Local Disk"}</span>
          <span class="fs-pill">{drive.filesystem}</span>
          {#if isSelected}
            <span class="active-indicator" title="Selected drive">
              {#if scanning}
                <RadarIcon size={12} class="spin" />
              {:else}
                <Check size={12} />
              {/if}
            </span>
          {/if}
        </div>

        <div class="meter-wrapper">
          <div class="meter-bar" aria-hidden="true">
            <div
              class="meter-fill"
              class:critical={usedRatio > 0.9}
              class:warning={usedRatio > 0.75 && usedRatio <= 0.9}
              style:width="{usedRatio * 100}%"
            ></div>
          </div>
        </div>

        <div class="card-footer">
          <span class="free-text"><strong>{formatSize(drive.free)}</strong> free of {formatSize(drive.total)}</span>
          <span class="pct-pill" class:alert={pct > 90}>{pct}%</span>
        </div>
      </button>
    {/each}

    {#if folderPicked}
      <div class="folder-card selected">
        <div class="folder-card-head">
          <FolderOpen size={15} class="folder-icon" />
          <span class="folder-title">Folder Selected</span>
          <button
            type="button"
            class="folder-clear-btn"
            disabled={d.busy}
            title="Reset to system drive"
            onclick={() => {
              const sys = d.drives.find((drv) => drv.system) ?? d.drives[0];
              if (sys) d.choose(sys.root);
            }}
          >
            <X size={13} />
          </button>
        </div>
        <div class="folder-path" title={d.target}>{d.target}</div>
      </div>
    {/if}
  </div>
</section>

{#if d.error && d.phase === "idle"}
  <div class="banner surface" role="alert"><CircleAlert size={18} /><span>{d.error}</span></div>
{/if}

{#if scanning && d.progress}
  <section class="scanning surface" aria-live="polite">
    <div class="counts">
      <span><strong>{d.progress.files.toLocaleString()}</strong> files</span>
      <span><strong>{d.progress.dirs.toLocaleString()}</strong> folders</span>
      <span><strong>{formatSize(d.sizeMode === "alloc" ? d.progress.alloc : d.progress.bytes)}</strong> {modeLabel}</span>
    </div>
    <ProgressBar value={d.scanShare} label="Scan progress" />
    <p class="current" title={d.progress.current}>Reading {d.progress.current}</p>
    <p class="note">Only folder listings are read: no file is opened, and cloud files are never downloaded.</p>
  </section>
{:else if d.done}
  {#if d.phase === "deleting" && d.deleting}
    <section class="scanning surface" aria-live="polite">
      <div class="counts"><span>Moving to the Recycle Bin · {Math.min(d.deleting.done + 1, d.deleting.total)} of {d.deleting.total}</span><em>{d.deleting.current}</em></div>
      <ProgressBar value={d.deleting.done / Math.max(1, d.deleting.total)} label="Moving to the Recycle Bin" />
    </section>
  {/if}
  {#if d.skipped.length}
    <section class="skipped surface" aria-label="Items left where they are">
      <div class="skipped-head">
        <Info size={15} />
        <span>{d.skipped.length === 1 ? "This item stays where it is:" : `These ${d.skipped.length} items stay where they are:`}</span>
        <button type="button" class="btn ghost small" onclick={() => d.dismissSkipped()}>Dismiss</button>
      </div>
      <ul>
        {#each d.skipped.slice(0, 50) as item (item.path)}
          <li><span class="path" title={item.path}>{item.path}</span><span class="why">{item.reason}</span></li>
        {/each}
      </ul>
    </section>
  {/if}
  <div class="results">
    <!-- Clean Unified Navigation & View Switcher Bar -->
    <div class="crumbs-bar surface">
      <nav class="crumbs" aria-label="Treemap folder">
        <button
          type="button"
          class="icon-btn back-btn"
          disabled={d.trail.length < 2}
          title="One folder up (Backspace)"
          aria-label="One folder up"
          onclick={() => d.zoomOut()}
        >
          <ArrowUp size={15} />
        </button>
        <ol class="crumb-list">
          {#each d.trail as crumb, index (crumb.id)}
            <li>
              {#if index > 0}<ChevronRight size={12} class="crumb-sep" />{/if}
              <button
                type="button"
                class="crumb-pill"
                aria-current={index === d.trail.length - 1 ? "location" : undefined}
                onclick={() => d.zoomTo(crumb.id)}
              >
                {#if index === 0}
                  <HardDrive size={13} class="crumb-icon" />
                {:else}
                  <Folder size={13} class="crumb-icon" />
                {/if}
                <span>{crumb.name}</span>
              </button>
            </li>
          {/each}
        </ol>
        <div class="crumb-tail">
          <span class="crumb-size-pill">{formatSize(d.currentSize)}</span>
          <div class="layout-switch" role="group" aria-label="View mode">
            <button
              type="button"
              class="layout-toggle-btn"
              class:active={layoutMode === "split"}
              onclick={() => layoutMode = "split"}
              title="Split view (Folder list + Treemap)"
            >
              <Columns2 size={14} />
            </button>
            <button
              type="button"
              class="layout-toggle-btn"
              class:active={layoutMode === "treemap"}
              onclick={() => layoutMode = "treemap"}
              title="Full Treemap view"
            >
              <LayoutGrid size={14} />
            </button>
            <button
              type="button"
              class="layout-toggle-btn"
              class:active={layoutMode === "list"}
              onclick={() => layoutMode = "list"}
              title="Full List view"
            >
              <List size={14} />
            </button>
          </div>
        </div>
      </nav>
    </div>

    <div class="split mode-{layoutMode}">
      {#if layoutMode !== "treemap"}
        <section class="pane list surface">
          <div class="tabs-row">
            <div class="tabs" role="tablist" aria-label="Results">
              {#each tabs as tab, index (tab.id)}
                <button
                  type="button"
                  id="da-tab-{tab.id}"
                  role="tab"
                  aria-selected={d.view === tab.id}
                  aria-controls="da-panel"
                  tabindex={d.view === tab.id ? 0 : -1}
                  class:active={d.view === tab.id}
                  onclick={() => d.setView(tab.id)}
                  onkeydown={(e) => onTabKey(e, index)}><tab.icon size={15} /> {tab.label}</button>
              {/each}
            </div>
            <button
              type="button"
              class="btn danger small recycle"
              disabled={!d.selection.size || d.busy}
              title="Move the selected items to the Recycle Bin (Del)"
              onclick={() => d.recycle()}
            >
              <Trash2 size={14} />
              {d.selection.size > 1 ? `Move ${d.selection.size} to Recycle Bin` : "Move to Recycle Bin"}
            </button>
          </div>
          <div id="da-panel" class="panel" role="tabpanel" aria-labelledby="da-tab-{d.view}">
            {#if d.view === "folders"}
              <FolderTree />
            {:else if d.view === "files"}
              <LargestFiles />
            {:else}
              <FileTypes />
            {/if}
          </div>
        </section>
      {/if}

      {#if layoutMode !== "list"}
        <section class="pane treemap surface" aria-label="Treemap">
          <Treemap />
        </section>
      {/if}
    </div>
  </div>
  <footer class="status">
    {#if d.root}
      <span>{d.root.files.toLocaleString()} files · {d.root.dirs.toLocaleString()} folders · {formatSize(sizeOf(d.root, d.sizeMode))} {modeLabel}</span>
    {/if}
    <span>Scanned in {seconds(d.done.elapsedMs)}</span>
    {#if d.done.inaccessible}
      <span class="warn" title={[...d.done.inaccessiblePaths, d.done.inaccessible > d.done.inaccessiblePaths.length ? "…" : ""].filter(Boolean).join("\n")}>
        <CircleAlert size={13} />
        {d.done.inaccessible.toLocaleString()} folder{d.done.inaccessible === 1 ? "" : "s"} could not be read
      </span>
    {/if}
    {#if d.done.windowsDrive}
      <span class="hint" title="Windows keeps one copy of many system files under several names (hard links, in WinSxS and System32). Without administrator rights each name counts, so the Windows folder can look bigger than the space it really takes.">
        <Info size={13} /> Hard-linked system files count once per link
      </span>
    {/if}
    {#if d.selection.size}<span class="selected">{d.selection.size.toLocaleString()} selected</span>{/if}
  </footer>
{:else if !scanning}
  <section class="empty surface">
    <HardDrive size={36} />
    <h2>Pick a drive or a folder, then Scan</h2>
    <p>The scan reads folder listings only, so even a whole drive takes well under a minute. Nothing is changed until you choose to move something to the Recycle Bin.</p>
  </section>
{/if}

<style>
  /* WizTree Primary Control Bar */
  .toolbar-box {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    margin-bottom: 10px;
    padding: 10px 14px;
    border-radius: 12px;
  }
  .toolbar-left {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    flex: 1;
    min-width: 0;
  }
  .drive-selector-label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: 13px;
    font-weight: 600;
  }
  :global(.selector-icon) {
    color: rgb(var(--accent-soft-rgb));
  }
  .select-container {
    position: relative;
    display: flex;
    align-items: center;
    min-width: 280px;
    max-width: 460px;
    flex: 1;
  }
  .wiz-select {
    width: 100%;
    height: 34px;
    padding: 0 32px 0 12px;
    border-radius: 8px;
    background: var(--input-fill);
    border: 1px solid var(--btn-border);
    color: var(--text-1);
    font-size: 13px;
    font-weight: 500;
    appearance: none;
    cursor: pointer;
    outline: none;
    transition: border-color var(--dur-fast), box-shadow var(--dur-fast);
  }
  .wiz-select:focus {
    border-color: rgb(var(--accent-rgb));
    box-shadow: 0 0 0 2px rgb(var(--accent-rgb) / 0.25);
  }
  .wiz-select:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .wiz-select option {
    background: var(--float-bg);
    color: var(--text-1);
  }
  :global(.select-arrow) {
    position: absolute;
    right: 10px;
    pointer-events: none;
    color: var(--text-3);
  }
  .scan-action-btn {
    height: 34px;
    padding: 0 16px;
    font-weight: 600;
    gap: 6px;
  }
  .scan-action-btn span {
    font-weight: 700;
  }
  .folder-pick-btn {
    height: 34px;
    padding: 0 12px;
    gap: 6px;
  }
  .toolbar-right {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-left: auto;
  }

  /* Drive Cards Ribbon */
  .drive-ribbon {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: var(--fluid-gap-3);
    padding: 10px 14px;
    border-radius: 12px;
  }
  .ribbon-label {
    padding-top: 6px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-3);
    flex-shrink: 0;
  }
  .ribbon-drives {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    flex: 1;
    min-width: 0;
  }
  .drive-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 210px;
    max-width: 280px;
    flex: 1 1 210px;
    padding: 9px 12px;
    border: 1px solid var(--btn-border);
    border-radius: 10px;
    background: var(--chip-fill);
    color: var(--text-1);
    text-align: left;
    box-shadow: var(--btn-sheen);
    cursor: pointer;
    transition: background var(--dur-fast), border-color var(--dur-fast), transform var(--dur-fast), box-shadow var(--dur-fast);
  }
  .drive-card:hover:not(:disabled) {
    background: var(--chip-fill-hover);
    border-color: rgb(var(--accent-rgb) / 0.35);
    transform: translateY(-1px);
  }
  .drive-card.selected {
    border-color: rgb(var(--accent-rgb) / 0.85);
    background: var(--selected);
    box-shadow: 0 0 14px rgb(var(--accent-rgb) / 0.22);
  }
  .drive-card.scanning-this {
    border-color: rgb(var(--accent-rgb));
    box-shadow: 0 0 16px rgb(var(--accent-rgb) / 0.3);
  }
  .drive-card:disabled:not(.selected) {
    opacity: 0.7;
    cursor: not-allowed;
  }
  .card-head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .drive-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: rgb(var(--accent-soft-rgb));
    font-size: 13px;
    flex-shrink: 0;
  }
  :global(.badge-icon) {
    color: rgb(var(--accent-soft-rgb));
  }
  .drive-vol-label {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 12px;
    color: var(--text-2);
    font-weight: 500;
    flex: 1;
  }
  .fs-pill {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--hover);
    color: var(--text-3);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.03em;
    flex-shrink: 0;
  }
  .active-indicator {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 17px;
    height: 17px;
    border-radius: 999px;
    background: var(--btn-primary);
    color: var(--btn-primary-text);
    flex-shrink: 0;
  }
  :global(.spin) {
    animation: da-spin 1.5s linear infinite;
  }
  @keyframes da-spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .meter-wrapper {
    width: 100%;
  }
  .meter-bar {
    display: block;
    height: 5px;
    border-radius: 999px;
    background: var(--input-fill);
    overflow: hidden;
  }
  .meter-fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: rgb(var(--accent-rgb));
    transition: width 0.3s ease;
  }
  .meter-fill.warning {
    background: #d29a4a;
  }
  .meter-fill.critical {
    background: var(--danger);
  }

  .card-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .free-text strong {
    color: var(--text-1);
  }
  .pct-pill {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--hover);
    font-size: 10.5px;
    font-weight: 600;
  }
  .pct-pill.alert {
    background: rgb(229 72 77 / 0.2);
    color: #ffb4b0;
  }

  /* Folder card */
  .folder-card {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 4px;
    min-width: 210px;
    max-width: 320px;
    padding: 9px 12px;
    border: 1px solid rgb(var(--accent-rgb) / 0.6);
    border-radius: 10px;
    background: var(--selected);
    box-shadow: 0 0 14px rgb(var(--accent-rgb) / 0.2);
  }
  .folder-card-head {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-1);
    font-size: 12px;
    font-weight: 600;
  }
  :global(.folder-icon) {
    color: rgb(var(--accent-soft-rgb));
  }
  .folder-title {
    flex: 1;
  }
  .folder-clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    border: none;
    background: var(--hover);
    color: var(--text-2);
    cursor: pointer;
  }
  .folder-clear-btn:hover {
    background: rgb(229 72 77 / 0.25);
    color: var(--text-1);
  }
  .folder-path {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 11px;
    color: var(--text-3);
    direction: ltr;
  }

  .mode { display: inline-flex; padding: 2px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--input-fill); }
  .mode button { height: 26px; padding: 0 10px; border-radius: 7px; color: var(--text-2); font-size: 12px; font-weight: 600; }
  .mode button:hover:not(:disabled) { color: var(--text-1); }
  .mode button.active { background: var(--selected); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .btn.glow { box-shadow: 0 0 14px rgb(var(--accent-rgb) / 0.35); }
  .banner { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; padding: 9px 12px; border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; font-size: var(--fluid-sm); border-radius: 10px; }
  .scanning { display: grid; gap: 8px; margin-bottom: var(--fluid-gap-3); padding: var(--fluid-gap-3); border-radius: 12px; animation: fade-in var(--dur-med) var(--ease-out) both; }
  .counts { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 18px; color: var(--text-2); font-size: var(--fluid-sm); font-variant-numeric: tabular-nums; }
  .counts strong { color: var(--text-1); font-size: var(--fluid-md); }
  .counts em { overflow: hidden; color: rgb(var(--accent-soft-rgb)); font-style: normal; white-space: nowrap; text-overflow: ellipsis; }
  .current { overflow: hidden; color: var(--text-3); font-size: var(--fluid-xs); white-space: nowrap; text-overflow: ellipsis; }
  .note { color: var(--text-3); font-size: var(--fluid-xs); }
  .skipped { margin-bottom: var(--fluid-gap-2); padding: 8px 12px; font-size: var(--fluid-xs); border-radius: 10px; }
  .skipped-head { display: flex; align-items: center; gap: 8px; color: var(--text-2); }
  .skipped-head .btn { margin-left: auto; }
  .skipped ul { display: grid; gap: 3px; max-height: 120px; margin: 6px 0 0; padding: 0; overflow: auto; list-style: none; }
  .skipped li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 12px; }
  .skipped .path { overflow: hidden; color: var(--text-1); white-space: nowrap; text-overflow: ellipsis; }
  .skipped .why { color: var(--text-3); }

  .results { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: 8px; }

  /* Sleek Breadcrumb Bar */
  .crumbs-bar {
    padding: 6px 10px;
    border-radius: 10px;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .crumbs .back-btn {
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: var(--chip-fill);
    border: 1px solid var(--btn-border);
  }
  .crumb-list {
    display: flex;
    align-items: center;
    flex: 1;
    gap: 3px;
    min-width: 0;
    margin: 0;
    padding: 0;
    overflow: hidden;
    list-style: none;
  }
  .crumb-list li {
    display: flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
  }
  :global(.crumb-sep) {
    color: var(--text-3);
    opacity: 0.6;
    flex-shrink: 0;
  }
  .crumb-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    overflow: hidden;
    max-width: 180px;
    padding: 4px 8px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-2);
    font-size: 12px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .crumb-pill:hover {
    background: var(--hover);
    color: var(--text-1);
  }
  .crumb-pill[aria-current="location"] {
    background: var(--selected);
    color: var(--text-1);
    font-weight: 600;
  }
  .crumb-pill span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  :global(.crumb-icon) {
    color: rgb(var(--accent-soft-rgb));
    flex-shrink: 0;
  }

  .crumb-tail {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
    margin-left: auto;
  }
  .crumb-size-pill {
    padding: 3px 8px;
    border-radius: 6px;
    background: var(--chip-fill);
    color: var(--text-1);
    font-size: 11.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .layout-switch {
    display: inline-flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--input-fill);
    border: 1px solid var(--btn-border);
    gap: 2px;
  }
  .layout-toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-3);
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .layout-toggle-btn:hover {
    color: var(--text-1);
  }
  .layout-toggle-btn.active {
    background: var(--selected);
    color: var(--text-1);
    box-shadow: var(--btn-sheen);
  }

  /* Flexible Split & Single-Pane Modes */
  .split { display: grid; gap: var(--fluid-gap-3); flex: 1; min-height: 0; }
  .split.mode-split { grid-template-columns: minmax(320px, 1fr) minmax(400px, 1.4fr); }
  .split.mode-treemap { grid-template-columns: minmax(0, 1fr); }
  .split.mode-list { grid-template-columns: minmax(0, 1fr); }

  .pane { display: flex; flex-direction: column; min-height: 0; min-width: 0; border-radius: 12px; overflow: hidden; }
  .tabs-row { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 6px; padding: 8px 10px 6px; border-bottom: 1px solid var(--glass-border); }
  .tabs { display: flex; gap: 4px; }
  .tabs button { display: flex; align-items: center; gap: 7px; padding: 6px 11px; border-radius: 8px; color: var(--text-2); font-size: 12.5px; }
  .tabs button:hover { background: var(--hover); color: var(--text-1); }
  .tabs button.active { background: var(--selected); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .tabs button :global(svg) { color: rgb(var(--accent-soft-rgb)); }
  .recycle:disabled { opacity: 0.4; }
  .panel { display: flex; flex-direction: column; flex: 1; min-height: 0; }

  /* Treemap Pane */
  .treemap { gap: 8px; padding: 10px; }

  .status { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 16px; margin-top: var(--fluid-gap-2); color: var(--text-3); font-size: var(--fluid-xs); font-variant-numeric: tabular-nums; }
  .status span { display: inline-flex; align-items: center; gap: 5px; }
  .status .warn { color: #e6b477; }
  .status .selected { margin-left: auto; color: var(--text-2); }
  .empty { display: grid; justify-items: center; gap: 10px; margin-top: var(--fluid-gap-4); padding: 48px 24px; color: var(--text-3); text-align: center; border-radius: 16px; }
  .empty :global(svg) { color: rgb(var(--accent-soft-rgb)); }
  .empty h2 { color: var(--text-1); font-size: var(--fluid-md); }
  .empty p { max-width: 60ch; font-size: var(--fluid-sm); line-height: 1.5; }

  @media (max-width: 900px) {
    .split.mode-split { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(280px, 1fr) minmax(280px, 1fr); }
  }
  @keyframes fade-in { from { opacity: 0; } }
</style>
