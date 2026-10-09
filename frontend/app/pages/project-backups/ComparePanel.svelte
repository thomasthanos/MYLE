<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import FileCode from "@lucide/svelte/icons/file-code";
  import FileImage from "@lucide/svelte/icons/file-image";
  import FileQuestion from "@lucide/svelte/icons/file-question";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { readJson, writeJson } from "../../../lib/storage";
  import { dialogFocus } from "../game-saves/dialog-focus";
  import {
    projectBackupsApi as api,
    SOURCE_ID,
    type Change,
    type ChangeStatus,
    type CompareProgress,
    type CompareResult,
    type FileDiff,
  } from "./api";
  import DiffView from "./DiffView.svelte";
  import { formatBytes, projectBackupsState as pb } from "./state.svelte";

  const view = pb.compare!;
  const project = pb.projects.find((item) => item.id === view.projectId);
  let result = $state<CompareResult | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let progress = $state<CompareProgress | null>(null);
  let cancelling = $state(false);
  let query = $state("");
  let shown = $state<Record<ChangeStatus, boolean>>({ added: true, modified: true, deleted: true });
  let selected = $state<Change | null>(null);
  let diff = $state<FileDiff | null>(null);
  let diffError = $state<string | null>(null);
  let diffLoading = $state(false);
  let listElement = $state<HTMLUListElement>();
  const MODE_KEY = "myle.projectBackups.diffMode";
  let mode = $state<"split" | "unified">(
    readJson(MODE_KEY, "split", (value) => value === "split" || value === "unified"),
  );
  /** Rows of the change list drawn (a comparison can list many thousands). */
  const LIST_STEP = 400;
  let listLimit = $state(LIST_STEP);
  /** Previews already read, so going back and forth is instant. */
  const cache = new Map<string, FileDiff>();
  let closed = false;

  const labels: Record<ChangeStatus, string> = { added: "Added", modified: "Modified", deleted: "Deleted" };

  function label(id: string) {
    if (id === SOURCE_ID) return "Project folder (now)";
    const backup = pb.backups[view.projectId]?.find((item) => item.id === id);
    return backup?.name ?? id.split("/").pop() ?? id;
  }

  onMount(() => {
    void (async () => {
      try {
        result = await api.compare(view.projectId, view.firstId, view.secondId, (value) => (progress = value));
        // The first change is opened right away: one less click.
        if (!closed && visible.length) void choose(visible[0]);
      } catch (cause) {
        error = cause instanceof Error ? cause.message : String(cause);
      } finally {
        loading = false;
      }
    })();
    return () => {
      closed = true;
    };
  });

  const visible = $derived(
    (result?.comparison.changes ?? []).filter(
      (change) => shown[change.status] && (!query || change.path.toLowerCase().includes(query.toLowerCase())),
    ),
  );
  const incomplete = $derived((result?.comparison.changes ?? []).filter((change) => change.stillInSource).length);

  $effect(() => {
    // A new filter starts the list from the top again.
    void query;
    void shown;
    listLimit = LIST_STEP;
  });

  const key = (change: Change) => `${change.oldName ?? ""}|${change.newName ?? ""}`;

  let timer: ReturnType<typeof setTimeout> | undefined;

  /** Opens the preview of `change`; `delay` while stepping with the keyboard. */
  async function choose(change: Change, delay = 0) {
    if (!result) return;
    selected = change;
    diffError = null;
    clearTimeout(timer);
    const known = cache.get(key(change));
    if (known) {
      diff = known;
      diffLoading = false;
      return;
    }
    diff = null;
    diffLoading = true;
    if (delay) {
      await new Promise((resolve) => (timer = setTimeout(resolve, delay)));
      if (selected !== change) return;
    }
    try {
      const value = await api.fileDiff(view.projectId, result.oldId, result.newId, change.oldName, change.newName);
      if (cache.size > 40) cache.delete(cache.keys().next().value!);
      cache.set(key(change), value);
      if (selected === change) diff = value;
    } catch (cause) {
      if (selected === change) diffError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (selected === change) diffLoading = false;
    }
  }

  function step(by: number) {
    if (!visible.length) return;
    const index = selected ? visible.findIndex((change) => change.path === selected!.path) : -1;
    const next = Math.max(0, Math.min(visible.length - 1, index < 0 ? 0 : index + by));
    if (next >= listLimit) listLimit = next + LIST_STEP;
    void choose(visible[next], 140);
    requestAnimationFrame(() =>
      listElement?.querySelector<HTMLElement>(`[data-index="${next}"]`)?.scrollIntoView({ block: "nearest" }),
    );
  }

  function setMode(value: "split" | "unified") {
    mode = value;
    writeJson(MODE_KEY, value);
  }

  async function cancelCompare() {
    if (!loading || cancelling) return;
    cancelling = true;
    try {
      await api.cancel("compare");
    } catch {
      cancelling = false;
    }
  }

  function close() {
    closed = true;
    clearTimeout(timer);
    if (loading) void api.cancel("compare");
    pb.closeCompare();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      if (query && document.activeElement?.closest(".search")) query = "";
      else close();
      return;
    }
    const target = event.target as HTMLElement | null;
    const typing = !!target?.closest("input, textarea") && !target?.closest(".search");
    if (typing || !result) return;
    if (event.key === "ArrowDown" || (event.key === "j" && !target?.closest("input"))) {
      event.preventDefault();
      step(1);
    } else if (event.key === "ArrowUp" || (event.key === "k" && !target?.closest("input"))) {
      event.preventDefault();
      step(-1);
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      document.querySelector<HTMLInputElement>(".compare-search")?.focus();
    }
  }

  const size = (value: number | null) => (value === null ? "—" : formatBytes(value));
  const percent = $derived(progress?.total ? Math.min(100, (progress.done / progress.total) * 100) : null);
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
      <button class="icon-btn" type="button" aria-label="Close" title="Close (Esc)" onclick={close}><X size={16} /></button>
    </header>

    {#if loading}
      <div class="state" role="status">
        <span class="state-line">
          <LoaderCircle size={17} class="spin" />
          {#if progress?.total}
            Comparing files… {progress.done.toLocaleString()} of {progress.total.toLocaleString()}
          {:else}
            Reading both sides…
          {/if}
        </span>
        <span class="bar" class:indeterminate={percent === null} aria-hidden="true">
          <span style:width={percent === null ? undefined : `${percent}%`}></span>
        </span>
        <small>Files with the same size and date are compared by their checksum; a large project takes a moment.</small>
        <button class="btn small" disabled={cancelling} onclick={cancelCompare}>{cancelling ? "Cancelling…" : "Cancel"}</button>
      </div>
    {:else if error}
      <div class="state error" role="alert">
        <span class="state-line"><CircleAlert size={17} /> {error}</span>
        <button class="btn small" onclick={close}>Close</button>
      </div>
    {:else if result}
      {@const comparison = result.comparison}
      <div class="filters">
        {#each ["added", "modified", "deleted"] as const as status (status)}
          <button class="chip {status}" class:active={shown[status]} aria-pressed={shown[status]}
            onclick={() => (shown = { ...shown, [status]: !shown[status] })}>
            {labels[status]} <span class="count">{comparison[status].toLocaleString()}</span>
          </button>
        {/each}
        <span class="unchanged">{comparison.unchanged.toLocaleString()} unchanged</span>
        <label class="search">
          <Search size={13} />
          <input class="input compare-search" type="search" placeholder="Filter files" bind:value={query}
            aria-label="Filter files" aria-keyshortcuts="Control+F" />
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
        <div class="list-pane">
          <ul class="changes" aria-label="Changed files" bind:this={listElement}>
            {#each visible.slice(0, listLimit) as change, index (change.path)}
              <li>
                <button class="change" class:active={selected?.path === change.path} data-index={index}
                  aria-current={selected?.path === change.path ? "true" : undefined}
                  title={`${labels[change.status]}: ${change.path}`} onclick={() => choose(change)}>
                  <span class="badge {change.status}">{change.status === "added" ? "A" : change.status === "modified" ? "M" : "D"}</span>
                  <span class="kind" aria-hidden="true">
                    {#if change.kind === "image"}<FileImage size={12} />{:else if change.kind === "binary"}<FileQuestion size={12} />{:else}<FileCode size={12} />{/if}
                  </span>
                  <span class="path"><bdi>{change.path}</bdi></span>
                  {#if change.stillInSource}<span class="still" title="Still in the project folder">in folder</span>{/if}
                </button>
              </li>
            {:else}
              <li class="none">
                {#if comparison.changes.length}
                  No files match.
                  <button class="btn small ghost" onclick={() => { query = ""; shown = { added: true, modified: true, deleted: true }; }}>Show all</button>
                {:else}
                  No differences: both hold the same files.
                {/if}
              </li>
            {/each}
            {#if visible.length > listLimit}
              <li class="none">
                <button class="btn small" onclick={() => (listLimit += LIST_STEP * 2)}>
                  Show more ({(visible.length - listLimit).toLocaleString()} left)
                </button>
              </li>
            {/if}
          </ul>
          <p class="keys">↑ ↓ to step through files · Ctrl+F to filter · Esc to close</p>
        </div>

        <div class="diff">
          {#if !selected}
            <p class="placeholder">{comparison.changes.length ? "Choose a file to preview its changes." : "Nothing to preview."}</p>
          {:else}
            <div class="diff-head">
              <span class="badge {selected.status}">{selected.status === "added" ? "A" : selected.status === "modified" ? "M" : "D"}</span>
              <span class="selectable diff-path">{selected.path}</span>
              <span class="sizes">{size(selected.oldSize)} → {size(selected.newSize)}</span>
              {#if diff && (diff.kind === "text" || (diff.kind === "image" && diff.rows)) && selected.status === "modified"}
                <span class="mode" role="group" aria-label="Diff layout">
                  <button class="icon-btn" class:active={mode === "split"} aria-pressed={mode === "split"} title="Side by side"
                    onclick={() => setMode("split")}><Columns2 size={14} /></button>
                  <button class="icon-btn" class:active={mode === "unified"} aria-pressed={mode === "unified"} title="Unified"
                    onclick={() => setMode("unified")}><Rows2 size={14} /></button>
                </span>
              {/if}
            </div>
            {#if diffLoading}
              <p class="placeholder"><LoaderCircle size={14} class="spin" /> Reading {selected.path}…</p>
            {:else if diffError}
              <p class="placeholder error"><CircleAlert size={14} /> {diffError}</p>
            {:else if diff}
              {#key key(selected)}
                <DiffView change={selected} {diff} {mode} />
              {/key}
            {/if}
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 24px; background: var(--scrim); }
  .dialog {
    display: flex; flex-direction: column; gap: 12px; width: min(1180px, 100%); height: min(780px, calc(100vh - 48px));
    padding: 18px 20px; border-radius: var(--radius-xl);
  }
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  h2 { font-size: 17px; }
  .sides { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; margin-top: 4px; color: var(--text-2); font-family: var(--font-mono); font-size: 11.5px; }
  .state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; flex: 1; color: var(--text-3); font-size: 12.5px; text-align: center; }
  .state-line { display: flex; align-items: center; gap: 8px; }
  .state small { max-width: 420px; color: var(--text-3); font-size: 11px; }
  .bar { position: relative; width: min(360px, 80%); height: 3px; overflow: hidden; border-radius: 999px; background: rgb(255 255 255 / 0.07); }
  .bar > span { display: block; height: 100%; border-radius: inherit; background: var(--accent-grad); transition: width var(--dur-med) var(--ease-out); }
  .bar.indeterminate > span { width: 30%; animation: sweep 1.3s var(--ease-in-out) infinite; }
  @keyframes sweep { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
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
  .list-pane { display: flex; flex-direction: column; gap: 5px; min-width: 0; min-height: 0; }
  .keys { color: var(--text-3); font-size: 10px; opacity: 0.8; }
  .kind { display: grid; flex: none; place-items: center; color: var(--text-3); opacity: 0.75; }
  .mode { display: inline-flex; gap: 2px; flex: none; }
  .mode .active { background: var(--selected); color: var(--text-1); }
  .diff-path { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .changes { flex: 1; min-height: 0; margin: 0; padding: 4px; overflow: auto; list-style: none; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 10px; background: rgb(0 0 0 / 0.12); }
  .change { display: flex; align-items: center; gap: 7px; width: 100%; min-width: 0; padding: 4px 6px; border-radius: 6px; text-align: left; font-size: 11.5px; }
  .change:hover { background: rgb(255 255 255 / 0.04); }
  .change.active { background: var(--selected); }
  .badge { display: grid; place-items: center; flex: none; width: 17px; height: 17px; border-radius: 4px; font-size: 10px; font-weight: 700; }
  .badge.added { background: rgb(62 207 142 / 0.15); color: #98dfbd; }
  .badge.modified { background: rgb(245 176 65 / 0.15); color: rgb(245 188 95); }
  .badge.deleted { background: rgb(229 72 77 / 0.15); color: rgb(255 145 145); }
  .path { min-width: 0; overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; direction: rtl; text-align: left; }
  .still { flex: none; margin-left: auto; padding: 0 5px; border-radius: 4px; background: rgb(245 176 65 / 0.1); color: rgb(245 188 95 / 0.9); font-size: 9.5px; }
  .none { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding: 12px; color: var(--text-3); font-size: 11.5px; }
  .diff { display: flex; flex-direction: column; min-width: 0; min-height: 0; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 10px; background: rgb(0 0 0 / 0.14); overflow: hidden; }
  .placeholder { display: flex; align-items: center; gap: 6px; padding: 14px; color: var(--text-3); font-size: 12px; }
  .diff-head { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-bottom: 1px solid rgb(255 255 255 / 0.055); color: var(--text-2); font-family: var(--font-mono); font-size: 11px; }
  .sizes { margin-left: auto; flex: none; color: var(--text-3); }
  @media (max-width: 780px) { .body { grid-template-columns: minmax(0, 1fr); grid-template-rows: 40% 1fr; } .search { margin-left: 0; } }
</style>
