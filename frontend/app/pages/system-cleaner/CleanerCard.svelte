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
  import { cleanerState, freedAndInUse } from "./state.svelte";

  let { category }: { category: CleanerCategory } = $props();

  const icons: Record<string, typeof HardDrive> = {
    temp: FileClock, prefetch: Zap, "recycle-bin": Recycle, update: RefreshCw, thumbnails: ImageIcon, errors: TriangleAlert, downloads: FolderDown,
  };
  const Icon = $derived(icons[category.icon] ?? HardDrive);
  const measured = $derived(cleanerState.sizes[category.id]);
  const checked = $derived(cleanerState.selected.has(category.id));
  /** Nothing measured yet and a scan is under way: show the sheen, not a number. */
  const pending = $derived(cleanerState.phase === "scanning" && !measured);
  const locked = $derived(measured?.locked ?? false);
  /** Emptied since the last scan: off until the next one measures it again. */
  const done = $derived(cleanerState.isCleaned(category.id));
  const outcome = $derived(cleanerState.outcome[category.id]);
  /** Cleaned, but not everything could go: files in use, or system files
   *  without administrator approval. */
  const partly = $derived(done && (measured?.bytes ?? 0) > 0);
  /** The bytes the run could not remove; an administrator pass already replaced
   *  its part of `skippedBytes` and zeroed `adminSkippedBytes`. */
  const inUse = $derived(done ? Math.max(outcome?.skippedBytes ?? 0, partly ? (measured?.bytes ?? 0) : 0) : 0);
  /** Only when the leftovers are the system folders the user did not approve. */
  const needsAdmin = $derived(partly && category.mayNeedAdmin && cleanerState.adminGranted !== true);
  const skippedFiles = $derived(outcome?.skipped ? ` (${outcome.skipped.toLocaleString()} ${outcome.skipped === 1 ? "file" : "files"})` : "");
</script>

<article class="card" class:on={checked} class:done class:partly class:working={cleanerState.phase === "cleaning" && checked}>
  <div class="head">
    <span class="icon"><Icon size={15} strokeWidth={1.7} /></span>
    <div class="titles">
      <h3 title={category.description}>{category.title}</h3>
      <p title={category.description}>{category.description}</p>
    </div>
    <strong class="size" class:shimmer={pending}>{measured ? formatSize(measured.bytes) : "0 B"}</strong>
    <input type="checkbox" class="switch" aria-label="Include {category.title}" {checked} disabled={cleanerState.busy || done} onchange={() => cleanerState.toggle(category.id)} />
  </div>
  <div class="foot">
    <span class="hint" title={category.hint}>
      {#if partly}
        <TriangleAlert size={11} />
        {freedAndInUse(outcome?.freed ?? 0, inUse)}{needsAdmin ? " · needs administrator" : ""}{skippedFiles}
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
      <button class="btn small" disabled={cleanerState.busy} onclick={() => cleanerState.allowAdmin()}><ShieldCheck size={12} /> Allow admin</button>
    {/if}
  </div>
</article>

<style>
  .card { display: flex; flex-direction: column; gap: 5px; min-width: 0; padding: 8px 10px; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 10px; background: linear-gradient(180deg, rgb(255 255 255 / 0.035), rgb(255 255 255 / 0.012)); transition: border-color var(--dur-fast), background var(--dur-fast); }
  .card:hover { border-color: rgb(255 255 255 / 0.14); background: rgb(255 255 255 / 0.04); }
  .card.on { border-color: rgb(var(--accent-rgb) / 0.32); }
  .card.working { border-color: rgb(var(--accent-rgb) / 0.55); }
  .card.done .head { opacity: 0.6; }
  .card.done .hint { color: var(--ok); }
  .card.partly .hint { color: rgb(245 188 95 / 0.9); }
  .head { display: flex; align-items: center; gap: 8px; }
  .icon { display: grid; place-items: center; width: 26px; height: 26px; flex: none; border-radius: 7px; background: rgb(var(--accent-rgb) / 0.12); color: var(--accent); }
  .titles { flex: 1; min-width: 0; }
  h3, .titles p { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  h3 { font-size: 12.25px; font-weight: 600; line-height: 1.25; }
  .titles p { color: var(--text-3); font-size: 10.5px; line-height: 1.3; }
  .size { flex: none; font-size: 13px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 6px; min-height: 16px; padding-left: 34px; }
  .hint { display: flex; align-items: center; gap: 4px; overflow: hidden; color: var(--text-3); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
</style>
