<script lang="ts">
  import { onMount } from "svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChartPie from "@lucide/svelte/icons/chart-pie";
  import Check from "@lucide/svelte/icons/check";
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
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
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
  import CloudStorage from "./cloud-storage/CloudStorage.svelte";

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
    if (typing || document.querySelector('[aria-modal="true"]:not(dialog), dialog[open]')) return;
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

<section class="scan-picker surface" aria-label="Scan target">
  <header class="picker-header">
    <h2>Scan target</h2>
    <div class="picker-options">
    {#if d.targetDrive?.mftPossible}
      <div class="picker-option">
        <span>Scan mode</span>
        <div class="mode" role="radiogroup" aria-label="Scan method">
          <button type="button" role="radio" aria-checked={d.fast} class:active={d.fast} disabled={d.busy} title="Windows asks for administrator approval" onclick={() => d.setFast(true)}>Fast</button>
          <button type="button" role="radio" aria-checked={!d.fast} class:active={!d.fast} disabled={d.busy} title="Scan without administrator approval" onclick={() => d.setFast(false)}>Normal</button>
        </div>
      </div>
    {/if}
    <div class="picker-option">
    <span>File sizes</span>
    <div class="mode" role="radiogroup" aria-label="Sizes">
      <button type="button" role="radio" aria-checked={d.sizeMode === "size"} class:active={d.sizeMode === "size"} disabled={d.phase === "deleting"} title="What the files hold" onclick={() => d.setSizeMode("size")}>Size</button>
      <button type="button" role="radio" aria-checked={d.sizeMode === "alloc"} class:active={d.sizeMode === "alloc"} disabled={d.phase === "deleting"} title="What the files take on the disk" onclick={() => d.setSizeMode("alloc")}>Size on disk</button>
    </div>
    </div>
    </div>
  </header>

  <div class="drive-grid" role="group" aria-label="Available storage drives">
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
        onclick={() => { if (!d.busy) d.choose(drive.root); }}
        ondblclick={() => {
          if (!d.busy) {
            d.choose(drive.root);
            void d.scan();
          }
        }}
      >
        <div class="card-head">
          <span class="drive-icon"><Icon size={20} /></span>
          <span class="drive-name">
            <strong>{drive.letter}:</strong>
            <span>{drive.label || "Local Disk"}</span>
          </span>
          <span class="active-indicator" class:chosen={isSelected} aria-hidden="true">
            {#if isSelected}
              {#if scanning}<RadarIcon size={13} class="spin" />{:else}<Check size={13} />{/if}
            {/if}
          </span>
        </div>
        <div class="card-capacity">
          <span><strong>{formatSize(drive.free)}</strong> free</span>
          <span class="drive-total">of {formatSize(drive.total)}</span>
        </div>
        <div class="meter-bar" aria-hidden="true">
          <div class="meter-fill" class:critical={usedRatio > 0.9} class:warning={usedRatio > 0.75 && usedRatio <= 0.9} style:width="{usedRatio * 100}%"></div>
        </div>
        <div class="card-footer">
          <span>{drive.filesystem}</span>
          <span class:capacity-alert={pct > 90}>{pct}% used</span>
        </div>
      </button>
    {/each}
  </div>

  <footer class="picker-footer">
    <div class="selected-target" aria-live="polite">
      {#if folderPicked}<FolderOpen size={17} />{:else}<HardDrive size={17} />{/if}
      <div class="target-copy">
        <span>{folderPicked ? "Selected folder" : "Selected drive"}</span>
        <strong title={d.target ?? ""}>{d.target ?? "Choose a drive or folder"}</strong>
      </div>
      {#if folderPicked}
        <button
          type="button"
          class="folder-clear-btn"
          disabled={d.busy || !d.drives.length}
          aria-label="Reset to system drive"
          title="Reset to system drive"
          onclick={() => {
            const sys = d.drives.find((drive) => drive.system) ?? d.drives[0];
            if (sys) d.choose(sys.root);
          }}
        ><X size={14} /></button>
      {/if}
    </div>
    <div class="picker-actions">
      <button type="button" class="btn folder-pick-btn" disabled={d.busy} onclick={() => d.pickFolder()}>
        <FolderOpen size={16} /><span>Choose folder</span>
      </button>
      {#if scanning}
        <button type="button" class="btn danger scan-action-btn" onclick={() => d.cancel()}><X size={16} /><span>Cancel scan</span></button>
      {:else}
        <button type="button" class="btn primary scan-action-btn" disabled={!d.target || d.busy} title={d.fastScan ? "Windows will ask for administrator approval" : "Scan the selected location"} onclick={() => d.scan()}>
          {#if d.fastScan}<ShieldCheck size={16} />{:else}<Zap size={16} />{/if}<span>{d.targetDrive ? (d.fastScan ? 'Fast scan ' : 'Scan ') + d.targetDrive.letter + ':' : folderPicked ? 'Scan folder' : 'Scan'}</span>
        </button>
      {/if}
    </div>
  </footer>
</section>

<CloudStorage />

{#if d.error && d.phase === "idle"}
  <div class="banner surface" role="alert"><CircleAlert size={18} /><span>{d.error}</span></div>
{/if}

{#if scanning && d.progress}
  <section class="scanning surface" aria-live="polite">
    {#if d.progress.phase === "approval"}
      <p class="phase"><ShieldCheck size={16} /> Waiting for administrator approval…</p>
      <ProgressBar value={null} label="Waiting for approval" />
      <p class="note">Approve the Windows prompt so MYLE can read the drive's file table. Decline it and the drive is scanned folder by folder instead.</p>
    {:else if d.progress.phase === "table"}
      <p class="phase"><ShieldCheck size={16} /> Reading the file table of {d.targetDrive?.letter ?? ""}:…</p>
      <ProgressBar value={null} label="Reading the file table" />
      <p class="note">Only the file table is read: no file is opened, and cloud files are never downloaded.</p>
    {:else}
      <div class="counts">
        <span><strong>{d.progress.files.toLocaleString()}</strong> files</span>
        <span><strong>{d.progress.dirs.toLocaleString()}</strong> folders</span>
        <span><strong>{formatSize(d.sizeMode === "alloc" ? d.progress.alloc : d.progress.bytes)}</strong> {modeLabel}</span>
      </div>
      <ProgressBar value={d.scanShare} label="Scan progress" />
      {#if d.progress.phase === "build"}
        <p class="current">Putting the folders together…</p>
      {:else}
        <p class="current" title={d.progress.current}>Reading {d.progress.current}</p>
        <p class="note">Only folder listings are read: no file is opened, and cloud files are never downloaded.</p>
      {/if}
    {/if}
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
    {#if d.done.method === "mft"}
      <span title="Read straight from the drive's file table"><ShieldCheck size={13} /> Read from the file table in {seconds(d.done.elapsedMs)}</span>
    {:else}
      <span>Scanned in {seconds(d.done.elapsedMs)}</span>
    {/if}
    {#if d.done.fallback}
      <span class="warn" title={d.done.fallback}><CircleAlert size={13} /> Fast scan unavailable, so it went folder by folder</span>
    {/if}
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
    <p>
      On an NTFS drive the fast scan reads the file table directly, a whole drive in seconds, after one administrator approval. The normal scan reads folder listings without it. Nothing is changed until you choose to move
      something to the Recycle Bin.
    </p>
  </section>
{/if}

<style>
  .scan-picker {
    overflow: hidden;
    margin-bottom: var(--fluid-gap-3);
    border-radius: 16px;
  }
  .picker-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    padding: 14px 18px 0;
  }
  .picker-header h2 { margin: 0; font-size: 15px; font-weight: 700; color: var(--text-1); }
  .picker-options { display: flex; align-items: center; flex-wrap: wrap; gap: 12px 20px; }
  .picker-option { display: flex; align-items: center; gap: 8px; }
  .picker-option > span { font-size: 11px; color: var(--text-3); }
  .drive-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 200px), 1fr));
    gap: 12px;
    padding: 16px 18px;
  }
  .drive-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
    padding: 15px;
    border: 1px solid var(--btn-border);
    border-radius: 12px;
    background: var(--input-fill);
    color: var(--text-1);
    text-align: left;
    cursor: pointer;
    transition: background var(--dur-fast), border-color var(--dur-fast);
  }
  .drive-card:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.45); background: var(--chip-fill-hover); }
  .drive-card.selected { border-color: rgb(var(--accent-rgb) / 0.85); background: var(--selected); }
  .drive-card:focus-visible, .folder-clear-btn:focus-visible { outline: 2px solid rgb(var(--accent-rgb)); outline-offset: 3px; }
  .drive-card:disabled:not(.selected) { opacity: 0.55; cursor: not-allowed; }
  .card-head { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .drive-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: var(--hover);
    color: var(--text-2);
    flex-shrink: 0;
  }
  .selected .drive-icon { color: rgb(var(--accent-soft-rgb)); background: rgb(var(--accent-rgb) / 0.15); }
  .drive-name { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
  .drive-name strong { font-size: 16px; line-height: 1.15; }
  .drive-name > span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--text-2); font-size: 12px; }
  .active-indicator { display: inline-flex; align-items: center; justify-content: center; width: 20px; height: 20px; border: 1px solid var(--btn-border); border-radius: 50%; flex-shrink: 0; }
  .active-indicator.chosen { border-color: transparent; background: var(--btn-primary); color: var(--btn-primary-text); }
  .card-capacity { display: flex; align-items: baseline; justify-content: space-between; flex-wrap: wrap; gap: 3px 8px; font-size: 11.5px; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .card-capacity strong { color: var(--text-1); font-size: 18px; font-weight: 700; }
  .drive-total { color: var(--text-3); }
  .meter-bar { height: 6px; border-radius: 999px; background: var(--hover); overflow: hidden; }
  .meter-fill { height: 100%; border-radius: inherit; background: rgb(var(--accent-rgb) / 0.5); transition: width var(--dur-med); }
  .selected .meter-fill { background: rgb(var(--accent-rgb)); }
  .drive-card .meter-fill.warning { background: #d29a4a; }
  .drive-card .meter-fill.critical { background: var(--danger); }
  .card-footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: -5px; color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; }
  .capacity-alert { color: var(--danger); }
  .picker-footer { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 14px; padding: 13px 18px; border-top: 1px solid var(--btn-border); }
  .selected-target { display: flex; align-items: center; gap: 10px; min-width: 0; flex: 1 1 160px; color: rgb(var(--accent-soft-rgb)); }
  .target-copy { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .target-copy > span { color: var(--text-3); font-size: 10.5px; }
  .target-copy strong { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--text-1); font-size: 12.5px; font-weight: 600; direction: ltr; }
  .picker-actions { display: flex; gap: 9px; flex-shrink: 0; }
  .scan-action-btn, .folder-pick-btn { min-height: 38px; padding: 0 15px; gap: 7px; font-weight: 600; }
  .scan-action-btn { min-width: 125px; }
  .folder-clear-btn { display: inline-flex; align-items: center; justify-content: center; width: 25px; height: 25px; flex-shrink: 0; border: 1px solid var(--btn-border); border-radius: 7px; color: var(--text-2); background: transparent; cursor: pointer; }
  .folder-clear-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  :global(.spin) { animation: da-spin 1.5s linear infinite; }
  @keyframes da-spin { to { transform: rotate(360deg); } }
  @media (max-width: 600px) {
    .picker-header { padding: 12px 12px 0; }
    .drive-grid { gap: 9px; padding: 12px; }
    .picker-footer { padding: 12px; }
    .picker-actions { width: 100%; }
    .picker-actions .btn { flex: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.spin) { animation: none; }
    .drive-card, .meter-fill { transition: none; }
  }

  .mode { display: inline-flex; padding: 2px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--input-fill); }
  .mode button { height: 26px; padding: 0 10px; border-radius: 7px; color: var(--text-2); font-size: 12px; font-weight: 600; }
  .mode button:hover:not(:disabled) { color: var(--text-1); }
  .mode button.active { background: var(--selected); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .banner { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; padding: 9px 12px; border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; font-size: var(--fluid-sm); border-radius: 10px; }
  .scanning { display: grid; gap: 8px; margin-bottom: var(--fluid-gap-3); padding: var(--fluid-gap-3); border-radius: 12px; animation: fade-in var(--dur-med) var(--ease-out) both; }
  .counts { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 18px; color: var(--text-2); font-size: var(--fluid-sm); font-variant-numeric: tabular-nums; }
  .counts strong { color: var(--text-1); font-size: var(--fluid-md); }
  .counts em { overflow: hidden; color: rgb(var(--accent-soft-rgb)); font-style: normal; white-space: nowrap; text-overflow: ellipsis; }
  .current { overflow: hidden; color: var(--text-3); font-size: var(--fluid-xs); white-space: nowrap; text-overflow: ellipsis; }
  .phase { display: flex; align-items: center; gap: 8px; color: var(--text-1); font-size: var(--fluid-sm); font-weight: 600; }
  .phase :global(svg) { color: rgb(var(--accent-soft-rgb)); }
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
