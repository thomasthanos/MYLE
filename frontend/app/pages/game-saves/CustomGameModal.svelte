<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { gameSavesState as gs, samePath } from "./state.svelte";
  import { dialogFocus } from "./dialog-focus";

  const editing = gs.editingCustomGame;
  let name = $state(editing?.name ?? "");
  let paths = $state<string[]>(editing ? [...editing.paths] : []);
  let installPath = $state(editing?.installPath ?? "");
  let autoBackup = $state(editing?.autoBackup ?? true);
  let nameInput = $state<HTMLInputElement>();

  onMount(() => nameInput?.focus());

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") gs.closeCustomDialog();
  }

  async function addSavePath() {
    const path = await gs.pickFolder("Choose a game save folder");
    if (path && !paths.some((existing) => samePath(existing, path))) paths = [...paths, path];
  }

  async function chooseInstallPath() {
    const path = await gs.pickFolder("Choose the game's install folder");
    if (path) installPath = path;
  }

  function removePath(path: string) {
    paths = paths.filter((item) => item !== path);
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const clean = name.trim();
    if (!clean || !paths.length || gs.locked) return;
    void gs.saveCustomGame({
      id: editing?.id ?? "",
      name: clean,
      paths,
      installPath: installPath.trim() || null,
      autoBackup,
    });
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div
    class="dialog glass glass--3"
    aria-labelledby="custom-game-title"
    aria-modal="true"
    role="dialog"
    tabindex="-1"
    {@attach dialogFocus}
    transition:scale={{ start: 0.97, duration: 170, easing: cubicOut }}
  >
    <form onsubmit={submit}>
    <header>
      <span>
        <h2 id="custom-game-title">{editing ? "Edit custom game" : "Add a custom game"}</h2>
        <p>Use this for games the database does not detect yet.</p>
      </span>
      <button class="icon-btn" type="button" aria-label="Close" disabled={gs.locked} onclick={() => gs.closeCustomDialog()}><X size={16} /></button>
    </header>

    <label class="field">
      <span>Game name</span>
      <input bind:this={nameInput} bind:value={name} class="input" disabled={gs.locked} maxlength="120" placeholder="Game title" required />
    </label>

    <div class="field">
      <span>Save folders</span>
      <div class="paths">
        {#each paths as path (path)}
          <div class="path">
            <span class="selectable" title={path}>{path}</span>
            <button type="button" class="icon-btn remove" disabled={gs.locked} aria-label={`Remove ${path}`} onclick={() => removePath(path)}>
              <Trash2 size={14} />
            </button>
          </div>
        {:else}
          <p class="empty">Choose at least one folder containing save files.</p>
        {/each}
      </div>
      <button type="button" class="btn add" disabled={gs.locked} onclick={addSavePath}><Plus size={14} /> {gs.settingsBusy === "folderPicker" ? "Choosing folder…" : "Add save folder"}</button>
    </div>

    <div class="field">
      <span>Install folder <small>optional</small></span>
      <div class="path-picker">
        <div class="picked selectable" class:empty={!installPath} title={installPath}>{installPath || "Not selected"}</div>
        <button type="button" class="btn" disabled={gs.locked} onclick={chooseInstallPath}><FolderSearch size={14} /> Choose</button>
        {#if installPath}<button type="button" class="btn ghost" disabled={gs.locked} onclick={() => (installPath = "")}>Clear</button>{/if}
      </div>
    </div>

    <label class="auto surface">
      <span>
        <strong>Automatic backup</strong>
        <small>Include this game in scheduled backups.</small>
      </span>
      <input class="switch" type="checkbox" disabled={gs.locked} bind:checked={autoBackup} />
    </label>

    <footer>
      <button type="button" class="btn" disabled={gs.locked} onclick={() => gs.closeCustomDialog()}>Cancel</button>
      <button class="btn primary" disabled={!name.trim() || !paths.length || gs.locked}>
        {gs.settingsBusy === "customGame" ? "Saving…" : editing ? "Save changes" : "Add game"}
      </button>
    </footer>
    </form>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 24px; background: var(--scrim); }

  /* The form scrolls, not the glass element (see BiosRestartDialog). */
  .dialog { display: flex; flex-direction: column; width: min(570px, 100%); max-height: calc(100vh - 48px); border-radius: var(--radius-xl); }

  form { display: grid; gap: 17px; min-height: 0; padding: 20px; overflow: auto; border-radius: inherit; }

  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }

  h2 { font-size: 17px; }

  header p { margin-top: 3px; color: var(--text-3); font-size: 12px; }

  .field { display: grid; gap: 7px; }

  .field > span { color: var(--text-2); font-size: 12px; font-weight: 600; }

  .field small { color: var(--text-3); font-size: 10px; font-weight: 400; }

  .paths { display: grid; gap: 5px; max-height: 150px; overflow: auto; }

  .path { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 7px; padding: 5px 6px 5px 9px; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 8px; background: rgb(0 0 0 / 0.12); }

  .path > span,
  .picked { overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }

  .remove { color: rgb(255 145 145 / 0.68); }

  .empty { color: var(--text-3); }

  p.empty { padding: 9px 10px; border: 1px dashed rgb(255 255 255 / 0.07); border-radius: 8px; font-size: 11.5px; }

  .add { justify-self: start; }

  .path-picker { display: flex; align-items: center; gap: 7px; }

  .picked { flex: 1; min-width: 0; padding: 8px 10px; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 9px; background: rgb(0 0 0 / 0.14); }

  .auto { display: flex; align-items: center; justify-content: space-between; gap: 20px; padding: 11px 12px; }

  .auto > span { display: grid; gap: 2px; }

  .auto strong { font-size: 12.5px; }

  .auto small { color: var(--text-3); font-size: 11px; }

  footer { display: flex; justify-content: flex-end; gap: 8px; }

  button:disabled { opacity: 0.45; pointer-events: none; }
</style>
