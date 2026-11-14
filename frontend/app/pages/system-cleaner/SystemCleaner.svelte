<script lang="ts">
  import { onMount, tick } from "svelte";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RadarIcon from "@lucide/svelte/icons/radar";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import HeartPulse from "@lucide/svelte/icons/heart-pulse";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import { formatSize } from "./api";
  import CleanerCard from "./CleanerCard.svelte";
  import CleanOverlay from "./CleanOverlay.svelte";
  import SoftwareHealth from "./SoftwareHealth.svelte";
  import DiskAnalyzer from "../disk-analyzer/DiskAnalyzer.svelte";
  import { cleanerState } from "./state.svelte";
  import { health } from "./health.svelte";

  const tools = [
    { id: "cleanup", label: "Cleanup", icon: Sparkles, color: "#7dd3fc" },
    { id: "health", label: "Software Health", icon: HeartPulse, color: "#c4b5fd" },
    { id: "disk", label: "Disk Analyzer", icon: HardDrive, color: "#fbbf24" },
  ] as const;
  type Tool = (typeof tools)[number]["id"];
  let tab = $state<Tool>("cleanup");
  let diskOpened = $state(false);
  let scanButton: HTMLButtonElement;
  let toolbar: HTMLElement;

  function selectTab(id: Tool) {
    tab = id;
    if (id === "disk") diskOpened = true;
  }

  function onTabKey(event: KeyboardEvent, index: number) {
    const next = event.key === "ArrowRight" ? (index + 1) % tools.length
      : event.key === "ArrowLeft" ? (index - 1 + tools.length) % tools.length
      : event.key === "Home" ? 0 : event.key === "End" ? tools.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault();
    selectTab(tools[next].id);
    document.getElementById(`${tools[next].id}-tab`)?.focus();
  }
  const locked = $derived(cleanerState.busy || health.busy || health.scanning);
  const showCleanup = $derived(cleanerState.phase === "cleaning" || cleanerState.celebration !== null);

  onMount(() => void cleanerState.load());

  const lastCleanedLabel = $derived.by(() => {
    const at = cleanerState.lastCleaned;
    if (!at) return "Never";
    const minutes = Math.floor((Date.now() - at) / 60000);
    if (minutes < 1) return "Just now";
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
    const days = Math.floor(hours / 24);
    return `${days} day${days === 1 ? "" : "s"} ago`;
  });

  const lastCleanedTitle = $derived(cleanerState.lastCleaned ? new Date(cleanerState.lastCleaned).toLocaleString() : "No cleanup yet");
  const selectedCount = $derived(cleanerState.selection.length);

  async function closeCleanup(rescan = false) {
    cleanerState.dismissCelebration();
    await tick();
    if (rescan) {
      toolbar.focus({ preventScroll: true });
      void cleanerState.scan();
    } else if (document.activeElement === document.body) {
      scanButton.focus({ preventScroll: true });
    }
  }
</script>

<PageHeader title="System Cleaner" subtitle="Clear temporary files, clean software leftovers and see what is filling your drives." />

<div class="tabs" role="tablist" aria-label="Cleaner tools" inert={showCleanup}>
  {#each tools as tool, index (tool.id)}
    <button type="button" id="{tool.id}-tab" role="tab" aria-selected={tab === tool.id} aria-controls="{tool.id}-panel" tabindex={tab === tool.id ? 0 : -1} class:active={tab === tool.id} style:--tool-color={tool.color} onclick={() => selectTab(tool.id)} onkeydown={(event) => onTabKey(event, index)}><tool.icon size={17} /> {tool.label}</button>
  {/each}
</div>

<div id="cleanup-panel" class="tool-panel" role="tabpanel" aria-labelledby="cleanup-tab" hidden={tab !== "cleanup"} inert={showCleanup}>

<section class="hero surface" aria-label="Cleanup controls" tabindex="-1" bind:this={toolbar}>
  <div class="stats">
    <div class="stat">
      <span class="label">Available to clean</span>
      <strong class="value" class:shimmer={cleanerState.phase === "scanning" || cleanerState.phase === "scanning-all"}>{cleanerState.scanned || cleanerState.total > 0 ? formatSize(cleanerState.total) : "—"}</strong>
    </div>
    <div class="stat">
      <span class="label">Selected{#if selectedCount} · {selectedCount}{/if}</span>
      <strong class="value" class:accent={selectedCount > 0}>{formatSize(cleanerState.selectedBytes)}</strong>
    </div>
    <div class="stat"><span class="label">Last cleaned</span><strong class="value small" title={lastCleanedTitle}>{lastCleanedLabel}</strong></div>
  </div>

  <div class="actions">
    <div class="action-group" role="group" aria-label="Scan">
      <button class="btn" type="button" bind:this={scanButton} disabled={locked || cleanerState.appElevated === null} onclick={() => cleanerState.scan()}><RadarIcon size={15} /> Scan</button>
      {#if cleanerState.canScanSystemFolders}
        <button class="btn" type="button" disabled={locked} onclick={() => cleanerState.allowAdmin()}><ShieldCheck size={15} /> Scan system folders</button>
      {/if}
    </div>
    <div class="action-group" role="group" aria-label="Selection and cleanup">
      <button class="btn select-all" type="button" disabled={locked || !cleanerState.bulkSelectable.length} title={cleanerState.scanned ? "Select Downloads from its card." : "Run a scan to see the available categories."} onclick={() => cleanerState.toggleAll()}>
        <CheckCheck size={15} />
        {cleanerState.allTicked ? "Deselect all" : "Select all"}
      </button>
      <button class="btn primary" type="button" disabled={locked || !selectedCount} onclick={() => cleanerState.clean()}>
        <Sparkles size={15} />
        Clean selected
      </button>
    </div>
  </div>

  <div class="hero-footer">
    <p class="status" role="status">
      {#if cleanerState.phase !== "idle"}<LoaderCircle size={13} class="spin" />{:else if cleanerState.scanned}<CheckCheck size={13} />{/if}
      {cleanerState.status}
    </p>
    <button class="sound" type="button" aria-label="Cleanup sound" aria-pressed={cleanerState.soundEnabled} title={cleanerState.soundEnabled ? "Mute cleanup sound" : "Enable cleanup sound"} onclick={() => cleanerState.toggleSound()}>
      {#if cleanerState.soundEnabled}<Volume2 size={15} />{:else}<VolumeX size={15} />{/if}
    </button>
  </div>
</section>

{#if cleanerState.error}
  <div class="banner surface" role="alert"><CircleAlert size={18} /><span>{cleanerState.error}</span></div>
{/if}

<div class="grid">
  {#each cleanerState.categories as category, index (category.id)}
    <CleanerCard {category} {index} />
  {/each}
</div>

</div>
<div id="health-panel" class="tool-panel" role="tabpanel" aria-labelledby="health-tab" hidden={tab !== "health"}><SoftwareHealth /></div>
<div id="disk-panel" class="tool-panel disk-panel" role="tabpanel" aria-labelledby="disk-tab" hidden={tab !== "disk"}>
  {#if diskOpened}<DiskAnalyzer active={tab === "disk"} />{/if}
</div>

{#if showCleanup}
  <CleanOverlay
    summary={cleanerState.celebration}
    onclose={() => void closeCleanup()}
    onscan={() => void closeCleanup(true)}
  />
{/if}

<style>
  .tabs { display: flex; flex-wrap: wrap; flex-shrink: 0; gap: 6px; margin: 0 0 20px; border-bottom: 1px solid var(--divider); padding-bottom: 8px; }
  .tabs button { display: flex; align-items: center; gap: 8px; padding: 9px 14px; border: 0; border-radius: 9px; background: transparent; color: var(--text-2); font: inherit; cursor: pointer; transition: background var(--dur-fast), box-shadow var(--dur-fast); }
  .tabs button:hover { background: var(--control-face), var(--hover); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .tabs button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .tabs button.active { background: var(--control-face), var(--selected); color: var(--text-1); box-shadow: var(--btn-sheen); }
  .tabs button:active { background-image: var(--control-face-press); box-shadow: var(--btn-sheen-press); }
  .tabs button :global(svg) { flex: none; color: var(--tool-color); }
  .tool-panel { flex: 0 0 auto; min-width: 0; }
  .disk-panel:not([hidden]) { display: flex; flex-direction: column; flex: 1 0 auto; min-height: 0; }
  .hero { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px var(--fluid-gap-4); margin-bottom: var(--fluid-gap-3); padding: var(--fluid-gap-3) var(--fluid-gap-4) 6px; }
  .hero:focus { outline: none; }
  .stats { display: flex; align-items: flex-end; flex-wrap: wrap; gap: 9px clamp(12px, 3vw, 34px); }
  .stat { display: grid; gap: 3px; }
  .label { color: var(--text-2); font-size: var(--fluid-xs); font-weight: 500; }
  .value { font-size: var(--fluid-lg); font-weight: 600; line-height: 1.15; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
  .value.small { font-size: var(--fluid-md); font-weight: 500; line-height: 1.35; }
  .value.accent { color: rgb(var(--accent-soft-rgb)); }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 16px; margin-left: auto; }
  .action-group { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; }
  .select-all:not(:hover) { background: transparent; box-shadow: none; }
  .hero-footer { display: flex; align-items: center; justify-content: space-between; flex: 1 0 100%; gap: 8px; min-width: 0; padding-top: 4px; border-top: 1px solid var(--divider); }
  .status { display: flex; align-items: center; gap: 6px; color: var(--text-2); font-size: var(--fluid-xs); }
  .status :global(svg) { flex: none; }
  .sound { flex: none; display: grid; place-items: center; width: 32px; height: 30px; border: 1px solid transparent; border-radius: 8px; background: none; color: var(--text-2); cursor: pointer; }
  .sound:hover { border-color: var(--divider); background-color: var(--hover); color: var(--text-1); }
  .banner { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; padding: 9px 12px; border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; font-size: var(--fluid-md); }
  /* Columns follow the window: ~3 at 1080p, 4 at 1440p, 5 at 4K. */
  .grid { --cols: 3; display: grid; grid-template-columns: repeat(var(--cols), minmax(0, 1fr)); align-items: stretch; gap: var(--fluid-gap-3); }
  @media (max-width: 760px) { .grid { --cols: 1; } }
  @media (min-width: 761px) and (max-width: 1199px) { .grid { --cols: 2; } }
  @media (min-width: 2200px) { .grid { --cols: 4; } }
  @media (min-width: 3200px) { .grid { --cols: 5; } }

  @media (max-width: 860px) { .actions { margin-left: 0; } }
</style>
