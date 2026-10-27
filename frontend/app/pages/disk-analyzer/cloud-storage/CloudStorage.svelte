<script lang="ts">
  import { onMount } from "svelte";
  import Cloud from "@lucide/svelte/icons/cloud";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import X from "@lucide/svelte/icons/x";
  import { cloudStorage as s } from "./state.svelte";
  import type { CloudProvider } from "./api";

  let dialog: HTMLDialogElement;
  const instructions: Record<CloudProvider, string[]> = {
    googleDrive: [
      "In Google Drive Preferences, use Stream files for the folders you want to keep online.",
      "Wait for Drive to finish uploading. Open the Drive folder, select the files or folders inside it, and use the client's offline availability controls to keep them online.",
      "Google manages its cache automatically. Turning off offline availability does not necessarily release all cached space immediately.",
    ],
    mega: [
      "MEGA Desktop sync does not convert existing local files into online-only placeholders. Deleting a synced local file can also delete its cloud copy.",
      "The separate MEGAcmd app offers an on-demand FUSE mount on Windows (beta), with its own setup and managed cache.",
      "A MEGAcmd mount does not convert or reclaim space from an existing MEGAsync sync folder. Follow MEGA's mounting guide before changing your sync setup.",
    ],
    dropbox: [
      "Open the Dropbox folder and wait for Dropbox to show that syncing is up to date.",
      "Right-click the files or folders you want to keep in the cloud and choose Make online-only.",
      "If the option is missing, check Dropbox's online-only support and desktop app settings.",
    ],
    oneDrive: [
      "Start OneDrive and enable Files On-Demand for this account.",
      "Wait for your uploads to finish. In File Explorer, right-click the required files or folders and choose Free up space.",
      "Your files stay in OneDrive and download again when you open them.",
    ],
  };
  const logo: Partial<Record<CloudProvider, string>> = {
    googleDrive: "/icons/installapps/Google-Drive.svg",
    dropbox: "/icons/installapps/Dropbox.svg",
  };
  onMount(() => void s.load());
  $effect(() => {
    if (!dialog) return;
    if (s.selected && !dialog.open) dialog.showModal();
    else if (!s.selected && dialog.open) dialog.close();
  });
</script>

<section class="cloud-storage surface" aria-label="Cloud storage">
  <div class="cloud-heading"><CloudDownload size={19} /><div><h2>Online-only files</h2><p>Manage local copies after uploads finish.</p></div></div>
  <div class="providers">
    {#each s.providers as provider (provider.provider)}
      <button
        type="button"
        class="provider-btn"
        class:unavailable={provider.mode === "unavailable"}
        disabled={s.loading || s.running}
        title={`${provider.name}: ${provider.detail}`}
        onclick={() => s.choose(provider.provider)}
      >
        {#if logo[provider.provider]}<img src={logo[provider.provider]} alt="" width="19" height="19" />{:else}<Cloud size={19} />{/if}
        <span>{provider.name}</span>
        <span class="status-dot" class:ready={provider.mode === "native"} aria-hidden="true"></span>
      </button>
    {/each}
  </div>
  <button type="button" class="refresh-btn" disabled={s.loading || s.running} aria-label="Refresh cloud apps" title="Refresh cloud apps" onclick={() => s.load()}><RefreshCw size={16} class={s.loading ? "refreshing" : ""} /></button>
  {#if s.loadError}<p class="load-error" role="alert">Cloud apps couldn't be checked: {s.loadError} <button type="button" onclick={() => s.load()}>Retry</button></p>{/if}
  {#if s.running}<p class="active-request" aria-live="polite">Checking {s.current?.name ?? "cloud"} files… <button type="button" onclick={() => { if (s.selected && !dialog.open) dialog.showModal(); }}>Show progress</button></p>{/if}
</section>

<dialog
  bind:this={dialog}
  class="cloud-dialog"
  aria-labelledby="cloud-storage-title"
  aria-modal={s.selected ? "true" : undefined}
  oncancel={(event) => { if (s.running) event.preventDefault(); else s.close(); }}
  onclose={() => s.close()}
>
  {#if s.current}
    <header class="dialog-header">
      <div><CloudDownload size={22} /><h2 id="cloud-storage-title">{s.current.name}</h2></div>
      <button type="button" class="close-btn" disabled={s.running} aria-label="Close cloud storage" onclick={() => s.close()}><X size={18} /></button>
    </header>
    <div class="dialog-content">
      <p class="provider-detail">{s.current.detail}</p>
      {#if s.current.mode === "native"}
        <h3>Make uploaded files online-only</h3>
        <p>Request online-only copies in all the cloud folders below. Files still uploading or whose sync status can't be verified stay on this PC. Opening online-only files again needs an internet connection.</p>
      {:else}
        <h3>{s.current.provider === "mega" ? "MEGA storage options" : "Manage offline availability"}</h3>
        <ol class="instructions">{#each instructions[s.current.provider] as instruction}<li>{instruction}</li>{/each}</ol>
      {/if}
      {#if s.current.folders.length}
        <ul class="folder-list" aria-label="Cloud folders">
          {#each s.current.folders as folder (folder.path)}
            <li><div><span>{folder.label}</span><strong title={folder.path}>{folder.path}</strong></div><button type="button" class="btn small" disabled={s.running} onclick={() => s.openFolder(folder.path)}><FolderOpen size={14} />Open</button></li>
          {/each}
        </ul>
      {/if}
      {#if s.running && s.progress}
        <div class="request-progress" role="status">
          <strong>{s.cancelling ? "Stopping…" : "Checking uploaded files…"}</strong>
          <span>{s.progress.visited.toLocaleString()} checked · {s.progress.requested.toLocaleString()} requests · {s.progress.pendingUploads.toLocaleString()} waiting for upload</span>
          {#if s.progress.current}<small title={s.progress.current}>{s.progress.current}</small>{/if}
        </div>
      {/if}
      {#if s.result}
        <div class="request-result" role="status">
          <h3>{s.result.cancelled ? "Request stopped" : s.result.requested ? "Online-only requests sent" : "No online-only requests sent"}</h3>
          <p>{s.result.requested.toLocaleString()} files requested. Your cloud app completes the change; this count does not mean that space has already been released.</p>
          <dl>
            <div><dt>Already online-only</dt><dd>{s.result.alreadyOnline.toLocaleString()}</dd></div>
            <div><dt>Waiting for upload</dt><dd>{s.result.pendingUploads.toLocaleString()}</dd></div>
            <div><dt>Left unchanged</dt><dd>{s.result.skipped.toLocaleString()}</dd></div>
            <div><dt>Couldn't process</dt><dd>{s.result.failed.toLocaleString()}</dd></div>
          </dl>
          {#if s.result.pendingUploads}<p>Pending files stay local. Wait for uploads to finish, then retry.</p>{/if}
          {#if s.result.errors.length}<details><summary>Show details</summary><ul>{#each s.result.errors as error}<li>{error}</li>{/each}</ul></details>{/if}
        </div>
      {/if}
      {#if s.error}<p class="action-error" role="alert">{s.error}</p>{/if}
      {#if s.loadError}<p class="action-error" role="alert">Refresh the cloud folders before requesting a change: {s.loadError}</p>{/if}
    </div>
    <footer class="dialog-actions">
      <button type="button" class="btn" onclick={() => s.help()}><ExternalLink size={15} />{s.current.provider === "mega" ? "MEGAcmd mounting guide" : "Official instructions"}</button>
      {#if s.running}
        <button type="button" class="btn danger" disabled={s.cancelling} onclick={() => s.cancel()}>{s.cancelling ? "Stopping…" : "Stop request"}</button>
      {:else if s.current.mode === "native"}
        <button type="button" class="btn primary" disabled={s.loading || !!s.loadError} onclick={() => s.makeOnlineOnly()}><CloudDownload size={16} />{s.result ? "Retry online only" : "Make uploaded files online-only"}</button>
      {:else}
        <button type="button" class="btn primary" onclick={() => s.close()}>Done</button>
      {/if}
    </footer>
  {/if}
</dialog>

<style>
  .cloud-storage { display: flex; align-items: center; flex-wrap: wrap; gap: 12px 18px; margin-bottom: var(--fluid-gap-3); padding: 13px 17px; border-radius: 13px; }
  .cloud-heading { display: flex; align-items: center; gap: 10px; color: rgb(var(--accent-soft-rgb)); }
  .cloud-heading h2 { margin: 0; color: var(--text-1); font-size: 13px; }
  .cloud-heading p { margin: 3px 0 0; font-size: 11px; color: var(--text-3); }
  .providers { display: flex; flex: 1; flex-wrap: wrap; gap: 8px; justify-content: flex-end; }
  .provider-btn { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-height: 35px; padding: 0 12px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--chip-fill); color: var(--text-1); font-size: 12px; font-weight: 600; cursor: pointer; }
  .provider-btn:hover:not(:disabled) { background: var(--chip-fill-hover); border-color: rgb(var(--accent-rgb) / 0.45); }
  .provider-btn.unavailable { color: var(--text-2); }
  .provider-btn:disabled { opacity: 0.55; cursor: default; }
  .status-dot { width: 5px; height: 5px; border-radius: 50%; background: var(--text-3); opacity: 0.6; }
  .status-dot.ready { background: #69bf97; opacity: 1; }
  .refresh-btn, .close-btn { display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; border: 0; border-radius: 7px; color: var(--text-2); background: transparent; cursor: pointer; }
  .refresh-btn:hover:not(:disabled), .close-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  .load-error, .active-request { flex-basis: 100%; margin: 0; color: var(--text-2); font-size: 12px; }
  .load-error { color: #ffb4b0; }
  .load-error button, .active-request button { padding: 2px 5px; text-decoration: underline; color: var(--text-1); }
  .cloud-dialog { width: min(630px, calc(100vw - 32px)); max-height: min(740px, calc(100vh - 40px)); margin: auto; padding: 0; border: 1px solid var(--btn-border); border-radius: 17px; background: var(--float-bg); color: var(--text-1); box-shadow: 0 25px 90px #0008; }
  .cloud-dialog[open] { display: flex; flex-direction: column; }
  .cloud-dialog::backdrop { background: #0009; }
  .dialog-header, .dialog-actions { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px; padding: 17px 20px; flex-shrink: 0; }
  .dialog-header { border-bottom: 1px solid var(--btn-border); }
  .dialog-header > div { display: flex; align-items: center; gap: 10px; color: rgb(var(--accent-soft-rgb)); }
  .dialog-header h2 { margin: 0; color: var(--text-1); font-size: 19px; }
  .dialog-content { overflow-y: auto; padding: 17px 20px; font-size: 13px; line-height: 1.6; }
  .dialog-content h3 { margin: 14px 0 7px; font-size: 14px; }
  .dialog-content p { margin: 0 0 12px; color: var(--text-2); }
  .provider-detail { color: var(--text-3); font-size: 12px; }
  .instructions { margin: 0; padding-left: 20px; color: var(--text-2); }
  .instructions li + li { margin-top: 9px; }
  .folder-list { list-style: none; padding: 0; margin: 18px 0; }
  .folder-list li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 12px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--input-fill); }
  .folder-list li + li { margin-top: 7px; }
  .folder-list li > div { display: flex; flex-direction: column; min-width: 0; }
  .folder-list span { color: var(--text-3); font-size: 11px; }
  .folder-list strong { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: 12px; font-weight: 600; }
  .request-progress, .request-result { display: flex; flex-direction: column; gap: 5px; padding: 12px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--input-fill); }
  .request-progress span, .request-progress small { color: var(--text-2); font-size: 11px; }
  .request-progress small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .request-result h3 { margin: 0; }
  .request-result p { margin: 5px 0; font-size: 12px; }
  .request-result dl { margin: 7px 0; font-size: 12px; }
  .request-result dl > div { display: flex; justify-content: space-between; gap: 10px; }
  .request-result dt { color: var(--text-2); }
  .request-result dd { margin: 0; font-variant-numeric: tabular-nums; }
  .request-result details { font-size: 11px; }
  .request-result details li { overflow-wrap: anywhere; }
  .dialog-content .action-error { padding: 10px 12px; margin-top: 14px; border: 1px solid rgb(229 72 77 / 0.4); border-radius: 8px; color: #ffb4b0; }
  .dialog-actions { border-top: 1px solid var(--btn-border); }
  .dialog-actions .btn { font-size: 12px; }
  :global(.refreshing) { animation: cloud-refresh 1s linear infinite; }
  @keyframes cloud-refresh { to { transform: rotate(360deg); } }
  @media (max-width: 720px) { .providers { flex-basis: 100%; justify-content: flex-start; } .provider-btn { flex: 1 1 130px; } .refresh-btn { margin-left: auto; } .dialog-actions .btn { flex: 1; } }
  @media (prefers-reduced-motion: reduce) { :global(.refreshing) { animation: none; } }
</style>
