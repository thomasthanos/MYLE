<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Eye from "@lucide/svelte/icons/eye";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { dialogFocus } from "../game-saves/dialog-focus";
  import { projectBackupsApi as api, type Preview } from "./api";
  import PreviewList from "./PreviewList.svelte";
  import { parseLines, projectBackupsState as pb } from "./state.svelte";

  const editing = pb.editor?.project ?? null;
  const fixMissing = pb.editor?.fixMissing ?? false;
  let name = $state(editing?.name ?? "");
  let appName = $state(editing?.appName ?? "");
  let appNameTouched = $state(!!editing);
  let sourcePath = $state(editing?.sourcePath ?? "");
  let closeApp = $state(editing?.closeApp ?? "");
  let extra = $state((editing?.extraExclusions ?? []).join("\n"));
  let keep = $state((editing?.keep ?? []).join("\n"));
  let preview = $state<Preview | null>(null);
  let previewError = $state<string | null>(null);
  let previewing = $state(false);
  let firstInput = $state<HTMLInputElement>();
  let sourceInput = $state<HTMLInputElement>();

  let confirmDiscard = $state(false);
  let discardBar = $state<HTMLElement>();
  $effect(() => { if (confirmDiscard) discardBar?.scrollIntoView({ block: "nearest", behavior: "smooth" }) });
  const initial = JSON.stringify([editing?.name ?? "", editing?.appName ?? "", editing?.sourcePath ?? "", editing?.closeApp ?? "", (editing?.extraExclusions ?? []).join("\n"), (editing?.keep ?? []).join("\n")]);
  const dirty = $derived(JSON.stringify([name, appName, sourcePath, closeApp, extra, keep]) !== initial);

  onMount(() => {
    (fixMissing ? sourceInput : firstInput)?.focus();
    // A preview still reading a big folder stops with the dialog.
    return () => { if (previewing) void api.cancelPreview() };
  });

  /** Closes the dialog; unsaved changes are confirmed first. */
  function close() {
    if (pb.busy) return;
    if (dirty && !confirmDiscard) { confirmDiscard = true; return }
    pb.closeEditor();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (confirmDiscard) confirmDiscard = false; else close();
  }

  /** A folder's name, made usable as a backup folder name. */
  function suggestAppName(path: string) {
    const last = path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
    return last.replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/, "").trim();
  }

  function setSource(path: string) {
    sourcePath = path;
    preview = null;
    if (!appNameTouched) appName = suggestAppName(path);
    if (!name.trim()) name = suggestAppName(path);
  }

  async function chooseSource() {
    const path = await pb.pickFolder("Choose the project folder");
    if (path) setSource(path);
  }

  async function runPreview() {
    if (!sourcePath.trim()) return;
    previewing = true;
    previewError = null;
    try {
      preview = await api.preview({ sourcePath: sourcePath.trim(), projectId: editing?.id ?? null, extraExclusions: parseLines(extra), keep: parseLines(keep) });
    } catch (error) {
      preview = null;
      const reason = error instanceof Error ? error.message : String(error);
      previewError = reason === "Cancelled." ? null : reason; // a newer preview (or closing) stopped this one
    } finally {
      previewing = false;
    }
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || !appName.trim() || !sourcePath.trim() || pb.busy) return;
    void pb.saveProject({
      id: editing?.id ?? "",
      name: name.trim(),
      appName: appName.trim(),
      sourcePath: sourcePath.trim(),
      closeApp: closeApp.trim() || null,
      extraExclusions: parseLines(extra),
      keep: parseLines(keep),
      lastBackup: editing?.lastBackup ?? null,
      lastResult: editing?.lastResult ?? null,
    });
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div
    class="dialog glass glass--3"
    aria-labelledby="project-editor-title"
    aria-modal="true"
    role="dialog"
    tabindex="-1"
    {@attach dialogFocus}
    transition:scale={{ start: 0.97, duration: 170, easing: cubicOut }}
  >
    <form onsubmit={submit}>
      <header>
        <span>
          <h2 id="project-editor-title">{fixMissing ? "Where is the project now?" : editing ? "Edit project" : "Add a project"}</h2>
          <p>Backups go to <code>{`Projects Backup\\${appName.trim() || "<backup name>"}`}</code> in your cloud folder.</p>
        </span>
        <button class="icon-btn" type="button" aria-label="Close" title="Close (Esc)" disabled={!!pb.busy} onclick={close}><X size={16} /></button>
      </header>

      {#if fixMissing}
        <div class="notice" role="status">
          <CircleAlert size={16} />
          <span>The project folder <code class="selectable">{editing?.sourcePath || "(none)"}</code> was not found. Choose where it is now; the backup carries on once you save.</span>
        </div>
      {/if}

      <div class="field">
        <span>Project folder</span>
        <div class="path-picker">
          <input bind:this={sourceInput} class="input mono" value={sourcePath} disabled={!!pb.busy} placeholder="D:\Projects\MyApp"
            oninput={(event) => setSource(event.currentTarget.value)} required />
          <button type="button" class="btn" disabled={!!pb.busy} onclick={chooseSource}><FolderSearch size={14} /> Choose</button>
        </div>
      </div>

      <div class="row">
        <label class="field">
          <span>Name</span>
          <input bind:this={firstInput} bind:value={name} class="input" disabled={!!pb.busy} maxlength="100" placeholder="My App" required />
        </label>
        <label class="field">
          <span>Backup name <small>folder and file names</small></span>
          <input bind:value={appName} class="input mono" disabled={!!pb.busy} maxlength="100" placeholder="MyApp"
            oninput={() => (appNameTouched = true)} required />
        </label>
      </div>
      <p class="hint">Backups are named <code>{appName.trim() || "MyApp"}_D&lt;day&gt;_V&lt;number&gt;.zip</code>, numbered on from the backups already there (also those made by Backup Projects).</p>

      <label class="field">
        <span>Close this program first <small>optional</small></span>
        <input bind:value={closeApp} class="input mono" disabled={!!pb.busy} maxlength="100" placeholder="MyApp.exe" />
        <small class="hint">For an app that keeps its files open. MYLE never closes itself.</small>
      </label>

      <div class="row">
        <label class="field">
          <span>Also leave out <small>one per line</small></span>
          <textarea bind:value={extra} class="input mono" rows="4" disabled={!!pb.busy} placeholder={"secrets/\n*.psd"} oninput={() => (preview = null)}></textarea>
        </label>
        <label class="field">
          <span>Always back up <small>even if excluded</small></span>
          <textarea bind:value={keep} class="input mono" rows="4" disabled={!!pb.busy} placeholder={"build/\nvendor/keep.zip"} oninput={() => (preview = null)}></textarea>
        </label>
      </div>
      <small class="hint">The global exclusions apply too (node_modules, dist, .git, caches…), and build output of Rust, .NET, Java, Python, C/C++, Flutter, Unity and others is found by itself. Change them under Exclusions. <code>.env</code> files are backed up.</small>

      <div class="preview-block">
        <button type="button" class="btn small" disabled={!sourcePath.trim() || previewing || !!pb.busy} onclick={runPreview}>
          {#if previewing}<LoaderCircle size={13} class="spin" />{:else}<Eye size={13} />{/if} {previewing ? "Reading the folder…" : preview ? "Preview again" : "Preview what is backed up"}
        </button>
        {#if !preview && !previewing && !previewError}
          <small class="hint">See the files that go into the zip, with sizes, and what is left out and why, before the first backup.</small>
        {/if}
        {#if previewError}<p class="error">{previewError}</p>{/if}
        {#if preview}<PreviewList {preview} />{/if}
      </div>

      <footer>
        {#if confirmDiscard}
          <span class="discard" role="alert" bind:this={discardBar}>
            Discard your changes?
            <button type="button" class="btn small" onclick={() => (confirmDiscard = false)}>Keep editing</button>
            <button type="button" class="btn small danger" onclick={() => pb.closeEditor()}>Discard</button>
          </span>
        {/if}
        <button type="button" class="btn" disabled={!!pb.busy} onclick={close}>Cancel</button>
        <button class="btn primary" disabled={!name.trim() || !appName.trim() || !sourcePath.trim() || !!pb.busy}>
          {pb.busy === "save" ? "Saving…" : fixMissing ? "Save and back up" : editing ? "Save changes" : "Add project"}
        </button>
      </footer>
    </form>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 24px; background: var(--scrim); }
  .dialog { display: flex; flex-direction: column; width: min(640px, 100%); max-height: calc(100vh - 48px); border-radius: var(--radius-xl); }
  form { display: grid; gap: 14px; min-height: 0; padding: 20px; overflow: auto; border-radius: inherit; }
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  h2 { font-size: 17px; }
  header p { margin-top: 3px; color: var(--text-3); font-size: 12px; }
  code { font-family: var(--font-mono); font-size: 0.95em; color: var(--text-2); overflow-wrap: anywhere; }
  .notice { display: flex; align-items: flex-start; gap: 8px; padding: 9px 11px; border: 1px solid rgb(245 176 65 / 0.22); border-radius: 9px; color: rgb(245 188 95 / 0.9); font-size: 12px; line-height: 1.45; }
  .notice :global(svg) { flex: none; margin-top: 1px; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .field > span { color: var(--text-2); font-size: 12px; font-weight: 600; }
  .field small { color: var(--text-3); font-size: 10px; font-weight: 400; }
  .row { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  .path-picker { display: flex; align-items: center; gap: 7px; }
  .path-picker input { flex: 1; min-width: 0; }
  .mono { font-family: var(--font-mono); font-size: 11.5px; }
  textarea { resize: vertical; min-height: 74px; line-height: 1.5; }
  .hint { margin-top: -6px; color: var(--text-3); font-size: 11px; line-height: 1.45; }
  .field .hint { margin-top: 0; }
  .preview-block { display: grid; gap: 8px; justify-items: start; }
  .error { color: rgb(255 145 145 / 0.85); font-size: 11.5px; }
  footer { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 8px; }
  .discard { display: inline-flex; align-items: center; gap: 6px; margin-right: auto; color: rgb(245 188 95 / 0.9); font-size: 12px; }
  .danger { border-color: rgb(229 72 77 / 0.35); color: rgb(255 145 145); }
  .preview-block .hint { margin-top: 0; }
  button:disabled { opacity: 0.45; pointer-events: none; }
  @media (max-width: 560px) { .row { grid-template-columns: minmax(0, 1fr); } }
</style>
