<script lang="ts">
  import { onMount } from "svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChartPie from "@lucide/svelte/icons/chart-pie";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Files from "@lucide/svelte/icons/files";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Info from "@lucide/svelte/icons/info";
  import Network from "@lucide/svelte/icons/network";
  import RadarIcon from "@lucide/svelte/icons/radar";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Usb from "@lucide/svelte/icons/usb";
  import X from "@lucide/svelte/icons/x";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { formatSize, sizeOf, type Drive } from "./api";
  import FileTypes from "./FileTypes.svelte";
  import FolderTree from "./FolderTree.svelte";
  import LargestFiles from "./LargestFiles.svelte";
  import { diskState as d, type View } from "./state.svelte";
  import Treemap from "./Treemap.svelte";

  onMount(() => void d.load());

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

<section class="bar surface">
  <div class="drives" role="group" aria-label="Drives">
    {#each d.drives as drive (drive.root)}
      {@const used = drive.total ? (drive.total - drive.free) / drive.total : 0}
      {@const Icon = driveIcon(drive)}
      <button
        type="button"
        class="drive"
        class:active={d.target === drive.root}
        aria-pressed={d.target === drive.root}
        disabled={d.busy}
        title={driveTitle(drive)}
        onclick={() => d.choose(drive.root)}
        ondblclick={() => d.scan()}
      >
        <span class="drive-icon"><Icon size={16} /></span>
        <span class="drive-text">
          <span class="drive-name"><strong>{drive.letter}:</strong> {drive.label || "Local Disk"}</span>
          <span class="meter" aria-hidden="true"><span class:full={used > 0.9} style:width="{used * 100}%"></span></span>
          <span class="drive-free">{formatSize(drive.free)} free of {formatSize(drive.total)}</span>
        </span>
      </button>
    {/each}
    {#if folderPicked}
      <span class="picked" title={d.target}><FolderOpen size={15} /><span>{d.target}</span></span>
    {/if}
  </div>
  <div class="actions">
    <button type="button" class="btn" disabled={d.busy} onclick={() => d.pickFolder()}><FolderOpen size={15} /> Choose folder…</button>
    <div class="mode" role="radiogroup" aria-label="Sizes">
      <button type="button" role="radio" aria-checked={d.sizeMode === "size"} class:active={d.sizeMode === "size"} disabled={d.phase === "deleting"} title="What the files hold" onclick={() => d.setSizeMode("size")}>Size</button>
      <button type="button" role="radio" aria-checked={d.sizeMode === "alloc"} class:active={d.sizeMode === "alloc"} disabled={d.phase === "deleting"} title="What the files take on the disk" onclick={() => d.setSizeMode("alloc")}>Size on disk</button>
    </div>
    {#if scanning}
      <button type="button" class="btn primary" onclick={() => d.cancel()}><X size={15} /> Cancel</button>
    {:else}
      <button type="button" class="btn primary" disabled={!d.target || d.busy} onclick={() => d.scan()}><RadarIcon size={15} /> Scan</button>
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
  <div class="split">
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
    <section class="pane treemap surface" aria-label="Treemap">
      <nav class="crumbs" aria-label="Treemap folder">
        <button type="button" class="icon-btn" disabled={d.trail.length < 2} title="One folder up (Backspace)" aria-label="One folder up" onclick={() => d.zoomOut()}><ArrowUp size={15} /></button>
        <ol>
          {#each d.trail as crumb, index (crumb.id)}
            <li>
              {#if index > 0}<ChevronRight size={12} />{/if}
              <button type="button" aria-current={index === d.trail.length - 1 ? "location" : undefined} onclick={() => d.zoomTo(crumb.id)}>{crumb.name}</button>
            </li>
          {/each}
        </ol>
        <span class="crumb-size">{formatSize(d.currentSize)}</span>
      </nav>
      <Treemap />
    </section>
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
        {d.done.inaccessible.toLocaleString()} folder{d.done.inaccessible === 1 ? "" : "s"} could not be read (Windows keeps them for administrators or other users)
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
    <HardDrive size={28} />
    <h2>Pick a drive or a folder, then Scan</h2>
    <p>The scan reads folder listings only, so even a whole drive takes well under a minute. Nothing is changed until you choose to move something to the Recycle Bin.</p>
  </section>
{/if}

<style>
  .bar { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: var(--fluid-gap-2) var(--fluid-gap-3); margin-bottom: var(--fluid-gap-3); padding: var(--fluid-gap-2) var(--fluid-gap-3); }
  .drives { display: flex; flex-wrap: wrap; gap: 8px; min-width: 0; }
  .drive { display: flex; align-items: center; gap: 9px; min-width: 150px; padding: 7px 11px; border: 1px solid var(--btn-border); border-radius: 10px; background: var(--chip-fill); color: var(--text-2); text-align: left; box-shadow: var(--btn-sheen); transition: background var(--dur-fast), border-color var(--dur-fast); }
  .drive:hover:not(:disabled) { background: var(--chip-fill-hover); color: var(--text-1); }
  .drive.active { border-color: rgb(var(--accent-rgb) / 0.55); background: var(--selected); color: var(--text-1); }
  .drive:disabled { opacity: 0.55; cursor: not-allowed; }
  .drive-icon { display: grid; color: rgb(var(--accent-soft-rgb)); }
  .drive-text { display: grid; gap: 3px; min-width: 0; }
  .drive-name { overflow: hidden; font-size: 12.5px; white-space: nowrap; text-overflow: ellipsis; }
  .drive-free { color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; }
  .meter { display: block; height: 4px; border-radius: 999px; background: rgb(0 0 0 / 0.3); overflow: hidden; }
  .meter span { display: block; height: 100%; border-radius: inherit; background: var(--accent); }
  .meter span.full { background: var(--danger); }
  .picked { display: inline-flex; align-items: center; gap: 7px; max-width: 360px; padding: 0 11px; border: 1px solid rgb(var(--accent-rgb) / 0.55); border-radius: 10px; background: var(--selected); color: var(--text-1); font-size: 12.5px; }
  .picked span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; direction: ltr; }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-left: auto; }
  .mode { display: inline-flex; padding: 2px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--input-fill); }
  .mode button { height: 26px; padding: 0 10px; border-radius: 7px; color: var(--text-2); font-size: 12px; font-weight: 600; }
  .mode button:hover:not(:disabled) { color: var(--text-1); }
  .mode button.active { background: var(--selected); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .banner { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; padding: 9px 12px; border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; font-size: var(--fluid-sm); }
  .scanning { display: grid; gap: 8px; margin-bottom: var(--fluid-gap-3); padding: var(--fluid-gap-3); animation: fade-in var(--dur-med) var(--ease-out) both; }
  .counts { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 18px; color: var(--text-2); font-size: var(--fluid-sm); font-variant-numeric: tabular-nums; }
  .counts strong { color: var(--text-1); font-size: var(--fluid-md); }
  .counts em { overflow: hidden; color: rgb(var(--accent-soft-rgb)); font-style: normal; white-space: nowrap; text-overflow: ellipsis; }
  .current { overflow: hidden; color: var(--text-3); font-size: var(--fluid-xs); white-space: nowrap; text-overflow: ellipsis; }
  .note { color: var(--text-3); font-size: var(--fluid-xs); }
  .skipped { margin-bottom: var(--fluid-gap-2); padding: 8px 12px; font-size: var(--fluid-xs); }
  .skipped-head { display: flex; align-items: center; gap: 8px; color: var(--text-2); }
  .skipped-head .btn { margin-left: auto; }
  .skipped ul { display: grid; gap: 3px; max-height: 120px; margin: 6px 0 0; padding: 0; overflow: auto; list-style: none; }
  .skipped li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 12px; }
  .skipped .path { overflow: hidden; color: var(--text-1); white-space: nowrap; text-overflow: ellipsis; }
  .skipped .why { color: var(--text-3); }
  .results { display: flex; flex-direction: column; flex: 1; min-height: 0; container-type: inline-size; }
  .split { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr); gap: var(--fluid-gap-3); flex: 1; min-height: 0; }
  .pane { display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .tabs-row { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 6px; padding: 8px 10px 6px; border-bottom: 1px solid var(--glass-border); }
  .tabs { display: flex; gap: 4px; }
  .tabs button { display: flex; align-items: center; gap: 7px; padding: 6px 11px; border-radius: 8px; color: var(--text-2); font-size: 12.5px; }
  .tabs button:hover { background: var(--hover); color: var(--text-1); }
  .tabs button.active { background: var(--selected); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .tabs button :global(svg) { color: rgb(var(--accent-soft-rgb)); }
  .recycle:disabled { opacity: 0.4; }
  .panel { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .treemap { gap: 8px; padding: 8px; }
  .crumbs { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .crumbs .icon-btn { width: 26px; height: 26px; }
  .crumbs ol { display: flex; align-items: center; flex: 1; gap: 2px; min-width: 0; margin: 0; padding: 0; overflow: hidden; list-style: none; }
  .crumbs li { display: flex; align-items: center; gap: 2px; min-width: 0; color: var(--text-3); }
  .crumbs li:last-child { flex-shrink: 0; }
  .crumbs li button { overflow: hidden; max-width: 220px; padding: 3px 6px; border-radius: 6px; color: var(--text-2); font-size: 12px; white-space: nowrap; text-overflow: ellipsis; }
  .crumbs li button:hover { background: var(--hover); color: var(--text-1); }
  .crumbs li button[aria-current] { color: var(--text-1); font-weight: 600; }
  .crumb-size { flex: none; color: var(--text-3); font-size: 12px; font-variant-numeric: tabular-nums; }
  .status { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 16px; margin-top: var(--fluid-gap-2); color: var(--text-3); font-size: var(--fluid-xs); font-variant-numeric: tabular-nums; }
  .status span { display: inline-flex; align-items: center; gap: 5px; }
  .status .warn { color: #e6b477; }
  .status .selected { margin-left: auto; color: var(--text-2); }
  .empty { display: grid; justify-items: center; gap: 8px; margin-top: var(--fluid-gap-4); padding: 36px 24px; color: var(--text-3); text-align: center; }
  .empty :global(svg) { color: rgb(var(--accent-soft-rgb)); }
  .empty h2 { color: var(--text-1); font-size: var(--fluid-md); }
  .empty p { max-width: 60ch; font-size: var(--fluid-sm); line-height: 1.5; }
  @container (max-width: 900px) {
    .split { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(260px, 1fr) minmax(260px, 1fr); }
  }
  @keyframes fade-in { from { opacity: 0; } }
</style>
