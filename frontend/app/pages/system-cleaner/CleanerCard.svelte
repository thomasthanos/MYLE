<script lang="ts">
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import FileClock from "@lucide/svelte/icons/file-clock";
  import FolderDown from "@lucide/svelte/icons/folder-down";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import ImageIcon from "@lucide/svelte/icons/image";
  import Lock from "@lucide/svelte/icons/lock";
  import Recycle from "@lucide/svelte/icons/recycle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Zap from "@lucide/svelte/icons/zap";
  import { formatSize, type CleanerCategory } from "./api";
  import { cleanerState } from "./state.svelte";
  import DownloadsCard from "./DownloadsCard.svelte";
  import { health } from "./health.svelte";

  let { category }: { category: CleanerCategory } = $props();

  const icons: Record<string, typeof HardDrive> = {
    temp: FileClock, prefetch: Zap, "recycle-bin": Recycle, update: RefreshCw, thumbnails: ImageIcon, errors: TriangleAlert, downloads: FolderDown,
  };
  const Icon = $derived(icons[category.icon] ?? HardDrive);
  const measured = $derived(cleanerState.sizes[category.id]);
  const checked = $derived(cleanerState.isTicked(category.id));
  /** Nothing measured yet and a scan is under way: show the sheen, not a number. */
  const pending = $derived(cleanerState.phase === "scanning" && !measured);
  const locked = $derived(measured?.locked ?? false);
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
  const needsAdmin = $derived(partly && category.mayNeedAdmin && cleanerState.adminGranted !== true);
  /** Whether the tick can change now: never before the first scan, while anything runs, or once cleaned. */
  const canTick = $derived(cleanerState.scanned && !cleanerState.busy && !health.busy && !health.scanning && !done);
  const skippedFiles = $derived(outcome?.skipped ? ` (${outcome.skipped.toLocaleString()} ${outcome.skipped === 1 ? "file" : "files"})` : "");
  /** Part of it is system folders that only an administrator can empty, and that approval has not been given (yet): drawn fainter. */
  const adminOnly = $derived(category.mayNeedAdmin && cleanerState.adminGranted !== true);
  const adminTip = $derived(
    cleanerState.adminGranted === false
      ? "Needs administrator approval, which was declined. Only your own files here are cleaned."
      : "Needs administrator approval. Windows asks for it when you scan.",
  );
</script>

<article class="card" class:on={checked} class:done class:partly class:admin={adminOnly} class:working={cleanerState.phase === "cleaning" && checked}>
  <!-- The whole head ticks the box: a big target, and the cursor shows when it cannot. -->
  <label class="head" class:blocked={!canTick} data-tip={cleanerState.scanned ? undefined : "Scan first: it measures every category and ticks them for you"}>
    <span class="icon"><Icon size={18} strokeWidth={1.7} /></span>
    <span class="titles">
      <span class="name">{category.title}{#if category.mayNeedAdmin}<span class="admin-tag" title={adminOnly ? adminTip : "Administrator approval was given for this scan."}><ShieldCheck size={10} /> Admin</span>{/if}</span>
      <span class="desc">{category.description}</span>
    </span>
    <strong class="size" class:shimmer={pending}>{measured ? formatSize(measured.bytes) : "0 B"}</strong>
    <input class="check" type="checkbox" aria-label="Include {category.title}" {checked} disabled={!canTick} onchange={() => cleanerState.toggle(category.id)} />
  </label>
  <div class="foot">
    <span class="hint" title={category.hint}>
      {#if partly}
        <TriangleAlert size={11} />
        <span class="result freed">Freed {formatSize(outcome?.freed ?? 0)}</span>
        {#if inUse}<span class="result">In use: {formatSize(inUse)}{skippedFiles} · close the app and scan again</span>{/if}
        {#if needsAdmin}<span class="result">Administrator needed{adminBytes ? `: ${formatSize(adminBytes)}` : ""}</span>{/if}
      {:else if done}
        <CheckCheck size={11} />
        {outcome?.freed ? `Freed ${formatSize(outcome.freed)}` : "Cleaned"} · scan again to re-check
      {:else if locked}
        <Lock size={11} /> System files skipped (no administrator)
      {:else if measured?.files}
        {measured.files.toLocaleString()} files · {category.hint}
      {:else}
        {category.hint}
      {/if}
    </span>
    {#if locked && !done && cleanerState.adminGranted === false}
      <button class="btn small" disabled={cleanerState.busy || health.busy || health.scanning} onclick={() => cleanerState.allowAdmin()}><ShieldCheck size={12} /> Allow admin</button>
    {/if}
  </div>
  {#if category.id === "downloads" && cleanerState.downloads}
    <details class="downloads-details"><summary>Review Downloads files</summary><DownloadsCard embedded /></details>
  {/if}
</article>

<style>
  .card { display: flex; flex-direction: column; gap: 10px; min-width: 0; padding: 16px; border: 1px solid var(--glass-border); border-radius: 12px; background: var(--surface-fill); box-shadow: var(--surface-depth); transition: border-color var(--dur-fast); }
  .card:hover { border-color: rgb(255 255 255 / 0.14); background: rgb(255 255 255 / 0.04); }
  .card.on { border-color: rgb(var(--accent-rgb) / 0.32); }
  .card.working { border-color: rgb(var(--accent-rgb) / 0.55); }
  .card.done .head { opacity: 0.6; }
  .card.done .hint { color: var(--ok); }
  .card.partly .hint { color: rgb(245 188 95 / 0.9); }
  /* System folders without administrator approval: fainter until hovered, so the cards that clean fully lead. */
  .card.admin { opacity: 0.58; transition: border-color var(--dur-fast), opacity var(--dur-med) var(--ease-out); }
  .card.admin:hover, .card.admin:focus-within { opacity: 0.92; }
  .admin-tag { display: inline-flex; align-items: center; gap: 3px; margin-left: 7px; padding: 1px 6px; border-radius: 999px; background: color-mix(in srgb, var(--warn) 13%, transparent); color: var(--warn); font-size: 10px; font-weight: 600; line-height: 1.4; vertical-align: 1px; white-space: nowrap; }
  .head { display: flex; align-items: flex-start; gap: 10px; cursor: pointer; }
  .head.blocked { cursor: not-allowed; }
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
