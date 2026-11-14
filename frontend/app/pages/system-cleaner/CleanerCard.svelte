<script lang="ts">
  import FileClock from "@lucide/svelte/icons/file-clock";
  import FolderDown from "@lucide/svelte/icons/folder-down";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import ImageIcon from "@lucide/svelte/icons/image";
  import Lock from "@lucide/svelte/icons/lock";
  import Radar from "@lucide/svelte/icons/radar";
  import Recycle from "@lucide/svelte/icons/recycle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Zap from "@lucide/svelte/icons/zap";
  import { formatSize, type CleanerCategory } from "./api";
  import { DOWNLOADS } from "./downloads";
  import { cleanerState } from "./state.svelte";
  import { health } from "./health.svelte";

  let { category, index }: { category: CleanerCategory; index: number } = $props();
  const disabledReasonId = $props.id();
  const isDownloads = $derived(category.id === DOWNLOADS);
  const downloadCount = $derived(cleanerState.downloads?.items.length ?? 0);

  const icons: Record<string, typeof HardDrive> = {
    temp: FileClock, prefetch: Zap, "recycle-bin": Recycle, update: RefreshCw, thumbnails: ImageIcon, errors: TriangleAlert, downloads: FolderDown,
  };
  const Icon = $derived(icons[category.icon] ?? HardDrive);
  const measured = $derived(cleanerState.sizes[category.id]);
  const checked = $derived(cleanerState.isTicked(category.id));
  const requiresAdmin = $derived(cleanerState.requiresAdmin(category));
  const scanningSystemFiles = $derived(
    (cleanerState.phase === "scanning-admin" || cleanerState.phase === "scanning-all") && category.mayNeedAdmin,
  );
  const pending = $derived(
    ((cleanerState.phase === "scanning" || cleanerState.phase === "scanning-all") && !requiresAdmin && !measured) ||
    (scanningSystemFiles && category.adminOnly),
  );
  const locked = $derived(measured?.locked ?? false);
  const limitedToUser = $derived(category.mayNeedAdmin && !cleanerState.adminGranted && !category.adminOnly);
  /** Emptied since the last scan: off until the next one measures it again. */
  const done = $derived(cleanerState.isCleaned(category.id));
  const disabledReason = $derived.by(() => {
    if (pending || (cleanerState.scanning && !requiresAdmin)) return "Wait for the scan to finish.";
    if (cleanerState.phase === "cleaning") return "Wait for the cleanup to finish.";
    if (health.scanning) return "Wait for the Software Health scan to finish.";
    if (health.busy) return "Wait for the Software Health operation to finish.";
    if (requiresAdmin) return cleanerState.appElevated
      ? "Run Scan to measure this system folder."
      : "Choose Scan system folders and approve administrator access to scan this category.";
    if (!cleanerState.scanned) return "Run a scan before selecting this category.";
    if (done) return "This category was already processed. Scan again to select it.";
    if (!measured) return "Scan again to measure this category before selecting it.";
    if (isDownloads && !downloadCount) return "No Downloads items to delete.";
    return "";
  });
  const canTick = $derived(!disabledReason);
</script>

<article class="card" class:blocked={!canTick} class:on={checked} class:done class:admin={requiresAdmin} class:scanning={pending} class:working={cleanerState.phase === "cleaning" && checked} style:--scan-delay="{index * 90}ms" data-tip-override={disabledReason}>
  <!-- The whole head ticks the box: a big target, and the cursor shows when it cannot. -->
  <label class="head">
    <span class="icon"><Icon size={18} strokeWidth={1.7} /></span>
    <span class="titles">
      <span class="name">{category.title}{#if category.adminOnly}<span class="admin-tag"><ShieldCheck size={10} /> Admin</span>{/if}</span>
      <span class="desc">{category.description}</span>
    </span>
    {#if pending}
      <span class="scan-state"><Radar size={14} /> Scanning</span>
    {:else if measured && !requiresAdmin}
      <strong class="size measured">{formatSize(measured.bytes)}</strong>
    {:else}
      <span class="size unread">—</span>
    {/if}
    <input class="check" type="checkbox" aria-label="Include {category.title}" aria-describedby={canTick ? undefined : disabledReasonId} {checked} disabled={!canTick} onchange={() => cleanerState.toggle(category.id)} />
  </label>
  {#if !done}
    <div class="foot">
      <span class="hint" data-tip={limitedToUser && measured ? "Choose Scan system folders to include shared Windows files." : category.hint}>
        {#if scanningSystemFiles && !category.adminOnly}
          {#if measured}{measured.files.toLocaleString()} user files <span aria-hidden="true">·</span>{/if}
          <span class="scan-state"><Radar size={12} /> Scanning system files</span>
        {:else if requiresAdmin && !pending}
          <Lock size={11} /> System folder · not scanned
        {:else if !pending && measured && limitedToUser}
          {measured.files.toLocaleString()} user files
        {:else if !pending && locked}
          <Lock size={11} /> Some files could not be read
        {:else if isDownloads && cleanerState.scanned}
          {#if downloadCount}
            {downloadCount.toLocaleString()} {downloadCount === 1 ? "item" : "items"} to delete permanently
          {:else}
            No items eligible for cleanup
          {/if}
        {:else if measured?.files}
          {measured.files.toLocaleString()} files · {category.hint}
        {:else}
          {category.hint}
        {/if}
      </span>
    </div>
  {/if}
  {#if disabledReason}<span id={disabledReasonId} hidden>{disabledReason}</span>{/if}
</article>

<style>
  .card { position: relative; isolation: isolate; display: flex; flex-direction: column; gap: 10px; min-width: 0; padding: 16px; border: 1px solid var(--glass-border); border-radius: 12px; background: var(--surface-fill); box-shadow: var(--surface-depth); transition: border-color var(--dur-fast); }
  .card:where(:not(.blocked)):hover { border-color: rgb(255 255 255 / 0.14); background: rgb(255 255 255 / 0.04); }
  .card.blocked, .card.blocked .head { cursor: not-allowed; }
  .card.on:not(.blocked) { border-color: rgb(var(--accent-rgb) / 0.44); background: linear-gradient(120deg, rgb(var(--accent-rgb) / 0.075), transparent 65%), var(--surface-fill); }
  .card.on:not(.blocked):hover { border-color: rgb(var(--accent-rgb) / 0.62); }
  .card:has(.check:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .card.working { border-color: rgb(var(--accent-rgb) / 0.55); }
  .card.scanning { border-color: rgb(var(--accent-rgb) / 0.42); overflow: hidden; }
  .card.scanning::before { content: ""; position: absolute; z-index: -1; inset: 0; opacity: 0; pointer-events: none; background: linear-gradient(110deg, transparent 31%, rgb(var(--accent-rgb) / 0.16) 50%, transparent 68%); animation: scan-sweep 1.7s linear var(--scan-delay) infinite; }
  .scan-state { display: inline-flex; align-items: center; gap: 5px; flex: none; color: rgb(var(--accent-soft-rgb)); font-size: var(--fluid-xs); font-weight: 600; }
  .scan-state :global(svg) { animation: scan-radar 1.7s linear infinite; }
  .size.measured { animation: scan-result 420ms var(--ease-out) both; }
  .size.unread { color: var(--text-3); }
  @keyframes scan-sweep { 0% { transform: translateX(-75%); opacity: 0; } 20%, 80% { opacity: 1; } 100% { transform: translateX(75%); opacity: 0; } }
  @keyframes scan-radar { to { transform: rotate(1turn); } }
  @keyframes scan-result { from { opacity: 0; transform: translateY(4px); color: rgb(var(--accent-soft-rgb)); } }
  @media (prefers-reduced-motion: reduce) {
    .card.scanning::before { display: none; }
    .scan-state :global(svg), .size.measured { animation: none; }
  }
  .card.done .desc { color: var(--text-3); }
  /* System folders without administrator approval stay fainter until an enabled card is hovered or a control receives focus. */
  .card.admin:where(:not(.scanning)) { opacity: 0.58; transition: border-color var(--dur-fast), opacity var(--dur-med) var(--ease-out); }
  .card.admin:where(:not(.blocked)):hover, .card.admin:where(:not(.scanning)):focus-within { opacity: 0.92; }
  .admin-tag { display: inline-flex; align-items: center; gap: 3px; margin-left: 7px; padding: 1px 6px; border-radius: 999px; background: color-mix(in srgb, var(--warn) 13%, transparent); color: var(--warn); font-size: 10px; font-weight: 600; line-height: 1.4; vertical-align: 1px; white-space: nowrap; }
  .head { display: flex; align-items: flex-start; gap: 10px; cursor: pointer; }
  .head .check { margin-top: 1px; }
  .icon { display: grid; place-items: center; width: 32px; height: 32px; flex: none; border-radius: 9px; background: rgb(125 211 252 / .09); color: #7dd3fc; box-shadow: inset 0 1px 0 rgb(255 255 255 / .1); }
  .card:nth-child(2) .icon { color: #f5c26b; background: rgb(245 194 107 / .08); }
  .card:nth-child(3) .icon { color: #98dfbd; background: rgb(152 223 189 / .08); }
  .card:nth-child(5) .icon { color: #c4b5fd; background: rgb(196 181 253 / .08); }
  .titles { flex: 1; min-width: 0; }
  .name { display: block; overflow-wrap: anywhere; font-size: var(--fluid-sm); font-weight: 600; line-height: 1.25; }
  .desc { display: block; margin-top: 2px; color: var(--text-2); font-size: var(--fluid-xs); line-height: 1.3; }
  .size { flex: none; font-size: var(--fluid-sm); font-weight: 600; font-variant-numeric: tabular-nums; }
  .foot { display: flex; align-items: center; margin-top: auto; padding-top: var(--fluid-gap-2); border-top: 1px solid rgb(255 255 255 / 0.05); }
  .hint { display: flex; align-items: center; gap: 4px; color: var(--text-2); font-size: var(--fluid-xs); flex-wrap: wrap; }
</style>
