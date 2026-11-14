<script lang="ts">
  import CheckCheck from "@lucide/svelte/icons/check-check";
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
  import DownloadsCard from "./DownloadsCard.svelte";
  import { health } from "./health.svelte";

  let { category, index }: { category: CleanerCategory; index: number } = $props();
  const disabledReasonId = $props.id();

  const icons: Record<string, typeof HardDrive> = {
    temp: FileClock, prefetch: Zap, "recycle-bin": Recycle, update: RefreshCw, thumbnails: ImageIcon, errors: TriangleAlert, downloads: FolderDown,
  };
  const Icon = $derived(icons[category.icon] ?? HardDrive);
  const measured = $derived(cleanerState.sizes[category.id]);
  const checked = $derived(cleanerState.isTicked(category.id));
  const requiresAdmin = $derived(cleanerState.requiresAdmin(category));
  const scanningSystemFiles = $derived(cleanerState.phase === "scanning-admin" && category.mayNeedAdmin);
  const pending = $derived(
    (cleanerState.phase === "scanning" && !requiresAdmin && !measured) ||
    (scanningSystemFiles && category.adminOnly),
  );
  const locked = $derived(measured?.locked ?? false);
  const limitedToUser = $derived(category.mayNeedAdmin && !cleanerState.adminGranted && !category.adminOnly);
  /** Emptied since the last scan: off until the next one measures it again. */
  const done = $derived(cleanerState.isCleaned(category.id));
  const outcome = $derived(cleanerState.outcome[category.id]);
  /* Cleaned, but not everything could go: files in use, or system files without administrator approval. */
  const partly = $derived(done && (measured?.bytes ?? 0) > 0);
  /* The bytes the run could not remove; an administrator pass already replaced its part of `skippedBytes` and zeroed `adminSkippedBytes`. */
  const remaining = $derived(done ? Math.max(outcome?.skippedBytes ?? 0, partly ? (measured?.bytes ?? 0) : 0) : 0);
  const adminBytes = $derived(Math.min(remaining, outcome?.adminSkippedBytes ?? 0));
  const inUse = $derived(Math.max(0, remaining - adminBytes));
  /** Only when the leftovers are the system folders the user did not approve. */
  const needsAdmin = $derived(done && category.mayNeedAdmin && locked && cleanerState.adminGranted !== true);
  const disabledReason = $derived.by(() => {
    if (pending || (cleanerState.scanning && !requiresAdmin)) return "Wait for the scan to finish.";
    if (cleanerState.phase === "cleaning") return "Wait for the cleanup to finish.";
    if (health.scanning) return "Wait for the Software Health scan to finish.";
    if (health.busy) return "Wait for the Software Health operation to finish.";
    if (requiresAdmin) return cleanerState.scanned
      ? "Choose Scan system folders and approve administrator access to scan this category."
      : "Run Scan, then choose Scan system folders to scan this category with administrator access.";
    if (!cleanerState.scanned) return "Run a scan before selecting this category.";
    if (done) return "This category was already processed. Scan again to select it.";
    if (!measured) return "Scan again to measure this category before selecting it.";
    if (category.id === DOWNLOADS && !cleanerState.downloads?.items.length) return "No Downloads items to delete.";
    return "";
  });
  const canTick = $derived(!disabledReason);
  const skippedFiles = $derived(outcome?.skipped ? ` (${outcome.skipped.toLocaleString()} ${outcome.skipped === 1 ? "file" : "files"})` : "");
</script>

<article class="card" class:blocked={!canTick} class:on={checked} class:done class:partly class:admin={requiresAdmin} class:scanning={pending} class:working={cleanerState.phase === "cleaning" && checked} style:--scan-delay="{index * 90}ms" data-tip-override={disabledReason}>
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
  <div class="foot">
    <span class="hint" data-tip={limitedToUser && measured ? "Choose Scan system folders to include shared Windows files." : category.hint}>
      {#if scanningSystemFiles && !category.adminOnly}
        {#if measured}{measured.files.toLocaleString()} user files <span aria-hidden="true">·</span>{/if}
        <span class="scan-state"><Radar size={12} /> Scanning system files</span>
      {:else if partly}
        <TriangleAlert size={11} />
        <span class="result freed">Freed {formatSize(outcome?.freed ?? 0)}</span>
        {#if inUse}<span class="result">In use: {formatSize(inUse)}{skippedFiles} · close the app and scan again</span>{/if}
        {#if needsAdmin}<span class="result">Administrator needed{adminBytes ? `: ${formatSize(adminBytes)}` : ""}</span>{/if}
      {:else if done}
        <CheckCheck size={11} />
        {outcome?.freed ? `Freed ${formatSize(outcome.freed)}` : "Cleaned"} · scan again to re-check
        {#if needsAdmin}<span class="result">System folders excluded</span>{/if}
      {:else if requiresAdmin && !pending}
        <Lock size={11} /> System folder · not scanned
      {:else if !pending && measured && limitedToUser}
        {measured.files.toLocaleString()} user files
      {:else if !pending && locked}
        <Lock size={11} /> Some files could not be read
      {:else if measured?.files}
        {measured.files.toLocaleString()} files · {category.hint}
      {:else}
        {category.hint}
      {/if}
    </span>
  </div>
  {#if category.id === DOWNLOADS && cleanerState.downloads}
    <details class="downloads-details"><summary>Review Downloads files</summary><DownloadsCard embedded /></details>
  {/if}
  {#if disabledReason}<span id={disabledReasonId} hidden>{disabledReason}</span>{/if}
</article>

<style>
  .card { position: relative; isolation: isolate; display: flex; flex-direction: column; gap: 10px; min-width: 0; padding: 16px; border: 1px solid var(--glass-border); border-radius: 12px; background: var(--surface-fill); box-shadow: var(--surface-depth); transition: border-color var(--dur-fast); }
  .card:where(:not(.blocked)):hover { border-color: rgb(255 255 255 / 0.14); background: rgb(255 255 255 / 0.04); }
  .card.blocked, .card.blocked .head { cursor: not-allowed; }
  .card.on { border-color: rgb(var(--accent-rgb) / 0.32); }
  .card.working { border-color: rgb(var(--accent-rgb) / 0.55); }
  .card.scanning { border-color: rgb(var(--accent-rgb) / 0.42); opacity: 1; }
  .card.scanning::before { content: ""; position: absolute; z-index: -1; inset: 0; border-radius: inherit; pointer-events: none; background: linear-gradient(110deg, transparent 33%, rgb(var(--accent-rgb) / 0.12) 49%, rgb(var(--accent-rgb) / 0.24) 50%, rgb(var(--accent-rgb) / 0.07) 57%, transparent 70%) 100% 0 / 240% 100%; animation: scan-sweep 1.7s linear var(--scan-delay) infinite; }
  .card.scanning .icon { animation: scan-icon 1.7s var(--ease-in-out) var(--scan-delay) infinite; }
  .scan-state { display: inline-flex; align-items: center; gap: 5px; flex: none; color: rgb(var(--accent-soft-rgb)); font-size: var(--fluid-xs); font-weight: 600; }
  .scan-state :global(svg) { animation: scan-radar 1.7s linear infinite; }
  .size.measured { animation: scan-result 420ms var(--ease-out) both; }
  .size.unread { color: var(--text-3); }
  @keyframes scan-sweep { from { background-position: 100% 0; } to { background-position: 0% 0; } }
  @keyframes scan-icon { 50% { box-shadow: 0 0 0 4px rgb(var(--accent-rgb) / 0.16), 0 0 20px rgb(var(--accent-rgb) / 0.25); } }
  @keyframes scan-radar { to { transform: rotate(1turn); } }
  @keyframes scan-result { from { opacity: 0; transform: translateY(4px); color: rgb(var(--accent-soft-rgb)); } }
  @media (prefers-reduced-motion: reduce) {
    .card.scanning::before { display: none; }
    .card.scanning .icon, .scan-state :global(svg), .size.measured { animation: none; }
  }
  .card.done .head { opacity: 0.6; }
  .card.done .hint { color: var(--ok); }
  .card.partly .hint { color: rgb(245 188 95 / 0.9); }
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
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 6px; margin-top: auto; padding-top: var(--fluid-gap-2); border-top: 1px solid rgb(255 255 255 / 0.05); }
  .hint { display: flex; align-items: center; gap: 4px; color: var(--text-2); font-size: var(--fluid-xs); flex-wrap: wrap; }
  .result { display: block; flex-basis: 100%; }
  .result.freed { color: var(--ok); }
  .downloads-details { font-size: var(--fluid-xs); color: var(--text-2); }
  .downloads-details summary { cursor: pointer; }
</style>
