<script lang="ts">
  // One project: where it stands (GitHub, branch, versions, last release,
  // CI) and its tabs.
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import FileDiff from "@lucide/svelte/icons/file-diff";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Hammer from "@lucide/svelte/icons/hammer";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Rocket from "@lucide/svelte/icons/rocket";
  import Tag from "@lucide/svelte/icons/tag";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Popover from "../../../lib/components/Popover.svelte";
  import { confirm } from "../../../lib/confirm.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf } from "./api";
  import BuildTab from "./BuildTab.svelte";
  import ChangesTab from "./ChangesTab.svelte";
  import GithubMark from "./GithubMark.svelte";
  import HistoryTab from "./HistoryTab.svelte";
  import ReleaseTab from "./ReleaseTab.svelte";
  import { buildKindLabels, formatRelative, githubReleases as gr, runState, syncText, type ListItem, type Tab } from "./state.svelte";

  let { item }: { item: ListItem } = $props();

  const status = $derived(item.status);
  const entry = $derived(item.entry);
  const repoId = $derived(item.repo.id);
  const remote = $derived(gr.remotes[repoId] ?? null);
  const github = $derived(status?.remote?.owner ? `https://github.com/${status.remote.owner}/${status.remote.repo}` : null);
  const lastRelease = $derived(entry ? (remote?.lastRelease[entry.id] ?? null) : null);
  const run = $derived(remote?.run ?? null);
  const ci = $derived(runState(run));
  const busy = $derived(gr.running(repoId));
  let menuOpen = $state(false);

  const tabs: { id: Tab; label: string; icon: typeof Hammer }[] = [
    { id: "changes", label: "Changes", icon: FileDiff },
    { id: "build", label: "Build", icon: Hammer },
    { id: "release", label: "Release", icon: Rocket },
    { id: "history", label: "Releases", icon: ListTree },
  ];

  async function remove() {
    menuOpen = false;
    const ok = await confirm({
      title: `Remove ${item.repo.name}?`,
      message: "It leaves this list only: the folder, its git history and its GitHub releases stay as they are.",
      confirmLabel: "Remove",
      danger: true,
    });
    if (!ok) return;
    try {
      await gr.removeRepo(repoId);
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  async function reveal() {
    menuOpen = false;
    try {
      if (entry) await api.reveal(entry.id, null);
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  const changeCount = $derived(entry?.monorepo ? entry.changes : (status?.changes ?? 0));
</script>

<button class="back" onclick={() => (gr.showList = true)}><ArrowLeft size={15} /> All projects</button>

<header class="head">
  <div class="title">
    <h2>
      {entry?.name ?? item.repo.name}
      {#if entry?.monorepo}<span class="in">in {item.repo.name}</span>{/if}
    </h2>
    <span class="path selectable" title={entry?.dir ?? item.repo.path}>{entry?.dir ?? item.repo.path}</span>
  </div>
  <div class="actions">
    {#if status && !status.problem}
      {#if status.branch.behind > 0}
        <button class="btn small primary" disabled={!!busy} onclick={() => void gr.pull(repoId)}>
          {#if busy === "pull"}<LoaderCircle size={13} class="spin" />{:else}<ArrowDown size={13} />{/if} Pull {status.branch.behind}
        </button>
      {/if}
      {#if status.branch.ahead > 0 || (!status.branch.upstream && status.branch.head)}
        <button class="btn small" disabled={!!busy || status.branch.behind > 0} onclick={() => void gr.push(repoId)} title={status.branch.behind > 0 ? "Pull first: GitHub has newer commits" : "Push your commits to GitHub"}>
          {#if busy === "push"}<LoaderCircle size={13} class="spin" />{:else}<ArrowUp size={13} />{/if} Push{status.branch.ahead ? ` ${status.branch.ahead}` : ""}
        </button>
      {/if}
    {/if}
    <button class="icon-btn" title="Fetch from GitHub and refresh" aria-label="Refresh" disabled={!!busy} onclick={() => void gr.fetch(repoId)}>
      <RefreshCw size={15} class={busy === "fetch" || gr.refreshing[repoId] ? "spin" : ""} />
    </button>
    <Popover bind:open={menuOpen} align="end">
      {#snippet trigger({ toggle, open })}
        <button class="icon-btn" aria-label="More" aria-haspopup="menu" aria-expanded={open} onclick={toggle}><Ellipsis size={16} /></button>
      {/snippet}
      {#snippet children()}
      <div class="menu">
        {#if entry}<button class="menu-item" onclick={() => void reveal()}><FolderOpen size={14} /> Open the folder</button>{/if}
        {#if github}<button class="menu-item" onclick={() => ((menuOpen = false), void openUrl(github))}><GithubMark size={14} /> Open on GitHub</button>{/if}
        <button class="menu-item danger" onclick={() => void remove()}><Trash2 size={14} /> Remove from the list</button>
      </div>
      {/snippet}
    </Popover>
  </div>
</header>

{#if !status}
  <div class="state"><LoaderCircle size={18} class="spin" /> Reading the repository…</div>
{:else if status.problem}
  <div class="state problem">
    <CircleAlert size={18} />
    <span>{status.problem}</span>
    <button class="btn small" onclick={() => void gr.refresh(repoId)}>Check again</button>
  </div>
{:else}
  <div class="facts">
    {#if github && status.remote}
      <button class="fact link" onclick={() => void openUrl(github)} title="Open on GitHub">
        <GithubMark size={13} /> {status.remote.owner}/{status.remote.repo}
      </button>
    {:else}
      <span class="fact warn"><CircleAlert size={13} /> No GitHub remote</span>
    {/if}
    <span class="fact" title={status.branch.upstream ? `Tracks ${status.branch.upstream}` : "This branch isn't on GitHub yet"}>
      <GitBranch size={13} /> {status.branch.branch ?? "detached HEAD"}
      <em class:attention={status.branch.behind > 0}>{syncText(status)}</em>
    </span>
    <span class="fact" class:attention={changeCount > 0}>
      <FileDiff size={13} /> {changeCount ? `${changeCount} changed ${changeCount === 1 ? "file" : "files"}` : "No changes"}
    </span>
    {#if entry}
      {#if entry.versions.files.length}
        <Popover align="start">
          {#snippet trigger({ toggle })}
            <button class="fact link" class:warn={entry.versions.mismatched.length > 0} onclick={toggle}>
              {#if entry.versions.mismatched.length}<CircleAlert size={13} />{/if}
              v{entry.versions.current ?? "?"}
              <em>{entry.versions.mismatched.length ? "files disagree" : `${entry.versions.files.filter((f) => !f.skipped).length} version ${entry.versions.files.length === 1 ? "file" : "files"}`}</em>
            </button>
          {/snippet}
          {#snippet children()}
          <div class="menu versions">
            <div class="menu-label">Version files</div>
            {#each entry.versions.files as file (file.path)}
              <div class="vrow" class:off={file.skipped} class:bad={entry.versions.mismatched.includes(file.path)}>
                <code>{file.path}</code><span>{file.skipped ? "skipped" : file.version}</span>
              </div>
            {/each}
            {#if entry.versions.mismatched.length}
              <p>A release sets them all to the new version.</p>
            {/if}
          </div>
          {/snippet}
        </Popover>
      {:else}
        <span class="fact"><CircleAlert size={13} /> No version file</span>
      {/if}
      <span class="fact" title={entry.lastTag?.legacy ? "A shared tag from before this app had its own" : "The newest tag of this project"}>
        <Tag size={13} />
        {#if lastRelease}
          {lastRelease.tagName} <em>{formatRelative(lastRelease.publishedAt ?? lastRelease.createdAt)}</em>
        {:else if entry.lastTag}
          {entry.lastTag.name} <em>{formatRelative(entry.lastTag.date)}</em>
        {:else}
          Not released yet
        {/if}
      </span>
    {/if}
    {#if run}
      <button class="fact link ci-{ci}" onclick={() => void openUrl(run.htmlUrl)} title={run.displayTitle ?? run.name ?? "Last Actions run"}>
        {#if ci === "running"}<LoaderCircle size={13} class="spin" />{:else if ci === "ok"}<CircleCheck size={13} />{:else}<CircleX size={13} />{/if}
        {run.name ?? "CI"} <em>{ci === "running" ? "running" : ci === "ok" ? "passed" : ci === "cancelled" ? "cancelled" : "failed"} · {formatRelative(run.updatedAt ?? run.createdAt)}</em>
      </button>
    {/if}
    {#if entry?.buildKinds.length}
      <span class="kinds">{#each entry.buildKinds as kind (kind)}<span>{buildKindLabels[kind]}</span>{/each}</span>
    {/if}
  </div>

  <nav class="tabs" aria-label="Project">
    {#each tabs as tab (tab.id)}
      <button class="tab" class:active={gr.tab === tab.id} aria-current={gr.tab === tab.id ? "page" : undefined} onclick={() => gr.setTab(tab.id)}>
        <tab.icon size={14} /> {tab.label}
        {#if tab.id === "changes" && changeCount}<span class="badge">{changeCount}</span>{/if}
        {#if tab.id === "build" && entry && gr.builds.get(entry.id)?.running}<LoaderCircle size={12} class="spin" />{/if}
        {#if tab.id === "release" && entry && gr.releases.get(entry.id)?.running}<LoaderCircle size={12} class="spin" />{/if}
      </button>
    {/each}
  </nav>

  <div class="body">
    {#if gr.tab === "changes"}
      <ChangesTab {item} />
    {:else if !entry}
      <div class="state">This repository has no project to build.</div>
    {:else if gr.tab === "build"}
      <BuildTab {item} {entry} />
    {:else if gr.tab === "release"}
      <ReleaseTab {item} {entry} />
    {:else}
      <HistoryTab {item} {entry} />
    {/if}
  </div>
{/if}

<style>
  .back {
    display: none;
    align-items: center;
    gap: 7px;
    align-self: flex-start;
    margin: 12px 0 0 14px;
    padding: 6px 10px;
    border-radius: 8px;
    color: var(--text-2);
    font-size: 12.5px;
  }

  .back:hover {
    background: var(--hover);
  }

  @container releases (max-width: 820px) {
    .back {
      display: flex;
    }
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px 18px 10px;
  }

  .title {
    flex: 1;
    min-width: 0;
  }

  h2 {
    margin: 0 0 3px;
    font-family: var(--font-display);
    font-size: 19px;
    font-weight: 620;
  }

  .in {
    margin-left: 4px;
    color: var(--text-3);
    font-size: 13px;
    font-weight: 450;
  }

  .path {
    display: block;
    overflow: hidden;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
  }

  .actions .icon-btn {
    width: 32px;
    height: 32px;
  }

  .menu {
    min-width: 210px;
    padding: 5px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }

  .menu-item.danger {
    color: #ff9d9d;
  }

  .versions {
    min-width: 280px;
    padding: 8px;
  }

  .versions p {
    margin: 6px 4px 2px;
    color: var(--text-3);
    font-size: 11.5px;
  }

  .vrow {
    display: flex;
    justify-content: space-between;
    gap: 14px;
    padding: 4px 6px;
    border-radius: 6px;
    font-size: 12px;
  }

  .vrow code {
    color: var(--text-2);
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .vrow.bad {
    background: rgb(255 180 84 / 0.1);
    color: #ffd08a;
  }

  .vrow.off {
    opacity: 0.5;
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 18px 12px;
  }

  .fact {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 9px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 7px;
    background: rgb(255 255 255 / 0.035);
    color: var(--text-1);
    font-size: 11.8px;
    white-space: nowrap;
  }

  .fact em {
    color: var(--text-3);
    font-style: normal;
  }

  .fact.link:hover {
    background: var(--hover);
  }

  .fact.warn,
  .fact .attention,
  .fact.attention {
    color: #ffd08a;
  }

  .fact.ci-ok {
    color: #7fe0b0;
  }

  .fact.ci-failed {
    color: #ff9d9d;
  }

  .fact.ci-running {
    color: #9fd3ff;
  }

  .kinds {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .kinds span {
    padding: 0 7px;
    border-radius: 6px;
    background: rgb(var(--accent-rgb) / 0.12);
    color: #c3c9f7;
    font-size: 10.5px;
    line-height: 20px;
  }

  .tabs {
    display: flex;
    gap: 2px;
    padding: 0 12px;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin-bottom: -1px;
    padding: 9px 12px;
    border-bottom: 2px solid transparent;
    color: var(--text-2);
    font-size: 12.8px;
  }

  .tab:hover {
    color: var(--text-1);
  }

  .tab.active {
    border-bottom-color: var(--accent);
    color: var(--text-1);
  }

  .badge {
    min-width: 18px;
    padding: 0 5px;
    border-radius: 999px;
    background: rgb(255 196 92 / 0.16);
    color: #ffd08a;
    font-size: 10.5px;
    line-height: 17px;
    text-align: center;
  }

  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 18px;
    color: var(--text-2);
    font-size: 12.5px;
  }

  .state.problem {
    padding: 12px 14px;
    border: 1px solid rgb(255 180 84 / 0.3);
    border-radius: 12px;
    background: rgb(255 180 84 / 0.08);
    color: #ffd08a;
  }

  .state.problem span {
    flex: 1;
    color: var(--text-2);
  }
</style>
