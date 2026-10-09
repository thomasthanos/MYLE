<script lang="ts">
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import FolderDown from "@lucide/svelte/icons/folder-down";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { confirm } from "../../../lib/confirm.svelte";
  import { readFlag, writeFlag } from "../../../lib/storage";
  import { toast } from "../../../lib/toast.svelte";
  import { formatSize } from "./api";

  type Item = { path: string; name: string; isDir: boolean; size: number; changed: number };
  type Preview = { folder: string; items: Item[]; total: number; kept: number };
  type Outcome = { deleted: number; freed: number; skipped: string[] };

  const KEY = "myle.cleaner.downloads";
  /** Off until the user turns it on. */
  let enabled = $state(readFlag(KEY, false));
  let busy = $state(false);
  let preview = $state<Preview | null>(null);
  /** Unticked in the list: kept this time. */
  let skip = $state<Record<string, boolean>>({});

  const chosen = $derived(preview ? preview.items.filter((item) => !skip[item.path]) : []);
  const chosenSize = $derived(chosen.reduce((sum, item) => sum + item.size, 0));

  function setEnabled(value: boolean) {
    enabled = value;
    writeFlag(KEY, value);
    if (!value) preview = null;
  }

  const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

  async function scan() {
    if (!isTauri()) return toast.error("The Downloads cleaner runs only in the app.");
    busy = true;
    try {
      preview = await invoke<Preview>("cleaner_downloads_preview");
      skip = {};
    } catch (error) {
      toast.error(message(error));
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!preview || !chosen.length) return;
    const ok = await confirm({
      title: `Permanently delete ${chosen.length} item${chosen.length === 1 ? "" : "s"}?`,
      message:
        `From ${preview.folder}, ${formatSize(chosenSize)}.\n\n` +
        "They are deleted permanently: not moved to the Recycle Bin, and they cannot be restored.\n\n" +
        "Documents and anything from the last 7 days are never touched.",
      confirmLabel: "Delete permanently",
      danger: true,
    });
    if (!ok) return;
    busy = true;
    try {
      const outcome = await invoke<Outcome>("cleaner_downloads_delete", { paths: chosen.map((item) => item.path) });
      const left = outcome.skipped.length;
      toast.success(
        `Deleted ${outcome.deleted} item${outcome.deleted === 1 ? "" : "s"}, ${formatSize(outcome.freed)} freed.` +
          (left ? ` ${left} in use or changed: left in place.` : ""),
      );
      await scan();
    } catch (error) {
      toast.error(message(error));
    } finally {
      busy = false;
    }
  }

  const when = (seconds: number) => new Date(seconds * 1000).toLocaleDateString();
</script>

<section class="downloads surface">
  <header>
    <span class="icon"><FolderDown size={16} /></span>
    <span class="text">
      <strong>Downloads folder</strong>
      <small>Old installers, archives and other files in Downloads. Documents (PDF, Word, Excel, PowerPoint, text, e-books…) and anything from the last 7 days stay.</small>
    </span>
    <input type="checkbox" class="switch" aria-label="Clean the Downloads folder" checked={enabled} onchange={(e) => setEnabled(e.currentTarget.checked)} />
  </header>

  {#if enabled}
    <div class="actions">
      <button class="btn small" disabled={busy} onclick={scan}>
        {#if busy}<LoaderCircle size={13} class="spin" />{/if} {preview ? "Scan again" : "Show what would go"}
      </button>
      {#if preview}
        <span class="sum">{chosen.length} of {preview.items.length} · {formatSize(chosenSize)} · {preview.kept} kept</span>
        <button class="btn small danger" disabled={busy || !chosen.length} onclick={remove}>
          <Trash2 size={13} /> Delete permanently
        </button>
      {/if}
    </div>
    {#if preview}
      {#if preview.items.length}
        <ul class="list">
          {#each preview.items as item (item.path)}
            <li>
              <label>
                <input type="checkbox" checked={!skip[item.path]} onchange={(e) => (skip[item.path] = !e.currentTarget.checked)} />
                <span class="name" title={item.path}>{item.name}{item.isDir ? "\\" : ""}</span>
                <span class="date">{when(item.changed)}</span>
                <span class="size">{formatSize(item.size)}</span>
              </label>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="none">Nothing to delete in {preview.folder}.</p>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .downloads {
    display: grid;
    gap: 10px;
    margin-top: 12px;
    padding: 14px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 9px;
    background: rgb(var(--accent-rgb) / 0.14);
    color: rgb(var(--accent-rgb));
  }

  .text {
    display: grid;
    flex: 1;
    gap: 2px;
  }

  .text strong {
    font-size: 13px;
  }

  .text small,
  .sum,
  .none,
  .date {
    color: var(--text-3);
    font-size: 11.5px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .sum {
    margin-right: auto;
  }

  .list {
    max-height: 260px;
    margin: 0;
    padding: 4px;
    overflow: auto;
    border: 1px solid var(--btn-border);
    border-radius: 10px;
    list-style: none;
  }

  .list label {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto 76px;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
    border-radius: 6px;
    font-size: 12px;
    cursor: pointer;
  }

  .list label:hover {
    background: var(--hover);
  }

  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .size {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
