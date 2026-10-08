<script lang="ts">
  // The project's releases on GitHub: edit their notes, delete some (and
  // their tags, if asked), or combine several into one set of notes.
  import { onMount, untrack } from "svelte";
  import Combine from "@lucide/svelte/icons/combine";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { confirm } from "../../../lib/confirm.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf, type EntryStatus, type Release } from "./api";
  import { formatRelative, githubReleases as gr, type ListItem } from "./state.svelte";

  let { item, entry }: { item: ListItem; entry: EntryStatus } = $props();

  const repoId = $derived(item.repo.id);
  let releases = $state<Release[] | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let all = $state(untrack(() => !entry.monorepo));
  let picked = $state<number[]>([]);
  let deleteTags = $state(false);
  let working = $state(false);
  let combining = $state(false);

  /** The release being edited (or the combined notes being saved). */
  let editor = $state<{ id: number; title: string; notes: string; prerelease: boolean; combined: number[] | null } | null>(null);

  onMount(() => {
    void load();
  });

  async function load() {
    if (!gr.page?.account) {
      error = "Connect your GitHub account to see the releases.";
      return;
    }
    loading = true;
    try {
      releases = await api.listReleases(repoId);
      error = null;
      picked = picked.filter((id) => releases!.some((r) => r.id === id));
    } catch (e) {
      error = messageOf(e);
    } finally {
      loading = false;
    }
  }

  /** This app's releases: its own tags, and the shared v… tags before them. */
  function mine(release: Release): boolean {
    if (!entry.monorepo) return true;
    if (release.tagName.startsWith(entry.tagPrefix)) return true;
    return entry.lastTag?.legacy === true && /^v\d/.test(release.tagName) && release.tagName.split(".")[0] === entry.lastTag.name.split(".")[0];
  }

  const shown = $derived((releases ?? []).filter((r) => all || mine(r)));
  const pickedReleases = $derived(shown.filter((r) => picked.includes(r.id)));

  function toggle(id: number) {
    picked = picked.includes(id) ? picked.filter((p) => p !== id) : [...picked, id];
  }

  async function remove(ids: number[]) {
    const list = (releases ?? []).filter((r) => ids.includes(r.id));
    if (!list.length) return;
    const names = list.map((r) => r.tagName).join(", ");
    const ok = await confirm({
      title: list.length === 1 ? `Delete the release ${list[0].tagName}?` : `Delete ${list.length} releases?`,
      message: `${names}. ${deleteTags ? "Their tags are deleted too, on GitHub and here." : "Their tags stay."} Downloads of their files stop working. This can't be undone.`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    working = true;
    try {
      const results = await api.deleteReleases(repoId, ids, deleteTags);
      const failed = results.filter((r) => !r.ok || r.error);
      const done = results.filter((r) => r.ok).length;
      if (done) toast.success(`Deleted ${done} ${done === 1 ? "release" : "releases"}.`);
      for (const f of failed) toast.error(`${f.tag || f.releaseId}: ${f.error ?? "not deleted"}`);
      picked = [];
      await load();
      void gr.loadRemote(repoId);
      if (deleteTags) void gr.refresh(repoId);
    } catch (e) {
      toast.error(messageOf(e));
    } finally {
      working = false;
    }
  }

  function edit(release: Release) {
    editor = { id: release.id, title: release.name ?? release.tagName, notes: release.body ?? "", prerelease: release.prerelease, combined: null };
  }

  async function save() {
    if (!editor) return;
    working = true;
    try {
      const updated = await api.updateRelease(repoId, editor.id, editor.title.trim(), editor.notes, editor.prerelease);
      releases = (releases ?? []).map((r) => (r.id === updated.id ? updated : r));
      const combined = editor.combined;
      editor = null;
      toast.success(`Saved ${updated.tagName}.`);
      // The others are deleted only when the user says so.
      const others = (combined ?? []).filter((id) => id !== updated.id);
      if (others.length) {
        picked = others;
        await remove(others);
      }
    } catch (e) {
      toast.error(messageOf(e));
    } finally {
      working = false;
    }
  }

  function combine() {
    const ids = pickedReleases.map((r) => r.id);
    if (ids.length < 2) return;
    // The notes go on the newest one.
    const newest = pickedReleases[0];
    void gr.ai({
      run: (provider) => api.aiCombine(repoId, ids, provider),
      apply: (notes) => {
        editor = { id: newest.id, title: notes.title ?? newest.name ?? newest.tagName, notes: notes.notes, prerelease: newest.prerelease, combined: ids };
      },
      setBusy: (value) => (combining = value),
    });
  }

  const downloads = (r: Release) => r.assets.reduce((sum, a) => sum + a.downloadCount, 0);
</script>

<div class="history">
  <div class="bar">
    {#if entry.monorepo}
      <label class="opt"><input type="checkbox" class="switch" bind:checked={all} /> All releases of {item.repo.name}</label>
    {/if}
    <span class="grow"></span>
    <button class="icon-btn" aria-label="Refresh" title="Refresh" disabled={loading} onclick={() => void load()}>
      <RefreshCw size={14} class={loading ? "spin" : ""} />
    </button>
  </div>

  {#if picked.length}
    <div class="selection">
      <span>{pickedReleases.length} picked</span>
      <label class="opt"><input type="checkbox" class="check" bind:checked={deleteTags} /> Delete their tags too</label>
      <span class="grow"></span>
      <button class="btn small" disabled={pickedReleases.length < 2 || combining || working} onclick={() => (gr.aiReady ? combine() : (gr.settingsOpen = "ai"))} title="One set of notes from the picked releases, saved on the newest">
        {#if combining}<LoaderCircle size={13} class="spin" />{:else}<Combine size={13} />{/if} Combine notes
      </button>
      <button class="btn small danger" disabled={working} onclick={() => void remove(pickedReleases.map((r) => r.id))}><Trash2 size={13} /> Delete</button>
      <button class="icon-btn" aria-label="Clear the selection" onclick={() => (picked = [])}><X size={13} /></button>
    </div>
  {/if}

  {#if editor}
    <div class="editor">
      <div class="editor-head">
        <strong>{editor.combined ? `Combined notes for ${(releases ?? []).find((r) => r.id === editor!.id)?.tagName}` : `Edit ${(releases ?? []).find((r) => r.id === editor!.id)?.tagName}`}</strong>
        <button class="icon-btn" aria-label="Close" onclick={() => (editor = null)}><X size={14} /></button>
      </div>
      <input class="input" bind:value={editor.title} aria-label="Title" />
      <textarea class="input notes" rows="10" bind:value={editor.notes} aria-label="Notes"></textarea>
      <div class="editor-foot">
        <label class="opt"><input type="checkbox" class="check" bind:checked={editor.prerelease} /> Pre-release</label>
        <span class="grow"></span>
        {#if editor.combined}<span class="hint">After saving, you're asked whether to delete the other {editor.combined.length - 1}.</span>{/if}
        <button class="btn small ghost" onclick={() => (editor = null)}>Cancel</button>
        <button class="btn small primary" disabled={working || !editor.title.trim()} onclick={() => void save()}>
          {#if working}<LoaderCircle size={13} class="spin" />{/if} Save on GitHub
        </button>
      </div>
    </div>
  {/if}

  {#if error}
    <div class="quiet">
      <span>{error}</span>
      {#if !gr.page?.account}<button class="btn small" onclick={() => (gr.settingsOpen = "account")}>Connect GitHub</button>{/if}
    </div>
  {:else if !releases}
    <div class="quiet"><LoaderCircle size={16} class="spin" /></div>
  {:else if !shown.length}
    <div class="quiet">No releases yet.</div>
  {:else}
    <ul class="list">
      {#each shown as r (r.id)}
        <li class:picked={picked.includes(r.id)}>
          <input type="checkbox" class="check" checked={picked.includes(r.id)} onchange={() => toggle(r.id)} aria-label="Pick {r.tagName}" />
          <div class="text">
            <strong>
              {r.name || r.tagName}
              {#if r.draft}<span class="badge draft">Draft</span>{/if}
              {#if r.prerelease}<span class="badge pre">Pre-release</span>{/if}
            </strong>
            <small>
              <code>{r.tagName}</code> · {formatRelative(r.publishedAt ?? r.createdAt)} · {r.assets.length} {r.assets.length === 1 ? "file" : "files"}
              {#if r.assets.length}· <Download size={10} /> {downloads(r).toLocaleString()}{/if}
            </small>
          </div>
          <button class="icon-btn" title="Edit" aria-label="Edit {r.tagName}" onclick={() => edit(r)}><Pencil size={13} /></button>
          <button class="icon-btn" title="Open on GitHub" aria-label="Open {r.tagName} on GitHub" onclick={() => void openUrl(r.htmlUrl)}><ExternalLink size={13} /></button>
          <button class="icon-btn danger" title="Delete" aria-label="Delete {r.tagName}" disabled={working} onclick={() => void remove([r.id])}><Trash2 size={13} /></button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .history {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    padding: 12px 16px 16px;
    overflow: auto;
    scrollbar-width: thin;
  }

  .bar,
  .selection,
  .editor-head,
  .editor-foot {
    display: flex;
    align-items: center;
    gap: 9px;
  }

  .grow {
    flex: 1;
  }

  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    font-size: 12.3px;
  }

  .selection {
    padding: 7px 10px;
    border: 1px solid rgb(var(--accent-rgb) / 0.3);
    border-radius: 10px;
    background: rgb(var(--accent-rgb) / 0.08);
    font-size: 12.3px;
  }

  .editor {
    display: grid;
    gap: 8px;
    padding: 12px;
    border: 1px solid rgb(var(--accent-rgb) / 0.3);
    border-radius: 12px;
    background: rgb(255 255 255 / 0.03);
  }

  .editor-head strong {
    flex: 1;
    font-size: 13px;
  }

  .notes {
    min-height: 160px;
    resize: vertical;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
  }

  .hint {
    color: var(--text-3);
    font-size: 11.5px;
  }

  .list {
    display: grid;
    gap: 3px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: 10px;
  }

  .list li:hover {
    background: var(--hover);
  }

  .list li.picked {
    border-color: rgb(var(--accent-rgb) / 0.3);
    background: rgb(var(--accent-rgb) / 0.1);
  }

  .text {
    display: grid;
    flex: 1;
    gap: 2px;
    min-width: 0;
  }

  .text strong {
    overflow: hidden;
    font-size: 13px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text small {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--text-3);
    font-size: 11.3px;
  }

  code {
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .badge {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: 5px;
    font-size: 10px;
    font-weight: 600;
  }

  .draft {
    background: rgb(255 255 255 / 0.1);
    color: var(--text-2);
  }

  .pre {
    background: rgb(255 196 92 / 0.16);
    color: #ffd08a;
  }

  .list .icon-btn {
    width: 28px;
    height: 28px;
  }

  .icon-btn.danger:hover {
    color: #ff9d9d;
  }

  .quiet {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 10px;
    min-height: 160px;
    color: var(--text-3);
    font-size: 12.5px;
  }
</style>
