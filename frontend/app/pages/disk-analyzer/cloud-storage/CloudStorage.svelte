<script lang="ts">
  import { onMount } from "svelte";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import Info from "@lucide/svelte/icons/info";
  import { cloudStorage as s } from "./state.svelte";

  const plural = (count: number, one: string, many = `${one}s`) => `${count.toLocaleString()} ${count === 1 ? one : many}`;
  const names = (list: { name: string }[]) => list.map((provider) => provider.name).join(", ");
  // The whole explanation lives in the info icon's tooltip.
  const info = $derived([
    s.ready.length ? `Makes uploaded files online-only in ${names(s.ready)}.` : "No cloud app on this PC allows online-only requests from here.",
    "Files still uploading, or whose sync status can't be verified, stay on this PC. Opening an online-only file again needs an internet connection.",
    ...(s.ready.some((provider) => provider.provider === "googleDrive")
      ? ["Google Drive: Drive's own Offline access › Online only, then its Clear cache. Open Storage on your computer in Drive's settings when asked, and MYLE presses it."]
      : []),
    ...(s.left.length ? [`Not included: ${names(s.left)}, which ${s.left.length === 1 ? "needs its" : "need their"} own storage controls.`] : []),
  ].join("\n\n"));
  const summary = $derived.by(() => {
    const result = s.result;
    if (!result) return "";
    const parts = [`${plural(result.requested, "file")} requested`];
    if (result.foldersRequested) parts.push(`${plural(result.foldersRequested, "Google Drive folder")} set to online-only`);
    if (result.driveCache?.state === "cleared") parts.push(`Google Drive cache cleared${result.driveCache.size ? ` (${result.driveCache.size} before)` : ""}`);
    else if (result.driveCache?.state === "notOpen") parts.push("Google Drive cache not cleared: its storage window wasn't opened");
    if (result.pendingUploads) parts.push(`${plural(result.pendingUploads, "file")} still uploading stay on this PC`);
    if (result.failed) parts.push(`${plural(result.failed, "item")} couldn't be processed`);
    return `${result.cancelled ? "Stopped" : "Done"} · ${parts.join(" · ")}`;
  });
  onMount(() => void s.load());
</script>

<section class="cloud-storage surface" aria-label="Cloud storage">
  <div class="cloud-heading"><span class="cloud-symbol"><CloudDownload size={21} /></span><div><h2>Online-only files</h2><p>Free up space once uploads finish.</p></div></div>
  <span class="info" title={info}><Info size={14} /></span>
  {#if s.running}
    <button type="button" class="btn danger stop-btn" disabled={s.cancelling} onclick={() => s.cancel()}>{s.cancelling ? "Stopping…" : "Stop"}</button>
  {:else}
    <button
      type="button"
      class="btn primary all-btn"
      disabled={s.loading || !!s.loadError || !s.ready.length}
      onclick={() => s.makeOnlineOnly()}
    ><CloudDownload size={13} />Make all online-only</button>
  {/if}
  {#if s.loadError}
    <p class="status error" role="alert">Cloud apps couldn't be checked: {s.loadError} <button type="button" onclick={() => s.load()}>Retry</button></p>
  {:else if s.running && s.progress?.waitingForDrive}
    <p class="status waiting" aria-live="polite">Open <b>Storage on your computer</b> in Google Drive's settings: MYLE presses Clear cache there as soon as it opens.</p>
  {:else if s.running && s.progress}
    <p class="status" aria-live="polite" title={s.progress.current ?? ""}>Checking uploaded files… {s.progress.visited.toLocaleString()} checked · {s.progress.requested.toLocaleString()} requests · {s.progress.pendingUploads.toLocaleString()} waiting for upload</p>
  {:else if s.error}
    <p class="status error" role="alert">{s.error}</p>
  {:else if s.result}
    <p class="status" role="status" title={s.result.errors.join("\n")}>{summary}</p>
  {/if}
</section>

<style>
  .cloud-storage { display: flex; align-items: center; flex-wrap: wrap; gap: 12px 16px; margin-bottom: var(--fluid-gap-3); padding: 15px 18px; border: 1px solid var(--glass-border); border-radius: 14px; background: var(--bg-1); box-shadow: var(--surface-depth); }
  .cloud-heading { display: flex; flex: 1; align-items: center; gap: 12px; min-width: 180px; }
  .cloud-symbol { display: grid; place-items: center; flex: none; width: 42px; height: 42px; border-radius: 12px; background: rgb(var(--accent-rgb) / 0.1); color: rgb(var(--accent-soft-rgb)); }
  .cloud-heading h2 { margin: 0; color: var(--text-1); font-size: 14px; font-weight: 700; }
  .cloud-heading p { margin: 3px 0 0; font-size: 12px; color: var(--text-3); line-height: 1.4; }
  .info { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; border-radius: 50%; color: var(--text-3); cursor: help; }
  .info:hover { color: var(--text-1); background: var(--hover); }
  .cloud-storage .all-btn { display: inline-flex; align-items: center; justify-content: center; min-height: 37px; padding: 0 14px; gap: 7px; font-size: 13px; white-space: nowrap; border-radius: 9px; background: var(--btn-fill); border: 1px solid var(--btn-border); box-shadow: none; color: rgb(var(--accent-soft-rgb)); font-weight: 700; cursor: pointer; transition: background var(--dur-fast), border-color var(--dur-fast); }
  .cloud-storage .all-btn:hover:not(:disabled) { background: var(--btn-fill-hover); border-color: rgb(var(--accent-rgb) / 0.5); }
  .all-btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  .all-btn:disabled { opacity: 0.5; cursor: default; }
  .stop-btn { min-height: 37px; padding: 0 14px; font-size: 13px; border-radius: 9px; }
  .status { flex-basis: 100%; margin: 0; padding: 10px 0 0 54px; border-top: 1px solid var(--panel-rim); overflow: hidden; color: var(--text-2); font-size: 12px; line-height: 1.5; white-space: nowrap; text-overflow: ellipsis; }
  .status.waiting { color: var(--text-1); white-space: normal; }
  .status.error { color: #ffb4b0; white-space: normal; }
  .status button { padding: 2px 5px; text-decoration: underline; color: var(--text-1); }
  @media (max-width: 600px) { .cloud-storage { padding: 13px; gap: 12px; } .cloud-heading { flex-basis: calc(100% - 44px); } .all-btn, .stop-btn { width: 100%; } .status { padding-left: 0; } }
</style>
