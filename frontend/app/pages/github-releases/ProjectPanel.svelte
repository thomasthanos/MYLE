<script lang="ts">
  // One project: where it stands (GitHub, branch, versions, last release,
  // CI) and its tabs.
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleSlash from "@lucide/svelte/icons/circle-slash";
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
  import Square from "@lucide/svelte/icons/square";
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
  let cancellingCi = $state(false);
  // Poll remote while CI is running so the status updates to passed/failed/cancelled
  // automatically as soon as the run finishes on GitHub.
  $effect(() => {
    if (ci === "running" && repoId) {
      const interval = setInterval(() => {
        void gr.loadRemote(repoId);
      }, 10_000);
      return () => clearInterval(interval);
    }
  });
  async function cancelCiRun() {
    if (!run || cancellingCi) return;
    const ok = await confirm({
      title: `Cancel ${run.name ?? "CI"} run?`,
      message: `Stop the GitHub Actions workflow run #${run.id} currently running on GitHub?`,
      confirmLabel: "Cancel run",
      danger: true,
    });
    if (!ok) return;
    cancellingCi = true;
    try {
      await api.cancelCi(repoId, run.id);
      toast.info(`Cancellation requested for ${run.name ?? "CI"} run.`);
      await gr.loadRemote(repoId);
    } catch (error) {
      toast.error(messageOf(error));
    } finally {
      cancellingCi = false;
    }
  }
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
  const versionFiles = $derived(entry ? entry.versions.files.filter((f) => !f.skipped).length : 0);
  const mismatched = $derived((entry?.versions.mismatched.length ?? 0) > 0);
  /** Alt+1…4 switch the tabs, Alt+R fetches and refreshes. */
  function onKeydown(event: KeyboardEvent) {
    if (!event.altKey || event.ctrlKey || event.metaKey || gr.settingsOpen || gr.found || event.defaultPrevented) return;
    const at = ["1", "2", "3", "4"].indexOf(event.key);
    if (at >= 0) {
      event.preventDefault();
      gr.setTab(tabs[at].id);
    } else if (event.key.toLowerCase() === "r" && !busy) {
      event.preventDefault();
      void gr.fetch(repoId);
    }
  }
</script>
<svelte:window onkeydown={onKeydown} />
<button class="back" onclick={() => (gr.showList = true)}><ArrowLeft size={15} /> All projects</button>
<header class="head">
  <span class="avatar" aria-hidden="true">{(entry?.name ?? item.repo.name).slice(0, 1).toUpperCase()}</span>
  <div class="title">
    <div class="name-row">
      <h2>{entry?.name ?? item.repo.name}</h2>
      {#if entry?.versions.files.length}
        <Popover align="start">
          {#snippet trigger({ toggle })}
            <button class="version" class:warn={mismatched} onclick={toggle} title={mismatched ? "The version files disagree" : `${versionFiles} version ${versionFiles === 1 ? "file" : "files"}`}>
              {#if mismatched}<CircleAlert size={12} />{/if}v{entry.versions.current ?? "?"}
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
            {#if mismatched}
              <p>A release sets them all to the new version.</p>
            {/if}
          </div>
          {/snippet}
        </Popover>
      {:else if entry}
        <span class="version muted" title="No version file found">no version</span>
      {/if}
      {#if entry?.monorepo}<span class="in">in {item.repo.name}</span>{/if}
    </div>
    <div class="where">
      {#if github && status?.remote}
        <button class="repo" onclick={() => void openUrl(github)} title="Open on GitHub"><GithubMark size={12} /> {status.remote.owner}/{status.remote.repo}</button>
        <span class="sep" aria-hidden="true">·</span>
      {/if}
      <button class="path" title="Open the folder: {entry?.dir ?? item.repo.path}" onclick={() => void reveal()}>{entry?.dir ?? item.repo.path}</button>
    </div>
  </div>
  {#if entry && status && !status.problem}
    <div class="last" title={entry.lastTag?.legacy ? "A shared tag from before this app had its own" : "The newest release of this project"}>
      <small>Last release</small>
      {#if lastRelease}
        <strong><Tag size={12} /> {lastRelease.tagName}</strong>
        <em>{formatRelative(lastRelease.publishedAt ?? lastRelease.createdAt)}</em>
      {:else if entry.lastTag}
        <strong><Tag size={12} /> {entry.lastTag.name}</strong>
        <em>{formatRelative(entry.lastTag.date)}</em>
      {:else}
        <strong class="none">Not released yet</strong>
      {/if}
    </div>
  {/if}
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
    <button class="icon-btn" title="Fetch from GitHub and refresh (Alt+R)" aria-label="Refresh" disabled={!!busy} onclick={() => void gr.fetch(repoId)}>
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
    {#if !github}
      <span class="fact warn"><CircleAlert size={13} /> No GitHub remote</span>
    {/if}
    <span class="fact" title={status.branch.upstream ? `Tracks ${status.branch.upstream}` : "This branch isn't on GitHub yet"}>
      <GitBranch size={13} /> {status.branch.branch ?? "detached HEAD"}
      <em class:attention={status.branch.behind > 0}>{syncText(status)}</em>
    </span>
    <span class="fact" class:attention={changeCount > 0}>
      <FileDiff size={13} /> {changeCount ? `${changeCount} changed ${changeCount === 1 ? "file" : "files"}` : "No changes"}
    </span>
    {#if run}
      <div class="fact ci-wrapper ci-{ci}">
        <button class="ci-btn" onclick={() => void openUrl(run.htmlUrl)} title="{run.displayTitle ?? run.name ?? 'Last Actions run'} (open in GitHub)">
          {#if ci === "running"}<LoaderCircle size={13} class="spin" />{:else if ci === "ok"}<CircleCheck size={13} />{:else if ci === "cancelled"}<CircleSlash size={13} />{:else}<CircleX size={13} />{/if}
          {run.name ?? "CI"} <em>{ci === "running" ? "running" : ci === "ok" ? "passed" : ci === "cancelled" ? "cancelled" : "failed"} · {formatRelative(run.updatedAt ?? run.createdAt)}</em>
        </button>
        {#if ci === "running"}
          <button
            class="ci-cancel-btn"
            title="Cancel this CI run on GitHub"
            disabled={cancellingCi}
            onclick={(e) => { e.stopPropagation(); void cancelCiRun(); }}
          >
            {#if cancellingCi}<LoaderCircle size={10} class="spin" />{:else}<Square size={9} />{/if}
            <span>Cancel</span>
          </button>
        {/if}
      </div>
    {/if}
    {#if entry?.buildKinds.length}
      <span class="kinds">{#each entry.buildKinds as kind (kind)}<span>{buildKindLabels[kind]}</span>{/each}</span>
    {/if}
  </div>
  <nav class="tabs" aria-label="Project">
    {#each tabs as tab, i (tab.id)}
      <button class="tab" class:active={gr.tab === tab.id} aria-current={gr.tab === tab.id ? "page" : undefined} title="{tab.label} (Alt+{i + 1})" onclick={() => gr.setTab(tab.id)}>
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
  .back { display: none; align-items: center; gap: 7px; align-self: flex-start; margin: 12px 0 0 14px; padding: 6px 10px; border-radius: 8px; color: var(--text-2); font-size: 12.5px; }
  .back:hover { background: var(--hover); }
  @container releases (max-width: 820px) {
    .back {
      display: flex;
    }
  }
  .head { display: flex; align-items: center; gap: 14px; padding: 16px 18px 12px; }
  .avatar { display: grid; flex: none; place-items: center; width: 44px; height: 44px; border: 1px solid rgb(var(--accent-rgb) / 0.3); border-radius: 13px; background: linear-gradient(145deg, rgb(var(--accent-rgb) / 0.28), rgb(62 207 142 / 0.1)); color: #dfe3ff; font-family: var(--font-display); font-size: 19px; font-weight: 650; }
  .title { display: grid; flex: 1; gap: 4px; min-width: 0; }
  .name-row { display: flex; align-items: center; gap: 8px; min-width: 0; }
  h2 { margin: 0; overflow: hidden; font-family: var(--font-display); font-size: 20px; font-weight: 640; text-overflow: ellipsis; white-space: nowrap; }
  .version { display: inline-flex; flex: none; align-items: center; gap: 4px; padding: 1px 8px; border: 1px solid rgb(var(--accent-rgb) / 0.3); border-radius: 999px; background: rgb(var(--accent-rgb) / 0.12); color: #c9cffb; font-family: var(--font-mono); font-size: 11.5px; line-height: 19px; }
  button.version:hover { background: rgb(var(--accent-rgb) / 0.2); }
  .version.warn { border-color: rgb(255 180 84 / 0.35); background: rgb(255 180 84 / 0.1); color: #ffd08a; }
  .version.muted { border-color: rgb(255 255 255 / 0.08); background: none; color: var(--text-3); }
  .in { flex: none; color: var(--text-3); font-size: 12.5px; }
  .where { display: flex; align-items: center; gap: 7px; min-width: 0; font-size: 11.5px; }
  .repo { display: inline-flex; flex: none; align-items: center; gap: 5px; padding: 0; color: #b7befa; font-size: 12px; }
  .repo:hover { text-decoration: underline; text-underline-offset: 2px; }
  .sep { color: var(--text-3); }
  .path { min-width: 0; padding: 0; overflow: hidden; color: var(--text-3); font-family: var(--font-mono); font-size: 11px; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .path:hover { color: var(--text-2); }
  .last { display: grid; flex: none; justify-items: end; gap: 1px; padding: 4px 12px; border-right: 1px solid rgb(255 255 255 / 0.07); }
  .last small { color: var(--text-3); font-size: 10.5px; letter-spacing: 0.04em; text-transform: uppercase; }
  .last strong { display: inline-flex; align-items: center; gap: 5px; font-family: var(--font-mono); font-size: 13px; font-weight: 600; }
  .last strong.none { color: var(--text-2); font-family: inherit; font-size: 12.5px; font-weight: 500; }
  .last em { color: var(--text-3); font-size: 11px; font-style: normal; }
  @container releases (max-width: 760px) {
    .last {
      display: none;
    }
  }
  @container releases (max-width: 1100px) {
    .facts .kinds {
      display: none;
    }
  }
  .actions { display: flex; flex: none; align-items: center; gap: 6px; }
  .actions .icon-btn { width: 32px; height: 32px; }
  .menu { min-width: 210px; padding: 5px; }
  .menu-item { display: flex; align-items: center; gap: 8px; width: 100%; }
  .menu-item.danger { color: #ff9d9d; }
  .versions { min-width: 280px; padding: 8px; }
  .versions p { margin: 6px 4px 2px; color: var(--text-3); font-size: 11.5px; }
  .vrow { display: flex; justify-content: space-between; gap: 14px; padding: 4px 6px; border-radius: 6px; font-size: 12px; }
  .vrow code { color: var(--text-2); font-family: var(--font-mono); font-size: 11px; }
  .vrow.bad { background: rgb(255 180 84 / 0.1); color: #ffd08a; }
  .vrow.off { opacity: 0.5; }
  .facts { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin: 0 18px 12px; padding: 4px 6px; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 10px; background: rgb(0 0 0 / 0.16); }
  .fact { display: inline-flex; align-items: center; gap: 7px; height: 28px; padding: 0 10px; border-radius: 7px; color: var(--text-1); font-size: 12px; white-space: nowrap; }
  .fact + .fact { box-shadow: -1px 0 0 rgb(255 255 255 / 0.06); }
  .fact em { color: var(--text-3); font-style: normal; }
  .fact.link:hover { background: var(--hover); }
  .fact.warn,
  .fact .attention,
  .fact.attention { color: #ffd08a; }
  .fact.ci-ok { color: #7fe0b0; }
  .fact.ci-failed { color: #ff9d9d; }
  .fact.ci-cancelled { color: var(--text-3); }
  .fact.ci-running { color: #9fd3ff; }
  .fact.ci-wrapper { padding: 0 4px 0 10px; gap: 8px; }
  .ci-btn { display: inline-flex; align-items: center; gap: 6px; height: 100%; color: inherit; background: none; border: none; padding: 0; cursor: pointer; font: inherit; }
  .ci-btn:hover { text-decoration: underline; }
  .ci-cancel-btn { display: inline-flex; align-items: center; gap: 3px; height: 20px; padding: 0 7px; border-radius: 5px; background: rgb(255 100 100 / 0.15); border: 1px solid rgb(255 100 100 / 0.3); color: #ff9d9d; font-size: 10.5px; font-weight: 550; cursor: pointer; line-height: 1; transition: all 0.15s ease; }
  .ci-cancel-btn:hover:not(:disabled) { background: rgb(255 100 100 / 0.28); border-color: rgb(255 100 100 / 0.5); color: #ffbebe; }
  .ci-cancel-btn:disabled { opacity: 0.5; cursor: default; }
  .kinds { display: inline-flex; align-items: center; gap: 5px; margin-left: auto; padding-right: 4px; }
  .kinds span { padding: 0 8px; border-radius: 6px; background: rgb(var(--accent-rgb) / 0.12); color: #c3c9f7; font-size: 11px; line-height: 21px; }
  .tabs { display: flex; gap: 4px; padding: 0 18px; border-bottom: 1px solid rgb(255 255 255 / 0.07); }
  .tab { display: inline-flex; align-items: center; gap: 8px; margin-bottom: -1px; padding: 10px 14px; border-bottom: 2px solid transparent; color: var(--text-2); font-size: 13px; transition: all 0.15s ease; }
  .tab:hover { color: var(--text-1); }
  .tab.active { border-bottom-color: var(--accent); color: var(--text-1); }
  .badge { min-width: 18px; padding: 0 5px; border-radius: 999px; background: rgb(255 196 92 / 0.16); color: #ffd08a; font-size: 10.5px; line-height: 17px; text-align: center; }
  .body { display: flex; flex: 1; flex-direction: column; min-height: 0; }
  .state { display: flex; align-items: center; gap: 10px; margin: 18px; color: var(--text-2); font-size: 12.5px; }
  .state.problem { padding: 12px 14px; border: 1px solid rgb(255 180 84 / 0.3); border-radius: 12px; background: rgb(255 180 84 / 0.08); color: #ffd08a; }
  .state.problem span { flex: 1; color: var(--text-2); }
</style>
