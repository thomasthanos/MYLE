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
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Usb from "@lucide/svelte/icons/usb";
  import X from "@lucide/svelte/icons/x";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { deletionShare, formatSize, sizeOf, type Drive } from "./api";
  import FileTypes from "./FileTypes.svelte";
  import FolderTree from "./FolderTree.svelte";
  import LargestFiles from "./LargestFiles.svelte";
  import { diskState as d, type View } from "./state.svelte";
  import Treemap from "./Treemap.svelte";
  import CloudStorage from "./cloud-storage/CloudStorage.svelte";

  onMount(() => void d.load());

  let layoutMode = $state<"split" | "treemap" | "list">("split");
  let showDrives = $state(false);
  const drivesExpanded = $derived(!d.done || showDrives);
  const deletingShare = $derived(deletionShare(d.deleting));

  const tabs: { id: View; label: string; icon: typeof Files }[] = [
    { id: "folders", label: "Folders", icon: FolderTreeIcon },
    { id: "files", label: "Largest files", icon: Files },
    { id: "types", label: "File types", icon: ChartPie },
  ];

  const scanning = $derived(d.phase === "scanning");
  const drivePicked = $derived(!!d.targetDrive);
  const folderPicked = $derived(!!d.target && !drivePicked);
  const modeLabel = $derived(d.sizeMode === "alloc" ? "on disk" : "in size");
  const currentCrumb = $derived(d.trail.length ? d.trail[d.trail.length - 1] : null);

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

<section class="scan-picker surface" class:compact={!drivesExpanded} aria-label="Scan target">
  <header class="picker-header">
    <div class="picker-title-area">
      <span class="picker-symbol"><HardDrive size={19} /></span>
      <div class="picker-copy"><h2>Scan target</h2><p>{d.done ? d.target ?? "Choose a drive or a folder." : "Choose a drive or a folder."}</p></div>
      {#if d.done}
        <button type="button" class="btn ghost small drive-toggle" aria-expanded={drivesExpanded} aria-controls="da-drive-grid" onclick={() => showDrives = !showDrives}>
          {drivesExpanded ? "Hide drives" : "Change drive"}
        </button>
      {/if}
      {#if folderPicked}
        <div class="folder-badge" title={d.target ?? ""}>
          <FolderOpen size={13} />
          <span class="folder-path">{d.target}</span>
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
          ><X size={12} /></button>
        </div>
      {/if}
    </div>

    <div class="picker-actions-bar">
      {#if d.targetDrive?.mftPossible}
        <div class="picker-option">
          <span class="option-label">Permissions</span>
          <div class="mode" role="radiogroup" aria-label="Scan permissions">
            <button
              type="button"
              role="radio"
              aria-checked={d.fast}
              class:active={d.fast}
              disabled={d.busy}
              title="Reads the NTFS file table directly (requires administrator approval)"
              onclick={() => d.setFast(true)}
            >Administrator</button>
            <button
              type="button"
              role="radio"
              aria-checked={!d.fast}
              class:active={!d.fast}
              disabled={d.busy}
              title="Scans folder by folder without administrator approval"
              onclick={() => d.setFast(false)}
            >Standard</button>
          </div>
        </div>
      {/if}

      <button type="button" class="btn folder-pick-btn" disabled={d.busy} onclick={() => d.pickFolder()} title="Choose a specific folder to scan">
        <FolderOpen size={14} /><span>Folder…</span>
      </button>

      {#if scanning}
        <button type="button" class="btn danger scan-action-btn" onclick={() => d.cancel()} title="Cancel scan">
          <X size={14} /><span>Cancel</span>
        </button>
      {:else}
        <button
          type="button"
          class="btn primary glow-cta scan-action-btn"
          disabled={!d.target || d.busy}
          title={d.fastScan ? "Windows will ask for administrator approval to read the NTFS file table" : "Scan folder by folder without administrator approval"}
          onclick={() => { showDrives = false; void d.scan(); }}
        >
          {#if d.fastScan}<ShieldCheck size={15} />{:else}<HardDrive size={15} />{/if}
          <span>{d.targetDrive ? (d.fastScan ? 'Scan ' + d.targetDrive.letter + ': (Admin)' : 'Scan ' + d.targetDrive.letter + ':') : folderPicked ? 'Scan folder' : 'Scan'}</span>
        </button>
      {/if}
    </div>
  </header>

  <div id="da-drive-grid" class="drive-grid" class:collapsed={!drivesExpanded} role="group" aria-label="Available storage drives">
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
        <div class="card-main">
          <span class="drive-icon-pill">{drive.letter}<small>:</small></span>
          <div class="drive-naming">
            <span class="drive-id">{drive.label || "Local Disk"}</span>
            <span class="drive-fs"><Icon size={12} />{drive.filesystem}</span>
          </div>
          <span class="drive-selection" aria-hidden="true">{#if isSelected}<Check size={12} />{/if}</span>
        </div>
        <div class="card-usage">
          <div class="usage-numbers"><strong>{formatSize(drive.free)}</strong><span>free</span></div>
          <span class="usage-total">of {formatSize(drive.total)}</span>
        </div>
        <div class="usage-meter">
          <div class="meter-bar" aria-hidden="true">
            <div class="meter-fill" class:critical={usedRatio > 0.9} class:warning={usedRatio > 0.75 && usedRatio <= 0.9} style:width="{usedRatio * 100}%"></div>
          </div>
          <span class="usage-pct" class:capacity-alert={pct > 90}>{pct}% used</span>
        </div>
      </button>
    {/each}
  </div>
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
      <div class="counts"><span>{d.deletingPermanently ? "Deleting permanently" : "Moving to the Recycle Bin"} · {Math.min(d.deleting.done + 1, d.deleting.total)} of {d.deleting.total}</span><em>{d.deleting.current}</em><span class="deletion-percent">{deletingShare === null ? "Working…" : `${Math.round(deletingShare * 100)}%`}</span></div>
      <ProgressBar value={deletingShare} label={d.deletingPermanently ? "Deleting permanently" : "Moving to the Recycle Bin"} />
    </section>
  {/if}
  {#if d.skipped.length && d.phase !== "deleting"}
    <section class="skipped surface" aria-label="Items left where they are">
      <div class="skipped-head">
        <CircleAlert size={16} />
        <span>{d.skipped.length === 1 ? "This item could not be removed" : `${d.skipped.length} items could not be removed`}</span>
        <button type="button" class="btn ghost small" onclick={() => d.dismissSkipped()}>Dismiss</button>
      </div>
      <ul>
        {#each d.skipped.slice(0, 50) as item (item.path)}
          <li>
            <div class="skipped-detail">
              <span class="path" title={item.path}>{item.path}</span>
              <span class="why">{item.reason.trim() || "Windows did not provide a reason. Try again, or open this item in File Explorer."}</span>
            </div>
            {#if item.canDeletePermanently}
              <button type="button" class="btn ghost small permanent-delete" disabled={d.busy} onclick={() => d.deletePermanently([item.id])}>
                <Trash2 size={14} /> Delete permanently…
              </button>
            {/if}
          </li>
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
          <div class="mode size-mode" role="radiogroup" aria-label="File size display">
            <button
              type="button"
              role="radio"
              aria-checked={d.sizeMode === "size"}
              class:active={d.sizeMode === "size"}
              disabled={d.phase === "deleting"}
              title="Logical size: actual data byte length of files"
              onclick={() => d.setSizeMode("size")}
            >Size</button>
            <button
              type="button"
              role="radio"
              aria-checked={d.sizeMode === "alloc"}
              class:active={d.sizeMode === "alloc"}
              disabled={d.phase === "deleting"}
              title="Size on disk: storage space taken on drive (clusters allocated)"
              onclick={() => d.setSizeMode("alloc")}
            >Size on disk</button>
          </div>
          <span
            class="crumb-size-pill"
            title={currentCrumb ? `Size: ${formatSize(currentCrumb.size)} · On disk: ${formatSize(currentCrumb.alloc)}` : undefined}
          >
            {formatSize(d.currentSize)} <small class="mode-tag">{modeLabel}</small>
          </span>
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
      <span title="Read straight from the NTFS file table with administrator approval"><ShieldCheck size={13} /> Scanned as Administrator in {seconds(d.done.elapsedMs)}</span>
    {:else}
      <span>Standard scan in {seconds(d.done.elapsedMs)}</span>
    {/if}
    {#if d.done.fallback}
      <span class="warn" title={d.done.fallback}><CircleAlert size={13} /> Administrator scan unavailable, so it ran as Standard</span>
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
      On an NTFS drive, Administrator mode reads the file table directly with Windows approval. Standard mode reads folder listings without elevation. Nothing is changed until you choose to move
      something to the Recycle Bin.
    </p>
  </section>
{/if}

<style>
  .scan-picker { overflow: hidden; flex-shrink: 0; margin-bottom: 12px; border-radius: 16px; border: 1px solid var(--glass-border); background: var(--bg-1); box-shadow: var(--surface-depth); }
  .drive-grid.collapsed { display: none; }
  .scan-picker.compact .picker-header { padding-block: 12px; border-bottom: 0; }
  .drive-toggle { white-space: nowrap; }
  .picker-header { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 14px 20px; padding: 16px 18px; border-bottom: 1px solid var(--panel-rim); }
  .picker-title-area { display: flex; align-items: center; flex-wrap: wrap; gap: 11px; min-width: 0; }
  .picker-symbol { display: grid; place-items: center; width: 38px; height: 38px; flex: none; border-radius: 11px; background: var(--hover); color: var(--icon-neutral); }
  .picker-copy h2 { margin: 0; font-size: 15px; line-height: 1.3; font-weight: 700; color: var(--text-1); }
  .picker-copy p { margin: 3px 0 0; font-size: 12px; line-height: 1.4; color: var(--text-3); }
  .folder-badge { display: inline-flex; align-items: center; gap: 7px; max-width: 250px; padding: 5px 8px; border-radius: 7px; background: var(--selected); color: rgb(var(--accent-soft-rgb)); font-size: 12px; }
  .folder-path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl; text-align: left; }
  .folder-clear-btn { display: grid; place-items: center; flex: none; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 5px; color: var(--text-2); background: transparent; cursor: pointer; }
  .folder-clear-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  .picker-actions-bar { display: flex; align-items: center; flex-wrap: wrap; gap: 9px; margin-left: auto; }
  .picker-option { display: flex; align-items: center; gap: 9px; margin-right: 5px; }
  .option-label { font-size: 12px; color: var(--text-3); }
  .mode { display: inline-flex; padding: 3px; border: 1px solid var(--panel-rim); border-radius: 10px; background: var(--input-fill); gap: 2px; }
  .mode button { height: 29px; padding: 0 11px; border-radius: 7px; border: 1px solid transparent; background: transparent; color: var(--text-3); font-size: 12px; font-weight: 600; cursor: pointer; transition: background var(--dur-fast), color var(--dur-fast); }
  .mode button:hover:not(:disabled) { color: var(--text-1); background: var(--hover); }
  .mode button.active { background: var(--btn-fill); border-color: var(--btn-border); color: rgb(var(--accent-soft-rgb)); }
  .folder-pick-btn, .scan-action-btn { display: inline-flex; align-items: center; justify-content: center; height: 37px; padding: 0 14px; gap: 7px; border-radius: 9px; font-size: 13px; font-weight: 700; white-space: nowrap; transition: background var(--dur-fast), border-color var(--dur-fast); }
  .folder-pick-btn { background: transparent; color: var(--text-2); border: 1px solid var(--glass-border); box-shadow: none; }
  .folder-pick-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); border-color: var(--btn-border); }
  .scan-action-btn.primary { background: var(--btn-primary); border: 1px solid rgb(var(--accent-rgb) / 0.55); color: var(--btn-primary-text); box-shadow: 0 2px 6px rgb(0 0 0 / 0.15); }
  .scan-action-btn.primary:hover:not(:disabled) { background: var(--btn-primary-hover); }
  .scan-action-btn.danger { background: var(--danger); color: #fff; }
  .scan-action-btn:disabled, .folder-pick-btn:disabled, .mode button:disabled { opacity: 0.5; cursor: default; }
  .drive-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 180px), 1fr)); gap: 10px; padding: 16px 18px 18px; }
  .drive-card { position: relative; display: flex; flex-direction: column; gap: 16px; min-width: 0; padding: 15px; border: 1px solid var(--glass-border); border-radius: 12px; background: var(--surface-fill); color: var(--text-1); text-align: left; cursor: pointer; box-shadow: none; transition: border-color var(--dur-fast), background var(--dur-fast); }
  .drive-card:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.55); background: var(--hover); }
  .drive-card.selected { border-color: rgb(var(--accent-rgb) / 0.85); background: var(--selected); box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.12); }
  .drive-card:focus-visible, .mode button:focus-visible, .folder-pick-btn:focus-visible, .scan-action-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  .drive-card:disabled:not(.selected) { opacity: 0.5; cursor: default; }
  .card-main { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .drive-icon-pill { display: inline-flex; align-items: baseline; justify-content: center; flex: none; width: 36px; height: 36px; padding-top: 5px; border-radius: 10px; background: var(--hover); color: var(--text-2); font-size: 23px; font-weight: 700; line-height: 1.1; }
  .drive-icon-pill small { font-size: 15px; padding-left: 1px; }
  .selected .drive-icon-pill { background: rgb(var(--accent-rgb) / 0.18); color: rgb(var(--accent-soft-rgb)); }
  .drive-naming { display: flex; flex: 1; flex-direction: column; gap: 4px; min-width: 0; }
  .drive-id { font-size: 14px; line-height: 1.2; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .drive-fs { display: inline-flex; align-items: center; gap: 5px; color: var(--text-3); font-size: 11px; line-height: 1.1; }
  .drive-selection { display: grid; place-items: center; flex: none; width: 17px; height: 17px; border: 1px solid var(--glass-border); border-radius: 50%; color: transparent; }
  .selected .drive-selection { background: var(--btn-primary); border-color: transparent; color: var(--btn-primary-text); }
  .card-usage { display: flex; align-items: baseline; justify-content: space-between; flex-wrap: wrap; gap: 3px 8px; font-variant-numeric: tabular-nums; }
  .usage-numbers { display: inline-flex; align-items: baseline; gap: 5px; color: var(--text-3); font-size: 12px; }
  .usage-numbers strong { color: var(--text-1); font-size: 21px; line-height: 1.1; font-weight: 700; letter-spacing: -0.02em; }
  .usage-total { color: var(--text-3); font-size: 11px; white-space: nowrap; }
  .usage-meter { display: flex; align-items: center; gap: 10px; }
  .meter-bar { flex: 1; height: 5px; border-radius: 99px; background: var(--input-fill); overflow: hidden; }
  .meter-fill { height: 100%; border-radius: inherit; background: var(--accent); transition: width var(--dur-med); }
  .drive-card .meter-fill.warning { background: #d5a65d; }
  .drive-card .meter-fill.critical { background: var(--danger); }
  .usage-pct { flex: none; min-width: 48px; font-size: 11px; color: var(--text-3); text-align: right; font-variant-numeric: tabular-nums; }
  .usage-pct.capacity-alert { color: var(--danger); }

  :global(.spin) { animation: da-spin 1.5s linear infinite; }
  @keyframes da-spin { to { transform: rotate(360deg); } }

  .banner { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; padding: 9px 12px; border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; font-size: var(--fluid-sm); border-radius: 10px; }
  .scanning { display: grid; gap: 8px; margin-bottom: var(--fluid-gap-3); padding: var(--fluid-gap-3); border-radius: 12px; animation: fade-in var(--dur-med) var(--ease-out) both; }
  .counts { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 18px; color: var(--text-2); font-size: var(--fluid-sm); font-variant-numeric: tabular-nums; }
  .counts strong { color: var(--text-1); font-size: var(--fluid-md); }
  .counts em { overflow: hidden; color: rgb(var(--accent-soft-rgb)); font-style: normal; white-space: nowrap; text-overflow: ellipsis; }
  .current { overflow: hidden; color: var(--text-3); font-size: var(--fluid-xs); white-space: nowrap; text-overflow: ellipsis; }
  .phase { display: flex; align-items: center; gap: 8px; color: var(--text-1); font-size: var(--fluid-sm); font-weight: 600; }
  .phase :global(svg) { color: rgb(var(--accent-soft-rgb)); }
  .note { color: var(--text-3); font-size: var(--fluid-xs); }
  .deletion-percent { margin-left: auto; flex-shrink: 0; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .scanning, .status { flex-shrink: 0; }
  .skipped { flex-shrink: 0; margin-bottom: var(--fluid-gap-2); padding: 12px 14px; font-size: var(--fluid-xs); border-radius: 12px; border-color: rgb(198 153 84 / 0.35); }
  .skipped-head { display: flex; align-items: center; gap: 8px; color: var(--text-2); }
  .skipped-head :global(svg) { flex-shrink: 0; color: #c69954; }
  .skipped-head > button { margin-left: auto; }
  .skipped ul { margin: 10px 0 0; padding: 0; list-style: none; display: grid; gap: 10px; max-height: 240px; overflow: auto; }
  .skipped li { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 18px; padding-top: 10px; border-top: 1px solid var(--glass-border); }
  .skipped-detail { flex: 1 1 300px; min-width: 0; display: grid; gap: 5px; }
  .skipped .path { color: var(--text-1); overflow-wrap: anywhere; }
  .skipped .why { color: var(--text-2); line-height: 1.5; }
  .permanent-delete { color: #e99595; border: 1px solid rgb(207 113 113 / 0.4); }
  .permanent-delete:hover { background: rgb(207 113 113 / 0.12); }

  .results { display: flex; flex-direction: column; gap: var(--fluid-gap-3); flex: 1 0 480px; min-height: 480px; }
  .crumbs-bar {
    padding: 6px 12px;
    border-radius: 10px;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
    flex-shrink: 0;
  }
  .back-btn:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text-1);
  }
  .back-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .crumb-list {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-x: auto;
    scrollbar-width: none;
    min-width: 0;
  }
  .crumb-list::-webkit-scrollbar {
    display: none;
  }
  .crumb-list li {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }
  :global(.crumb-sep) {
    color: var(--text-3);
    opacity: 0.6;
    flex-shrink: 0;
  }
  .crumb-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-2);
    font-size: 12px;
    cursor: pointer;
    max-width: 220px;
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
    gap: 8px;
    flex-shrink: 0;
    margin-left: auto;
  }
  .mode.size-mode {
    border-radius: 7px;
    padding: 2px;
  }
  .mode.size-mode button {
    height: 22px;
    padding: 0 7px;
    font-size: 11px;
    border-radius: 5px;
  }
  .crumb-size-pill {
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--chip-fill);
    color: var(--text-1);
    font-size: 11px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .crumb-size-pill .mode-tag {
    color: var(--text-3);
    font-size: 10px;
    font-weight: 500;
    margin-left: 2px;
  }

  .layout-switch {
    display: inline-flex;
    padding: 2px;
    border-radius: 7px;
    background: var(--input-fill);
    border: 1px solid var(--btn-border);
    gap: 2px;
  }
  .layout-toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 5px;
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

  .split { display: grid; gap: var(--fluid-gap-3); flex: 1; min-height: 420px; }
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
  .recycle { margin-left: auto; }
  .panel { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .status { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; margin-top: var(--fluid-gap-2); color: var(--text-3); font-size: var(--fluid-xs); font-variant-numeric: tabular-nums; }
  .status .warn { color: #f5bc5f; display: inline-flex; align-items: center; gap: 4px; }
  .status .hint { color: var(--text-3); display: inline-flex; align-items: center; gap: 4px; }
  .status .selected { margin-left: auto; color: rgb(var(--accent-soft-rgb)); font-weight: 600; }
  .empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; flex: 1; min-height: 260px; padding: var(--fluid-gap-4); border-radius: 12px; text-align: center; color: var(--text-3); }
  .empty h2 { color: var(--text-1); font-size: var(--fluid-md); }
  .empty p { max-width: 520px; font-size: var(--fluid-xs); line-height: 1.6; }

  @media (max-width: 1050px) {
    .split.mode-split { grid-template-columns: minmax(0, 1fr); flex: none; }
    .split.mode-split > .pane { min-height: 420px; }
  }

  @media (max-width: 768px) {
    .picker-header { padding: 13px; }
    .drive-grid { gap: 9px; padding: 12px; }
    .picker-actions-bar { width: 100%; margin-left: 0; }
    .picker-option { flex: 1 1 100%; margin-right: 0; justify-content: space-between; }
    .scan-action-btn, .folder-pick-btn { flex: 1; }
    .drive-card { padding: 12px; }
    .usage-numbers strong { font-size: 19px; }
  }
</style>
