<script lang="ts">
  // The project's releases on GitHub: edit their notes, delete some (and
  // their tags, if asked), or combine several into one set of notes; and the
  // tags no release uses, to delete on their own.
  import { onMount, untrack } from "svelte";
  import Combine from "@lucide/svelte/icons/combine";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Tag from "@lucide/svelte/icons/tag";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { confirm } from "../../../lib/confirm.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf, type Deleted, type EntryStatus, type LoneTag, type Release } from "./api";
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
  let view = $state<"releases" | "tags">("releases");
  let tags = $state<LoneTag[] | null>(null);
  let tagsError = $state<string | null>(null);
  let pickedTags = $state<string[]>([]);

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
    const lone = loadTags();
    try {
      releases = await api.listReleases(repoId);
      error = null;
      picked = picked.filter((id) => releases!.some((r) => r.id === id));
    } catch (e) {
      error = messageOf(e);
    } finally {
      await lone;
      loading = false;
    }
  }

  async function loadTags() {
    try {
      tags = await api.loneTags(repoId);
      tagsError = null;
      pickedTags = pickedTags.filter((name) => tags!.some((t) => t.name === name));
    } catch (e) {
      tagsError = messageOf(e);
    }
  }

  /** This app's releases: its own tags, and the shared v… tags before them. */
  function mine(release: Release): boolean {
    if (!entry.monorepo) return true;
    if (release.tagName.startsWith(entry.tagPrefix)) return true;
    return entry.lastTag?.legacy === true && /^v\d/.test(release.tagName) && release.tagName.split(".")[0] === entry.lastTag.name.split(".")[0];
  }

  /** The same for a tag. */
  function mineTag(name: string): boolean {
    if (!entry.monorepo || name.startsWith(entry.tagPrefix)) return true;
    return entry.lastTag?.legacy === true && /^v\d/.test(name) && name.split(".")[0] === entry.lastTag.name.split(".")[0];
  }

  const shown = $derived((releases ?? []).filter((r) => all || mine(r)));
  const shownTags = $derived((tags ?? []).filter((t) => all || mineTag(t.name)));
  const pickedLone = $derived(shownTags.filter((t) => pickedTags.includes(t.name)));
  const allTagsPicked = $derived(shownTags.length > 0 && pickedLone.length === shownTags.length);

  function toggleTag(name: string) {
    pickedTags = pickedTags.includes(name) ? pickedTags.filter((n) => n !== name) : [...pickedTags, name];
  }

  function where(tag: LoneTag): string {
    return tag.local && tag.remote ? "on GitHub and this PC" : tag.remote ? "only on GitHub" : "only on this PC";
  }

  async function removeTags(names: string[]) {
    const list = shownTags.filter((t) => names.includes(t.name));
    if (!list.length) return;
    const listed = list.slice(0, 15).map((t) => t.name).join(", ") + (list.length > 15 ? ` and ${list.length - 15} more` : "");
    const ok = await confirm({
      title: list.length === 1 ? `Delete the tag ${list[0].name}?` : `Delete ${list.length} tags?`,
      message: `${listed}. No release uses ${list.length === 1 ? "it" : "them"}; ${list.length === 1 ? "it is" : "they are"} deleted on GitHub and on this PC. Links to ${list.length === 1 ? "it" : "them"} stop working. This can't be undone.`,
      confirmLabel: list.length === 1 ? "Delete tag" : `Delete ${list.length} tags`,
      danger: true,
    });
    if (!ok) return;
    working = true;
    try {
      const results = await api.deleteTags(repoId, list.map((t) => t.name));
      const done = results.filter((r) => !r.error && (r.remoteDeleted || r.localDeleted)).length;
      if (done) toast.success(`Deleted ${done} ${done === 1 ? "tag" : "tags"}.`);
      for (const f of results.filter((r) => r.error)) toast.error(`${f.tag}: ${f.error}`);
      pickedTags = [];
      await loadTags();
      void gr.refresh(repoId);
    } catch (e) {
      toast.error(messageOf(e));
    } finally {
      working = false;
    }
  }
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
      const done = results.filter((r) => r.ok).length;
      if (done) toast.success(`Deleted ${done} ${done === 1 ? "release" : "releases"}.`);
      for (const f of results.filter((r) => r.error)) toast.error(`${f.tag || f.releaseId}: ${f.error}`);
      for (const kept of results.filter((r) => r.tagKept)) toast.info(`${kept.tag}: ${kept.tagKept}`);
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
    if (editor.combined) return saveCombined();
    working = true;
    try {
      const updated = await api.updateRelease(repoId, editor.id, editor.title.trim(), editor.notes, editor.prerelease);
      releases = (releases ?? []).map((r) => (r.id === updated.id ? updated : r));
      editor = null;
      toast.success(`Saved ${updated.tagName}.`);
    } catch (e) {
      toast.error(messageOf(e));
    } finally {
      working = false;
    }
  }

  /** The releases a combine deletes (all picked but the one getting the notes). */
  const combineOthers = $derived(editor?.combined ? (releases ?? []).filter((r) => editor!.combined!.includes(r.id) && r.id !== editor!.id) : []);

  /** Why a release's tag stays when its release goes, if it does. */
  function tagStays(release: Release, target: Release, combined: number[]): string | null {
    if (release.tagName === target.tagName) return `the combined release uses it`;
    if ((releases ?? []).some((r) => r.tagName === release.tagName && !combined.includes(r.id))) return "another release uses it";
    return null;
  }

  /** What a combine couldn't delete, to try again. */
  let leftovers = $state<{ targetId: number; targetTag: string; failures: Deleted[] } | null>(null);
  let retrying = $state(false);

  function report(results: Deleted[], targetId: number, targetTag: string, combinedInto: string | null) {
    const failures = results.filter((d) => d.error);
    const releasesGone = results.filter((d) => d.ok && d.releaseId).length;
    const tagsGone = results.filter((d) => d.tagDeleted).length;
    if (combinedInto) {
      toast.success(
        `Combined into ${combinedInto}. Deleted ${releasesGone} ${releasesGone === 1 ? "release" : "releases"} and ${tagsGone} ${tagsGone === 1 ? "tag" : "tags"}.`,
      );
    } else if (releasesGone || tagsGone) {
      toast.success(`Deleted ${releasesGone ? `${releasesGone} ${releasesGone === 1 ? "release" : "releases"}` : ""}${releasesGone && tagsGone ? " and " : ""}${tagsGone ? `${tagsGone} ${tagsGone === 1 ? "tag" : "tags"}` : ""}.`);
    }
    for (const kept of results.filter((d) => d.tagKept)) toast.info(`${kept.tag}: ${kept.tagKept}`);
    leftovers = failures.length ? { targetId, targetTag, failures } : null;
  }

  async function saveCombined() {
    if (!editor?.combined) return;
    const combined = editor.combined;
    const target = (releases ?? []).find((r) => r.id === editor!.id);
    if (!target) return;
    const others = combineOthers;
    const lines = others.map((r) => {
      const stays = tagStays(r, target, combined);
      const name = r.name && r.name !== r.tagName ? `${r.name} (${r.tagName})` : r.tagName;
      return stays ? `• ${name}: the release only; its tag stays (${stays})` : `• ${name}: the release and its tag ${r.tagName}`;
    });
    const ok = await confirm({
      title: `Combine ${combined.length} releases into ${target.tagName}?`,
      message:
        `${target.tagName} gets the combined title and notes, and keeps its tag.\n\n` +
        `Then ${others.length === 1 ? "this is" : `these ${others.length} are`} deleted, on GitHub and on this PC:\n${lines.join("\n")}\n\n` +
        `If saving the notes fails, nothing is deleted. Downloads of the deleted releases' files stop working. This can't be undone.`,
      confirmLabel: `Save and delete ${others.length}`,
      danger: true,
    });
    if (!ok || !editor) return;
    working = true;
    try {
      const result = await api.combine(repoId, target.id, editor.title.trim(), editor.notes, editor.prerelease, others.map((r) => r.id));
      editor = null;
      picked = [];
      report(result.deleted, result.release.id, result.release.tagName, result.release.tagName);
      await load();
      void gr.loadRemote(repoId);
      void gr.refresh(repoId);
    } catch (e) {
      // Nothing was deleted: the editor stays open with the notes.
      toast.error(messageOf(e));
    } finally {
      working = false;
    }
  }

  async function retryLeftovers() {
    if (!leftovers) return;
    const { targetId, targetTag, failures } = leftovers;
    retrying = true;
    try {
      const results = await api.combineRetry(
        repoId,
        targetId,
        failures.filter((d) => !d.ok && d.releaseId).map((d) => d.releaseId),
        failures.filter((d) => d.ok && !d.tagDeleted && !d.tagKept && d.tag).map((d) => d.tag),
      );
      report(results, targetId, targetTag, null);
      await load();
      void gr.refresh(repoId);
    } catch (e) {
      toast.error(messageOf(e));
    } finally {
      retrying = false;
    }
  }

  function combine() {
    // The notes go on the newest one.
    const byDate = [...pickedReleases].sort((a, b) => Date.parse(b.publishedAt ?? b.createdAt ?? "") - Date.parse(a.publishedAt ?? a.createdAt ?? "") || 0);
    const ids = byDate.map((r) => r.id);
    if (ids.length < 2) return;
    const newest = byDate[0];
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
    <div class="views" role="tablist" aria-label="Show">
      <button role="tab" aria-selected={view === "releases"} class:active={view === "releases"} onclick={() => (view = "releases")}>
        Releases {#if releases}<span>{shown.length}</span>{/if}
      </button>
      <button role="tab" aria-selected={view === "tags"} class:active={view === "tags"} onclick={() => (view = "tags")} title="Tags no release uses (a failed or abandoned release leaves one)">
        <Tag size={12} /> Tags without a release {#if tags}<span class:warn={shownTags.length > 0}>{shownTags.length}</span>{/if}
      </button>
    </div>
    {#if entry.monorepo}
      <label class="opt"><input type="checkbox" class="switch" bind:checked={all} /> All of {item.repo.name}</label>
    {/if}
    <span class="grow"></span>
    <button class="icon-btn" aria-label="Refresh" title="Refresh" disabled={loading} onclick={() => void load()}>
      <RefreshCw size={14} class={loading ? "spin" : ""} />
    </button>
  </div>

  {#if view === "tags"}
    {#if tagsError}
      <div class="quiet">
        <span>{tagsError}</span>
        {#if !gr.page?.account}<button class="btn small" onclick={() => (gr.settingsOpen = "account")}>Connect GitHub</button>{/if}
      </div>
    {:else if !tags}
      <div class="quiet"><LoaderCircle size={16} class="spin" /></div>
    {:else if !shownTags.length}
      <div class="quiet">Every tag has a release.</div>
    {:else}
      <div class="selection" class:idle={!pickedLone.length}>
        <label class="opt">
          <input type="checkbox" class="check" checked={allTagsPicked} indeterminate={pickedLone.length > 0 && !allTagsPicked} onchange={() => (pickedTags = allTagsPicked ? [] : shownTags.map((t) => t.name))} aria-label="Pick all" />
          {pickedLone.length ? `${pickedLone.length} of ${shownTags.length} picked` : `Pick all ${shownTags.length}`}
        </label>
        <span class="grow"></span>
        <button class="btn small danger" disabled={working || !pickedLone.length} onclick={() => void removeTags(pickedLone.map((t) => t.name))}>
          {#if working}<LoaderCircle size={13} class="spin" />{:else}<Trash2 size={13} />{/if} Delete {pickedLone.length || ""} {pickedLone.length === 1 ? "tag" : "tags"}
        </button>
      </div>
      <ul class="list">
        {#each shownTags as t (t.name)}
          <li class:picked={pickedTags.includes(t.name)}>
            <input type="checkbox" class="check" checked={pickedTags.includes(t.name)} onchange={() => toggleTag(t.name)} aria-label="Pick {t.name}" />
            <div class="text">
              <strong><code class="tag-name">{t.name}</code></strong>
              <small>{where(t)}{t.date ? ` · ${formatRelative(t.date)}` : ""}</small>
            </div>
            <button class="icon-btn danger" title="Delete this tag" aria-label="Delete {t.name}" disabled={working} onclick={() => void removeTags([t.name])}><Trash2 size={13} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
  {#if picked.length}
    <div class="selection">
      <span>{pickedReleases.length} picked</span>
      <label class="opt"><input type="checkbox" class="check" bind:checked={deleteTags} /> Delete their tags too</label>
      <span class="grow"></span>
      <button class="btn small" disabled={pickedReleases.length < 2 || combining || working} onclick={() => (gr.aiReady ? combine() : (gr.settingsOpen = "ai"))} title="One set of notes from the picked releases on the newest; the others and their tags are then deleted (you confirm first)">
        {#if combining}<LoaderCircle size={13} class="spin" />{:else}<Combine size={13} />{/if} Combine notes
      </button>
      <button class="btn small danger" disabled={working} onclick={() => void remove(pickedReleases.map((r) => r.id))}><Trash2 size={13} /> Delete</button>
      <button class="icon-btn" aria-label="Clear the selection" onclick={() => (picked = [])}><X size={13} /></button>
    </div>
  {/if}

  {#if leftovers}
    <div class="leftovers" role="alert">
      <div class="leftovers-head">
        <strong>The combine into {leftovers.targetTag} left some behind</strong>
        <button class="icon-btn" aria-label="Dismiss" onclick={() => (leftovers = null)}><X size={13} /></button>
      </div>
      <ul>
        {#each leftovers.failures as f (f.releaseId + f.tag)}
          <li><code>{f.tag || f.releaseId}</code> {f.error}</li>
        {/each}
      </ul>
      <div class="leftovers-foot">
        <span class="hint">The notes are saved on {leftovers.targetTag}; its tag is never deleted.</span>
        <span class="grow"></span>
        <button class="btn small" disabled={retrying || working} onclick={() => void retryLeftovers()}>
          {#if retrying}<LoaderCircle size={13} class="spin" />{:else}<RefreshCw size={13} />{/if} Try again
        </button>
      </div>
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
        {#if editor.combined}<span class="hint">Saving deletes {combineOthers.map((r) => r.tagName).join(", ")} and their tags; you confirm first.</span>{/if}
        <button class="btn small ghost" disabled={working} onclick={() => (editor = null)}>Cancel</button>
        <button class="btn small primary" class:danger={!!editor.combined} disabled={working || !editor.title.trim() || (!!editor.combined && !editor.notes.trim())} onclick={() => void save()}>
          {#if working}<LoaderCircle size={13} class="spin" />{/if} {editor.combined ? `Save and delete ${combineOthers.length}` : "Save on GitHub"}
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
  }

  .bar,
  .selection,
  .editor-head,
  .editor-foot {
    display: flex;
    align-items: center;
    gap: 9px;
  }

  .editor-foot {
    flex-wrap: wrap;
  }

  .editor-foot .opt {
    white-space: nowrap;
  }

  .grow {
    flex: 1;
  }

  .views {
    display: flex;
    gap: 2px;
    padding: 3px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 10px;
    background: rgb(255 255 255 / 0.03);
  }

  .views button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--text-3);
    font: inherit;
    font-size: 12.3px;
    cursor: pointer;
  }

  .views button:hover {
    color: var(--text-1);
  }

  .views button.active {
    background: rgb(var(--accent-rgb) / 0.16);
    color: var(--text-1);
  }

  .views span {
    padding: 0 6px;
    border-radius: 99px;
    background: rgb(255 255 255 / 0.08);
    color: var(--text-2);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }

  .views span.warn {
    background: rgb(255 196 92 / 0.16);
    color: #ffd08a;
  }

  .selection.idle {
    border-color: rgb(255 255 255 / 0.06);
    background: rgb(255 255 255 / 0.03);
  }

  .tag-name {
    font-size: 12.5px;
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

  .leftovers {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border: 1px solid rgb(255 120 120 / 0.3);
    border-radius: 10px;
    background: rgb(255 120 120 / 0.06);
    font-size: 12.3px;
  }

  .leftovers-head,
  .leftovers-foot {
    display: flex;
    align-items: center;
    gap: 9px;
  }

  .leftovers-head strong {
    flex: 1;
  }

  .leftovers ul {
    display: grid;
    gap: 3px;
    margin: 0;
    padding-left: 16px;
    color: var(--text-2);
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
