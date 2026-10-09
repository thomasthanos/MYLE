<script lang="ts" module>
  import type { ImportPreview } from "./api";

  export type DialogKind = { kind: "import"; preview: ImportPreview } | { kind: "export" } | { kind: "master" };
</script>

<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { toast } from "../../../lib/toast.svelte";
  import { passwordsApi as api } from "./api";
  import { afterImport, passwords as p } from "./state.svelte";

  let { dialog, onclose }: { dialog: DialogKind; onclose: () => void } = $props();

  let first = $state("");
  let second = $state("");
  /** The new master password again: one typo would lock the vault. */
  let again = $state("");
  /** The master password again, before every password leaves in a backup. */
  let master = $state("");
  let working = $state(false);
  let error = $state<string | null>(null);

  const titles = {
    import: "Import passwords",
    export: "Export an encrypted backup",
    master: "Change master password",
  };

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (working) return;
    working = true;
    error = null;
    try {
      if (dialog.kind === "import") {
        const count = await api.importFile(dialog.preview.path, dialog.preview.backup ? first : undefined);
        await p.refresh();
        afterImport();
        if (count) toast.success(`Imported ${count} ${count === 1 ? "entry" : "entries"}.`);
        else toast.info("Everything in the file is already in your vault.");
        onclose();
      } else if (dialog.kind === "export") {
        if (first !== second) throw new Error("The two passwords are not the same.");
        const path = await api.exportBackup(first, master);
        if (path) {
          toast.success("Backup saved. Keep its password: it is not your master password.");
          onclose();
        }
      } else {
        if (second.length < 10) throw new Error("Use at least 10 characters for the new master password.");
        if (second !== again) throw new Error("The two new passwords are not the same.");
        if (await p.changeMaster(first, second)) onclose();
        else error = p.error;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      working = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !working && onclose()} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div
    class="dialog glass glass--3"
    role="dialog"
    aria-modal="true"
    aria-labelledby="vault-dialog-title"
    transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}
  >
  <form onsubmit={submit}>
    <header>
      <h2 id="vault-dialog-title">{titles[dialog.kind]}</h2>
      <button type="button" class="icon-btn" aria-label="Close" onclick={onclose}><X size={16} /></button>
    </header>

    {#if dialog.kind === "import"}
      <p class="path selectable">{dialog.preview.path}</p>
      {#if dialog.preview.backup}
        <p>This is a MYLE backup. Enter the password it was saved with.</p>
        <input class="input" type="password" bind:value={first} placeholder="Backup password" />
      {:else}
        <p>
          <strong>{dialog.preview.count}</strong> {dialog.preview.count === 1 ? "login" : "logins"} found. They are added next
          to what you have; nothing is replaced.
        </p>
        {#if dialog.preview.sample.length}
          <ul class="sample">
            {#each dialog.preview.sample as [title, user], i (i)}<li><span>{title}</span><small>{user}</small></li>{/each}
          </ul>
        {/if}
        <p class="hint">Delete the exported file afterwards: it holds your passwords in plain text.</p>
      {/if}
    {:else if dialog.kind === "export"}
      <p>Every entry, encrypted with a password you choose now. Import it here on any PC to get them back.</p>
      <input class="input" type="password" autocomplete="current-password" bind:value={master} placeholder="Your master password" />
      <input class="input" type="password" autocomplete="new-password" bind:value={first} placeholder="Backup password (10+ characters)" />
      <input class="input" type="password" autocomplete="new-password" bind:value={second} placeholder="Type it again" />
    {:else}
      <p>Your entries stay as they are; only the key that opens them is re-sealed.</p>
      <input class="input" type="password" autocomplete="current-password" bind:value={first} placeholder="Current master password" />
      <input class="input" type="password" autocomplete="new-password" bind:value={second} placeholder="New master password (10+ characters)" />
      <input class="input" type="password" autocomplete="new-password" bind:value={again} placeholder="Type the new one again" />
    {/if}

    {#if error}<p class="error">{error}</p>{/if}

    <footer>
      <button type="button" class="btn ghost" onclick={onclose} disabled={working}>Cancel</button>
      <button type="submit" class="btn primary" disabled={working || (dialog.kind !== "import" && (!first || !second)) || (dialog.kind === "export" && !master) || (dialog.kind === "master" && !again) || (dialog.kind === "import" && dialog.preview.backup && !first)}>
        {#if working}<LoaderCircle size={14} class="spin" />{/if}
        {dialog.kind === "import" ? "Import" : dialog.kind === "export" ? "Save backup" : "Change"}
      </button>
    </footer>
  </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 91;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--scrim);
  }

  .dialog {
    width: min(440px, 100%);
    border-radius: var(--radius-xl);
  }

  form {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
    padding: 20px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    font-size: 17px;
  }

  p {
    color: var(--text-2);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .path {
    min-width: 0;
    overflow: hidden;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sample {
    display: grid;
    gap: 3px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .sample li {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    padding: 6px 10px;
    border-radius: 8px;
    background: rgb(0 0 0 / 0.18);
    font-size: 12px;
  }

  .sample small {
    color: var(--text-3);
  }

  .hint {
    color: #ffd08a;
    font-size: 11.5px;
  }

  .error {
    color: #ff9d9d;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
