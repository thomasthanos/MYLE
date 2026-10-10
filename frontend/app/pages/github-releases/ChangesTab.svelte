<script lang="ts">
  // The repository's uncommitted changes: pick what goes in, see each
  // file's diff, write the message (or let AI), commit and push.
  import { onMount, untrack } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import GitCommitHorizontal from "@lucide/svelte/icons/git-commit-horizontal";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import SquareTerminal from "@lucide/svelte/icons/square-terminal";
  import X from "@lucide/svelte/icons/x";
  import { readJson, writeJson } from "../../../lib/storage";
  import DiffView from "../project-backups/DiffView.svelte";
  import type { Change } from "../project-backups/api";
  import { githubReleasesApi as api, messageOf, type ChangeKind, type FileChange, type FileDiff, type GitStatus } from "./api";
  import CleanOverview from "./CleanOverview.svelte";
  import { githubReleases as gr, type ListItem } from "./state.svelte";
  let { item }: { item: ListItem } = $props();
  const repoId = $derived(item.repo.id);
  const entry = $derived(item.entry);
  const status = $derived(item.status);
  const MODE_KEY = "myle.githubReleases.diffMode";
  let git = $state<GitStatus | null>(null);
  let loadError = $state<string | null>(null);
  let onlyApp = $state(true);
  let selectedPath = $state<string | null>(null);
  let diff = $state<FileDiff | null>(null);
  let diffError = $state<string | null>(null);
  let diffLoading = $state(false);
  let mode = $state<"split" | "unified">(readJson(MODE_KEY, "unified", (v) => v === "split" || v === "unified"));
  let staging = $state(false);
  let generating = $state(false);
  let outputOpen = $state(false);
  /** The message box keeps its text per repository while MYLE is open. */
  let message = $state(untrack(() => gr.drafts[item.repo.id] ?? ""));
  $effect(() => {
    gr.drafts[repoId] = message;
  });
  const busy = $derived(gr.running(repoId));
  const output = $derived(gr.gitOutput[repoId] ?? null);
  const changes = $derived.by<FileChange[]>(() => {
    const all = git?.changes ?? [];
    if (!entry?.monorepo || !onlyApp || !entry.sub) return all;
    const prefix = `${entry.sub}/`;
    return all.filter((c) => c.path.startsWith(prefix));
  });
  const hidden = $derived((git?.changes.length ?? 0) - changes.length);
  const staged = $derived(changes.filter((c) => c.staged !== "none"));
  const stagedAnywhere = $derived((git?.changes ?? []).filter((c) => c.staged !== "none").length);
  const allStaged = $derived(changes.length > 0 && changes.every((c) => c.staged === "all"));
  const someStaged = $derived(staged.length > 0 && !allStaged);
  const selected = $derived(changes.find((c) => c.path === selectedPath) ?? null);
  const conflicted = $derived(changes.some((c) => c.kind === "conflicted"));
  async function load() {
    try {
      git = await api.changes(repoId);
      loadError = null;
      if (selectedPath && !git.changes.some((c) => c.path === selectedPath)) selectedPath = null;
      if (!selectedPath && changes.length) void open(changes[0].path);
      else if (selectedPath) void open(selectedPath, true);
    } catch (error) {
      loadError = messageOf(error);
    }
  }
  onMount(() => {
    void load();
  });
  // The status changed (a refresh, a commit): read the files again.
  let seen = "";
  $effect(() => {
    const key = `${status?.changes}|${status?.branch.head}|${gr.refreshing[repoId] ? 1 : 0}`;
    if (key !== seen && !gr.refreshing[repoId]) {
      const first = seen === "";
      seen = key;
      if (!first) void load();
    }
  });
  async function open(path: string, quiet = false) {
    selectedPath = path;
    if (!quiet) {
      diff = null;
      diffLoading = true;
    }
    diffError = null;
    try {
      const result = await api.fileDiff(repoId, path);
      if (selectedPath === path) diff = result;
    } catch (error) {
      if (selectedPath === path) diffError = messageOf(error);
    } finally {
      if (selectedPath === path) diffLoading = false;
    }
  }
  async function stage(paths: string[], on: boolean) {
    if (!paths.length || staging) return;
    staging = true;
    try {
      git = await api.stage(repoId, paths, on);
    } catch (error) {
      gr.showProblem({ code: "FAILED", message: messageOf(error), details: null });
    } finally {
      staging = false;
    }
  }
  function toggleAll() {
    if (allStaged) void stage(changes.map((c) => c.path), false);
    else void stage(changes.filter((c) => c.staged !== "all").map((c) => c.path), true);
  }
  async function commit(push: boolean) {
    if (!message.trim()) {
      document.getElementById("gr-commit-message")?.focus();
      return;
    }
    // Nothing picked: everything shown goes in.
    if (!stagedAnywhere) {
      await stage(changes.map((c) => c.path), true);
      if (!git?.changes.some((c) => c.staged !== "none")) return;
    }
    outputOpen = push;
    if (await gr.commit(repoId, message.trim(), push)) {
      message = "";
      await load();
    }
  }
  function generate() {
    void gr.ai({
      run: (provider) => api.aiCommitMessage(repoId, provider),
      apply: (answer) => {
        message = answer.text;
        aiNote = `${providerName(answer.provider)} · ${answer.model}`;
      },
      setBusy: (value) => (generating = value),
    });
  }
  let aiNote = $state<string | null>(null);
  const providerName = (id: string) => gr.page?.ai.providers.find((p) => p.id === id)?.name ?? id;
  const firstAi = $derived(gr.page?.ai.order.map((id) => gr.page?.ai.providers.find((p) => p.id === id)).find((p) => p?.ready) ?? null);
  const letters: Record<ChangeKind, string> = {
    added: "A",
    modified: "M",
    deleted: "D",
    renamed: "R",
    copied: "C",
    typeChanged: "T",
    untracked: "U",
    conflicted: "!",
  };
  const kindNames: Record<ChangeKind, string> = {
    added: "Added",
    modified: "Modified",
    deleted: "Deleted",
    renamed: "Renamed",
    copied: "Copied",
    typeChanged: "Type changed",
    untracked: "New, not tracked yet",
    conflicted: "Merge conflict",
  };
  function split(path: string): [string, string] {
    const at = path.lastIndexOf("/");
    return at < 0 ? ["", path] : [path.slice(0, at + 1), path.slice(at + 1)];
  }
  /** DiffView's view of a git change. */
  const change = $derived.by<Change | null>(() => {
    if (!selected || !diff) return null;
    const removed = selected.kind === "deleted";
    const added = selected.kind === "added" || selected.kind === "untracked";
    const sizes = diff.kind === "binary" ? [diff.old?.size ?? null, diff.new?.size ?? null] : diff.kind === "image" ? [diff.oldInfo?.size ?? null, diff.newInfo?.size ?? null] : [null, null];
    return {
      path: selected.path,
      kind: diff.kind,
      status: removed ? "deleted" : added ? "added" : "modified",
      oldName: removed || !added ? (selected.origPath ?? selected.path) : null,
      newName: removed ? null : selected.path,
      oldSize: sizes[0],
      newSize: sizes[1],
      stillInSource: false,
    };
  });
  function setMode(next: "split" | "unified") {
    mode = next;
    writeJson(MODE_KEY, next);
  }
  function onMessageKey(event: KeyboardEvent) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void commit(event.shiftKey);
    }
  }
  let checkboxes = $state<Record<string, HTMLInputElement>>({});
  let allBox = $state<HTMLInputElement>();
  $effect(() => {
    if (allBox) allBox.indeterminate = someStaged;
    for (const c of changes) {
      const box = checkboxes[c.path];
      if (box) box.indeterminate = c.staged === "partial";
    }
  });
</script>
<div class="changes">
  {#if status && status.branch.behind > 0}
    <div class="notice">
      <ArrowDown size={15} />
      <span>GitHub has <strong>{status.branch.behind}</strong> newer {status.branch.behind === 1 ? "commit" : "commits"}. Pull them before you push (your changes are kept).</span>
      <button class="btn small primary" disabled={!!busy} onclick={() => void gr.pull(repoId)}>
        {#if busy === "pull"}<LoaderCircle size={13} class="spin" />{/if} Pull (rebase)
      </button>
    </div>
  {/if}
  {#if git && !loadError && !changes.length}
    <CleanOverview {item} {hidden} onShowHidden={entry?.monorepo ? () => (onlyApp = false) : undefined} />
  {:else}
  <div class="grid">
    <section class="files">
      <div class="files-head">
        <label class="all">
          <input type="checkbox" class="check" bind:this={allBox} checked={allStaged} disabled={!changes.length || staging} onchange={toggleAll} aria-label="Pick every file" />
          <span>{changes.length ? `${changes.length} changed ${changes.length === 1 ? "file" : "files"}` : "No changes"}</span>
        </label>
        {#if staged.length}<span class="picked">{staged.length} picked</span>{/if}
        {#if staging}<LoaderCircle size={13} class="spin" />{/if}
      </div>
      {#if entry?.monorepo && entry.sub}
        <label class="only">
          <input type="checkbox" class="switch" bind:checked={onlyApp} />
          <span>Only {entry.name}{hidden && onlyApp ? ` (${hidden} elsewhere)` : ""}</span>
        </label>
      {/if}
      <div class="file-list" role="listbox" aria-label="Changed files">
        {#if loadError}
          <p class="error">{loadError}</p>
        {:else if !git}
          <div class="quiet"><LoaderCircle size={15} class="spin" /></div>
        {:else}
          {#each changes as c (c.path)}
            {@const [dir, name] = split(c.path)}
            <div class="file" class:selected={c.path === selectedPath} role="option" aria-selected={c.path === selectedPath}>
              <input
                type="checkbox"
                class="check"
                bind:this={checkboxes[c.path]}
                checked={c.staged === "all"}
                disabled={staging}
                onchange={() => void stage([c.path], c.staged !== "all")}
                aria-label="Include {c.path}"
              />
              <button class="file-name" onclick={() => void open(c.path)} title={c.origPath ? `${c.origPath} → ${c.path}` : c.path}>
                <span class="name">{name}</span>
                <span class="dir">{dir}</span>
              </button>
              <span class="kind k-{c.kind}" title={kindNames[c.kind]}>{letters[c.kind]}</span>
            </div>
          {:else}
            <div class="quiet clean">
              <CircleCheck size={22} />
              <strong>Nothing to commit</strong>
              <span>{hidden ? `${hidden} changed ${hidden === 1 ? "file is" : "files are"} in other apps.` : "The working tree matches the last commit."}</span>
            </div>
          {/each}
        {/if}
      </div>
      <div class="composer">
        <textarea
          id="gr-commit-message"
          class="input"
          rows="3"
          bind:value={message}
          placeholder={changes.length ? "Commit message (Ctrl+Enter commits, Ctrl+Shift+Enter pushes too)" : "Commit message"}
          onkeydown={onMessageKey}
          spellcheck="true"
        ></textarea>
        <div class="ai-row">
          <button
            class="btn small ghost"
            disabled={generating || !changes.length}
            onclick={() => (gr.aiReady ? generate() : (gr.settingsOpen = "ai"))}
            title={gr.aiReady ? `Write the message from the diff with ${firstAi?.name ?? "AI"}` : "Set up an AI provider first"}
          >
            {#if generating}<LoaderCircle size={13} class="spin" />{:else}<Sparkles size={13} />{/if}
            {generating ? "Writing…" : "Generate with AI"}
          </button>
          {#if aiNote && !generating}<span class="ai-note">{aiNote}</span>{/if}
        </div>
        {#if conflicted}<p class="error">Resolve the merge conflicts first.</p>{/if}
        <div class="commit-row">
          <button class="btn" disabled={!!busy || !changes.length || conflicted || !message.trim()} onclick={() => void commit(false)}>
            {#if busy === "commit"}<LoaderCircle size={14} class="spin" />{:else}<GitCommitHorizontal size={14} />{/if}
            {stagedAnywhere ? `Commit ${stagedAnywhere}` : "Commit all"}
          </button>
          <button
            class="btn primary"
            disabled={!!busy || !changes.length || conflicted || !message.trim() || (status?.branch.behind ?? 0) > 0}
            title={(status?.branch.behind ?? 0) > 0 ? "Pull first: GitHub has newer commits" : undefined}
            onclick={() => void commit(true)}
          >
            {#if busy === "commit-push"}<LoaderCircle size={14} class="spin" />{:else}<ArrowUp size={14} />{/if}
            Commit & Push
          </button>
        </div>
      </div>
    </section>
    <section class="diff">
      {#if selected}
        <div class="diff-head">
          <code title={selected.path}>{selected.origPath ? `${selected.origPath} → ` : ""}{selected.path}</code>
          <div class="modes">
            <button class="icon-btn" class:active={mode === "unified"} title="One column" aria-label="One column" onclick={() => setMode("unified")}><Rows2 size={14} /></button>
            <button class="icon-btn" class:active={mode === "split"} title="Side by side" aria-label="Side by side" onclick={() => setMode("split")}><Columns2 size={14} /></button>
          </div>
        </div>
        <div class="diff-body">
          {#if diffError}
            <p class="error">{diffError}</p>
          {:else if diffLoading || !diff || !change}
            <div class="quiet"><LoaderCircle size={16} class="spin" /></div>
          {:else}
            {#key `${selected.path}|${selected.staged}`}
              <DiffView {change} {diff} {mode} />
            {/key}
          {/if}
        </div>
      {:else}
        <div class="quiet">Choose a file to see what changed.</div>
      {/if}
    </section>
  </div>
  {/if}
  {#if output}
    <div class="output" class:open={outputOpen}>
      <button class="output-head" onclick={() => (outputOpen = !outputOpen)}>
        <SquareTerminal size={13} /> {output.action}
        {#if output.running}<LoaderCircle size={12} class="spin" />{/if}
        <span class="grow"></span>
        <ChevronDown size={13} class="chev" />
      </button>
      {#if outputOpen}
        <pre class="selectable">{output.lines.join("\n") || "…"}</pre>
        {#if !output.running}
          <button class="icon-btn close" aria-label="Clear" onclick={() => delete gr.gitOutput[repoId]}><X size={12} /></button>
        {/if}
      {/if}
    </div>
  {/if}
</div>
<style>
  .changes { display: flex; flex: 1; flex-direction: column; gap: 12px; min-height: 0; padding: 14px 18px; overflow-y: auto; container: changes / inline-size; }
  .notice,
  .output { flex: none; }
  .notice { display: flex; align-items: center; gap: 10px; padding: 9px 14px; border: 1px solid rgb(127 216 255 / 0.28); border-radius: 10px; background: rgb(127 216 255 / 0.07); color: #9fdcff; font-size: 12.5px; }
  .notice span { flex: 1; color: var(--text-2); }
  .grid { display: grid; flex: 1; grid-template-columns: minmax(290px, 380px) minmax(0, 1fr); gap: 12px; min-height: 0; }
  /* Too narrow for side by side: the files, then the diff, and the tab
     scrolls. */
  @container changes (max-width: 760px) {
    .grid {
      display: flex;
      flex: none;
      flex-direction: column;
    }
    .file-list {
      max-height: 240px;
    }
    .diff {
      flex: none;
      min-height: 320px;
    }
  }
  section { display: flex; flex-direction: column; min-width: 0; min-height: 0; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 12px; background: rgb(0 0 0 / 0.15); }
  .files-head { display: flex; align-items: center; gap: 10px; padding: 11px 14px; border-bottom: 1px solid rgb(255 255 255 / 0.06); font-size: 12.5px; font-weight: 550; }
  .all { display: flex; flex: 1; align-items: center; gap: 10px; }
  .picked { color: var(--text-3); font-size: 11.5px; font-weight: 400; }
  .only { display: flex; align-items: center; gap: 8px; padding: 8px 14px; border-bottom: 1px solid rgb(255 255 255 / 0.06); color: var(--text-2); font-size: 11.8px; }
  .file-list { flex: 1; min-height: 120px; padding: 6px; overflow: auto; display: flex; flex-direction: column; gap: 2px; }
  .file { display: flex; align-items: center; gap: 10px; padding: 6px 10px; border-radius: 8px; transition: background 0.15s ease; }
  .file:hover { background: var(--hover); }
  .file.selected { background: rgb(var(--accent-rgb) / 0.18); }
  .file-name { display: flex; flex: 1; align-items: baseline; gap: 8px; min-width: 0; padding: 2px 0; overflow: hidden; text-align: left; white-space: nowrap; background: none; border: none; cursor: pointer; }
  .name { flex: none; max-width: 100%; overflow: hidden; color: var(--text-1); font-size: 12.8px; font-weight: 500; text-overflow: ellipsis; }
  .dir { min-width: 0; overflow: hidden; color: var(--text-3); font-size: 11px; text-overflow: ellipsis; direction: ltr; text-align: left; opacity: 0.8; }
  .kind { flex: none; padding: 1px 6px; border-radius: 4px; font-family: var(--font-mono); font-size: 10.5px; font-weight: 700; text-align: center; line-height: 1.4; }
  .k-added,
  .k-untracked { background: rgb(111 219 165 / 0.14); color: #6fdba5; }
  .k-modified,
  .k-typeChanged { background: rgb(255 198 107 / 0.14); color: #ffc66b; }
  .k-deleted { background: rgb(255 143 143 / 0.14); color: #ff8f8f; }
  .k-renamed,
  .k-copied { background: rgb(159 180 255 / 0.14); color: #9fb4ff; }
  .k-conflicted { background: rgb(255 107 107 / 0.2); color: #ff6b6b; }
  .composer { display: grid; gap: 10px; padding: 12px; border-top: 1px solid rgb(255 255 255 / 0.07); background: rgb(0 0 0 / 0.1); border-radius: 0 0 12px 12px; }
  textarea { min-height: 78px; resize: vertical; font-size: 12.5px; line-height: 1.45; padding: 10px 12px; border-radius: 8px; }
  .ai-row { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ai-note { overflow: hidden; color: var(--text-3); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .commit-row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .commit-row .btn { height: 36px; font-weight: 550; border-radius: 8px; }
  .diff-head { display: flex; align-items: center; gap: 12px; padding: 9px 14px; border-bottom: 1px solid rgb(255 255 255 / 0.07); background: rgb(0 0 0 / 0.1); border-radius: 12px 12px 0 0; }
  .diff-head code { flex: 1; overflow: hidden; color: var(--text-1); font-family: var(--font-mono); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; padding: 2px 8px; border-radius: 6px; background: rgb(255 255 255 / 0.04); }
  .modes { display: flex; gap: 3px; }
  .modes .icon-btn { width: 28px; height: 28px; }
  .diff-body { flex: 1; min-height: 0; padding: 12px; overflow: auto; }
  .quiet { display: grid; place-items: center; align-content: center; gap: 6px; min-height: 140px; height: 100%; color: var(--text-3); font-size: 12.3px; text-align: center; }
  .clean { color: #7fe0b0; }
  .clean strong { color: var(--text-1); font-size: 13px; }
  .clean span { max-width: 30ch; color: var(--text-3); }
  .error { margin: 8px; color: #ff9d9d; font-size: 12px; }
  .output { position: relative; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 10px; background: rgb(0 0 0 / 0.2); }
  .output-head { display: flex; align-items: center; gap: 7px; width: 100%; padding: 7px 11px; color: var(--text-2); font-size: 12px; }
  .grow { flex: 1; }
  .output:not(.open) :global(.chev) { transform: rotate(-90deg); }
  .output pre { max-height: 180px; margin: 0; padding: 0 12px 10px; overflow: auto; color: var(--text-2); font-family: var(--font-mono); font-size: 11px; line-height: 1.5; white-space: pre-wrap; }
  .close { position: absolute; right: 34px; top: 4px; width: 24px; height: 24px; }
</style>
