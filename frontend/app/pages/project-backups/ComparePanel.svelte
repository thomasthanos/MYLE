<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { dialogFocus } from "../game-saves/dialog-focus";
  import {
    projectBackupsApi as api,
    SOURCE_ID,
    type Change,
    type ChangeStatus,
    type CompareResult,
    type FileDiff,
  } from "./api";
  import { formatBytes, projectBackupsState as pb } from "./state.svelte";

  const view = pb.compare!;
  const project = pb.projects.find((item) => item.id === view.projectId);
  let result = $state<CompareResult | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let query = $state("");
  let shown = $state<Record<ChangeStatus, boolean>>({ added: true, modified: true, deleted: true });
  let selected = $state<Change | null>(null);
  let diff = $state<FileDiff | null>(null);
  let diffError = $state<string | null>(null);
  let diffLoading = $state(false);

  const labels: Record<ChangeStatus, string> = { added: "Added", modified: "Modified", deleted: "Deleted" };

  function label(id: string) {
    if (id === SOURCE_ID) return "Project folder (now)";
    const backup = pb.backups[view.projectId]?.find((item) => item.id === id);
    return backup?.name ?? id.split("/").pop() ?? id;
  }

  onMount(() => {
    void (async () => {
      try {
        result = await api.compare(view.projectId, view.firstId, view.secondId);
      } catch (cause) {
        error = cause instanceof Error ? cause.message : String(cause);
      } finally {
        loading = false;
      }
    })();
  });

  const visible = $derived(
    (result?.comparison.changes ?? []).filter(
      (change) => shown[change.status] && (!query || change.path.toLowerCase().includes(query.toLowerCase())),
    ),
  );
  const incomplete = $derived((result?.comparison.changes ?? []).filter((change) => change.stillInSource).length);

  async function choose(change: Change) {
    if (!result) return;
    selected = change;
    diff = null;
    diffError = null;
    diffLoading = true;
    try {
      const value = await api.fileDiff(view.projectId, result.oldId, result.newId, change.oldName, change.newName);
      if (selected === change) diff = value;
    } catch (cause) {
      if (selected === change) diffError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (selected === change) diffLoading = false;
    }
  }

  function close() {
    if (loading) void api.cancel();
    pb.closeCompare();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") close();
  }

  const size = (value: number | null) => (value === null ? "—" : formatBytes(value));
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div
    class="dialog glass glass--3"
    aria-labelledby="compare-title"
    aria-modal="true"
    role="dialog"
    tabindex="-1"
    {@attach dialogFocus}
    transition:scale={{ start: 0.97, duration: 170, easing: cubicOut }}
  >
    <header>
      <span class="heading">
        <h2 id="compare-title">Compare {project?.name ?? "backups"}</h2>
        <p class="sides">
          <span class="selectable">{label(result?.oldId ?? view.firstId)}</span>
          <ArrowRight size={13} />
          <span class="selectable">{label(result?.newId ?? view.secondId)}</span>
        </p>
      </span>
      <button class="icon-btn" type="button" aria-label="Close" onclick={close}><X size={16} /></button>
    </header>

    {#if loading}
      <div class="state"><LoaderCircle size={17} class="spin" /> Reading both sides…</div>
    {:else if error}
      <div class="state error"><CircleAlert size={17} /> {error}</div>
    {:else if result}
      {@const comparison = result.comparison}
      <div class="filters">
        {#each ["added", "modified", "deleted"] as const as status (status)}
          <button class="chip {status}" class:active={shown[status]} aria-pressed={shown[status]}
            onclick={() => (shown = { ...shown, [status]: !shown[status] })}>
            {labels[status]} <span class="count">{comparison[status]}</span>
          </button>
        {/each}
        <span class="unchanged">{comparison.unchanged.toLocaleString()} unchanged</span>
        <label class="search">
          <Search size={13} />
          <input class="input" placeholder="Filter files" bind:value={query} aria-label="Filter files" />
        </label>
      </div>
      {#if incomplete}
        <p class="hint" role="note">
          <CircleAlert size={13} />
          {incomplete} deleted {incomplete === 1 ? "file is" : "files are"} still in the project folder: the newer backup is missing
          {incomplete === 1 ? "it" : "them"} (it was incomplete, or its exclusions differ).
        </p>
      {/if}

      <div class="body">
        <ul class="changes" aria-label="Changed files">
          {#each visible as change (change.path)}
            <li>
              <button class="change" class:active={selected?.path === change.path} onclick={() => choose(change)}>
                <span class="badge {change.status}">{change.status === "added" ? "A" : change.status === "modified" ? "M" : "D"}</span>
                <span class="path" title={change.path}>{change.path}</span>
                {#if change.stillInSource}<span class="still" title="Still in the project folder">in folder</span>{/if}
              </button>
            </li>
          {:else}
            <li class="none">{comparison.changes.length ? "No files match." : "No differences: both hold the same files."}</li>
          {/each}
        </ul>

        <div class="diff">
          {#if !selected}
            <p class="placeholder">Choose a file to see what changed.</p>
          {:else if diffLoading}
            <p class="placeholder"><LoaderCircle size={14} class="spin" /> Reading {selected.path}…</p>
          {:else if diffError}
            <p class="placeholder error">{diffError}</p>
          {:else if diff}
            <div class="diff-head">
              <span class="selectable">{selected.path}</span>
              <span class="sizes">{size(selected.oldSize)} → {size(selected.newSize)}</span>
            </div>
            {#if diff.kind === "binary"}
              <p class="placeholder">A binary file: {size(diff.oldSize)} → {size(diff.newSize)}.</p>
            {:else if diff.kind === "tooLarge"}
              <p class="placeholder">Too large to show line by line (over 2 MB).</p>
            {:else if diff.identical}
              <p class="placeholder">Same text; only line endings or encoding differ.</p>
            {:else}
              <div class="table" role="table" aria-label={`Changes in ${selected.path}`}>
                {#each diff.rows as row, index (index)}
                  {#if row.kind === "gap"}
                    <div class="gap" role="row"><span role="cell">⋯</span></div>
                  {:else}
                    <div class="line {row.kind}" role="row">
                      <span class="no" role="cell">{row.oldLine ?? ""}</span>
                      <span class="text old" role="cell">{row.oldText ?? ""}</span>
                      <span class="no" role="cell">{row.newLine ?? ""}</span>
                      <span class="text new" role="cell">{row.newText ?? ""}</span>
                    </div>
                  {/if}
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 24px; background: rgb(4 6 12 / 0.58); }
  .dialog {
    display: flex; flex-direction: column; gap: 12px; width: min(1180px, 100%); height: min(780px, calc(100vh - 48px));
    padding: 18px 20px; border-radius: var(--radius-xl);
  }
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  h2 { font-size: 17px; }
  .sides { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; margin-top: 4px; color: var(--text-2); font-family: var(--font-mono); font-size: 11.5px; }
  .state { display: flex; align-items: center; justify-content: center; gap: 8px; flex: 1; color: var(--text-3); font-size: 12.5px; }
  .error { color: rgb(255 145 145 / 0.85); }
  .filters { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
  .chip.added.active { border-color: rgb(62 207 142 / 0.35); color: #98dfbd; }
  .chip.modified.active { border-color: rgb(245 176 65 / 0.35); color: rgb(245 188 95 / 0.95); }
  .chip.deleted.active { border-color: rgb(229 72 77 / 0.35); color: rgb(255 145 145 / 0.95); }
  .unchanged { color: var(--text-3); font-size: 11px; }
  .search { display: flex; align-items: center; gap: 6px; margin-left: auto; color: var(--text-3); }
  .search input { width: 220px; height: 30px; font-size: 12px; }
  .hint { display: flex; align-items: center; gap: 6px; color: rgb(245 188 95 / 0.88); font-size: 11.5px; }
  .body { display: grid; grid-template-columns: minmax(220px, 0.42fr) minmax(0, 1fr); gap: 10px; flex: 1; min-height: 0; }
  .changes { margin: 0; padding: 4px; overflow: auto; list-style: none; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 10px; background: rgb(0 0 0 / 0.12); }
  .change { display: flex; align-items: center; gap: 7px; width: 100%; min-width: 0; padding: 4px 6px; border-radius: 6px; text-align: left; font-size: 11.5px; }
  .change:hover { background: rgb(255 255 255 / 0.04); }
  .change.active { background: var(--selected); }
  .badge { display: grid; place-items: center; flex: none; width: 17px; height: 17px; border-radius: 4px; font-size: 10px; font-weight: 700; }
  .badge.added { background: rgb(62 207 142 / 0.15); color: #98dfbd; }
  .badge.modified { background: rgb(245 176 65 / 0.15); color: rgb(245 188 95); }
  .badge.deleted { background: rgb(229 72 77 / 0.15); color: rgb(255 145 145); }
  .path { min-width: 0; overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; direction: rtl; text-align: left; }
  .still { flex: none; margin-left: auto; padding: 0 5px; border-radius: 4px; background: rgb(245 176 65 / 0.1); color: rgb(245 188 95 / 0.9); font-size: 9.5px; }
  .none { padding: 12px; color: var(--text-3); font-size: 11.5px; }
  .diff { display: flex; flex-direction: column; min-width: 0; min-height: 0; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 10px; background: rgb(0 0 0 / 0.14); overflow: hidden; }
  .placeholder { display: flex; align-items: center; gap: 6px; padding: 14px; color: var(--text-3); font-size: 12px; }
  .diff-head { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-bottom: 1px solid rgb(255 255 255 / 0.055); color: var(--text-2); font-family: var(--font-mono); font-size: 11px; }
  .diff-head span:first-child { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .sizes { margin-left: auto; flex: none; color: var(--text-3); }
  .table { flex: 1; overflow: auto; font-family: var(--font-mono); font-size: 11px; line-height: 1.55; }
  .line { display: grid; grid-template-columns: 44px minmax(0, 1fr) 44px minmax(0, 1fr); min-width: 0; }
  .no { padding: 0 6px; color: var(--text-3); text-align: right; user-select: none; opacity: 0.7; }
  .text { padding: 0 8px; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--text-2); user-select: text; }
  .text.old { border-right: 1px solid rgb(255 255 255 / 0.05); }
  .line.removed .old, .line.changed .old { background: rgb(229 72 77 / 0.12); color: rgb(255 190 190); }
  .line.added .new, .line.changed .new { background: rgb(62 207 142 / 0.11); color: rgb(190 240 215); }
  .gap { padding: 1px 10px; background: rgb(var(--accent-rgb) / 0.05); color: var(--text-3); }
  @media (max-width: 780px) { .body { grid-template-columns: minmax(0, 1fr); grid-template-rows: 40% 1fr; } .search { margin-left: 0; } }
</style>
