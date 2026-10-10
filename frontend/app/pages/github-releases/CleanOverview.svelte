<script lang="ts">
  // Nothing to commit: what the project can do next (build, release, push),
  // and the commits since its last release.
  import { onMount } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import CircleCheckBig from "@lucide/svelte/icons/circle-check-big";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import GitCommitHorizontal from "@lucide/svelte/icons/git-commit-horizontal";
  import Hammer from "@lucide/svelte/icons/hammer";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Rocket from "@lucide/svelte/icons/rocket";
  import Tag from "@lucide/svelte/icons/tag";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf, type RecentCommits } from "./api";
  import GithubMark from "./GithubMark.svelte";
  import { formatRelative, githubReleases as gr, syncText, type ListItem } from "./state.svelte";

  let { item, hidden = 0, onShowHidden }: { item: ListItem; hidden?: number; onShowHidden?: () => void } = $props();

  const entry = $derived(item.entry);
  const status = $derived(item.status);
  const repoId = $derived(item.repo.id);
  const busy = $derived(gr.running(repoId));
  const github = $derived(status?.remote?.owner ? `https://github.com/${status.remote.owner}/${status.remote.repo}` : null);
  const ahead = $derived(status?.branch.ahead ?? 0);
  const behind = $derived(status?.branch.behind ?? 0);

  let recent = $state<RecentCommits | null>(null);
  let recentError = $state<string | null>(null);

  async function load() {
    if (!entry) return;
    try {
      recent = await api.recentCommits(entry.id, 8);
      recentError = null;
    } catch (error) {
      recentError = messageOf(error);
    }
  }

  onMount(() => {
    void load();
  });

  // A commit, push or pull moved HEAD: read the log again.
  let seenHead = "";
  $effect(() => {
    const head = `${status?.branch.head}|${ahead}`;
    if (seenHead && head !== seenHead) void load();
    seenHead = head;
  });

  async function reveal() {
    try {
      if (entry) await api.reveal(entry.id, null);
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  const releaseLabel = $derived(recent && recent.unreleased > 0 ? `Release ${recent.unreleased} ${recent.unreleased === 1 ? "commit" : "commits"}` : "Release");
</script>

<div class="clean">
  <div class="hero">
    <span class="hero-icon"><CircleCheckBig size={22} /></span>
    <div class="hero-text">
      <strong>Nothing to commit</strong>
      <span>
        {#if hidden}
          {hidden} changed {hidden === 1 ? "file is" : "files are"} in other apps of this repository.
          {#if onShowHidden}<button class="link" onclick={onShowHidden}>Show them</button>{/if}
        {:else if status}
          The working tree matches the last commit · {syncText(status)}.
        {:else}
          The working tree matches the last commit.
        {/if}
      </span>
    </div>
  </div>

  <div class="actions" role="group" aria-label="Next steps">
    {#if behind > 0}
      <button class="action accent" disabled={!!busy} onclick={() => void gr.pull(repoId)}>
        {#if busy === "pull"}<LoaderCircle size={16} class="spin" />{:else}<ArrowDown size={16} />{/if}
        <span><strong>Pull {behind}</strong><small>Newer commits on GitHub</small></span>
      </button>
    {:else if ahead > 0}
      <button class="action accent" disabled={!!busy} onclick={() => void gr.push(repoId)}>
        {#if busy === "push"}<LoaderCircle size={16} class="spin" />{:else}<ArrowUp size={16} />{/if}
        <span><strong>Push {ahead}</strong><small>{ahead === 1 ? "A commit" : "Commits"} not on GitHub yet</small></span>
      </button>
    {/if}
    {#if entry}
      <button class="action" onclick={() => gr.setTab("build")} title="Build tab (Alt+2)">
        <Hammer size={16} />
        <span><strong>Build</strong><small>Run the build with a live log</small></span>
      </button>
      <button class="action" class:accent={!ahead && !behind && !!recent?.unreleased} onclick={() => gr.setTab("release")} title="Release tab (Alt+3)">
        <Rocket size={16} />
        <span><strong>{releaseLabel}</strong><small>{recent?.lastTag ? `Since ${recent.lastTag}` : "First release"}</small></span>
      </button>
      <button class="action" onclick={() => void reveal()}>
        <FolderOpen size={16} />
        <span><strong>Open folder</strong><small>In File Explorer</small></span>
      </button>
    {/if}
    {#if github}
      <button class="action" onclick={() => void openUrl(github)}>
        <GithubMark size={16} />
        <span><strong>Open on GitHub</strong><small>{status?.remote?.owner}/{status?.remote?.repo}</small></span>
      </button>
    {/if}
  </div>

  {#if entry}
    <section class="commits" aria-label="Recent commits">
      <header>
        <h3>
          {#if recent?.unreleased}
            {recent.unreleased} {recent.unreleased === 1 ? "commit" : "commits"} since {recent.lastTag ?? "the start"}
          {:else}
            Recent commits
          {/if}
        </h3>
        {#if recent && recent.lastTag && !recent.unreleased}<span class="released">Everything is released in {recent.lastTag}</span>{/if}
      </header>
      {#if recentError}
        <p class="error">{recentError}</p>
      {:else if !recent}
        <div class="quiet"><LoaderCircle size={15} class="spin" /></div>
      {:else}
        <ol>
          {#each recent.commits as commit, i (commit.sha)}
            {#if recent.lastTag && recent.unreleased > 0 && i === recent.unreleased}
              <li class="divider" aria-label="Released in {recent.lastTag}"><Tag size={11} /> {recent.lastTag}</li>
            {/if}
            <li class:unreleased={i < recent.unreleased}>
              <span class="dot" aria-hidden="true"><GitCommitHorizontal size={14} /></span>
              <span class="subject" title={commit.subject}>{commit.subject}</span>
              {#if i < ahead}<span class="pill">not pushed</span>{/if}
              <span class="meta">{commit.author} · {formatRelative(commit.date)}</span>
              {#if github}
                <button class="sha" title="Open this commit on GitHub" onclick={() => void openUrl(`${github}/commit/${commit.sha}`)}>
                  {commit.sha.slice(0, 7)} <ArrowUpRight size={11} />
                </button>
              {:else}
                <code class="sha">{commit.sha.slice(0, 7)}</code>
              {/if}
            </li>
          {:else}
            <li class="none">No commits yet.</li>
          {/each}
        </ol>
      {/if}
    </section>
  {/if}

  <p class="keys" aria-label="Keyboard shortcuts">
    <span><kbd>Alt</kbd>+<kbd>1</kbd>–<kbd>4</kbd> tabs</span>
    <span><kbd>Alt</kbd>+<kbd>R</kbd> refresh</span>
    <span><kbd>/</kbd> search projects</span>
  </p>
</div>

<style>
  .clean { display: flex; flex: 1; flex-direction: column; gap: 14px; min-height: 0; overflow: auto; }

  .hero,
  .actions,
  .commits,
  .keys { flex: none; }

  .hero { display: flex; align-items: center; gap: 12px; padding: 4px 2px 0; }

  .hero-icon { display: grid; flex: none; place-items: center; width: 42px; height: 42px; border-radius: 12px; background: rgb(62 207 142 / 0.12); color: #6fdba5; }

  .hero-text { display: grid; gap: 2px; min-width: 0; }

  .hero-text strong { font-size: 14px; }

  .hero-text span { color: var(--text-3); font-size: 12px; }

  .link { padding: 0; color: #b7befa; font-size: inherit; text-decoration: underline; text-underline-offset: 2px; }

  .actions { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 8px; }

  .action { display: flex; align-items: center; gap: 11px; min-width: 0; padding: 11px 13px; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 11px; background: rgb(255 255 255 / 0.03); color: var(--text-2); text-align: left; transition: background var(--dur-fast), border-color var(--dur-fast); }

  .action:hover:not(:disabled) { border-color: rgb(255 255 255 / 0.13); background: var(--hover); color: var(--text-1); }

  .action.accent { border-color: rgb(var(--accent-rgb) / 0.35); background: rgb(var(--accent-rgb) / 0.1); color: #c9cffb; }

  .action > span { display: grid; gap: 1px; min-width: 0; }

  .action strong { color: var(--text-1); font-size: 12.8px; font-weight: 600; }

  .action small { overflow: hidden; color: var(--text-3); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }

  .commits { display: flex; flex-direction: column; min-height: 0; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 12px; background: rgb(0 0 0 / 0.14); }

  .commits header { display: flex; align-items: baseline; gap: 10px; padding: 10px 14px; border-bottom: 1px solid rgb(255 255 255 / 0.06); }

  h3 { margin: 0; font-size: 12.8px; font-weight: 620; }

  .released { color: #7fe0b0; font-size: 11.5px; }

  ol { margin: 0; padding: 4px; list-style: none; }

  li { display: flex; align-items: center; gap: 9px; min-width: 0; padding: 6px 10px; border-radius: 8px; font-size: 12.3px; }

  li:hover { background: var(--hover); }

  .dot { display: inline-flex; flex: none; color: var(--text-3); }

  li.unreleased .dot { color: #a9b3ff; }

  .subject { flex: 1; min-width: 0; overflow: hidden; color: var(--text-1); text-overflow: ellipsis; white-space: nowrap; }

  .pill { flex: none; padding: 0 7px; border-radius: 999px; background: rgb(255 196 92 / 0.14); color: #ffd08a; font-size: 10.5px; line-height: 17px; }

  .meta { flex: none; color: var(--text-3); font-size: 11px; }

  .sha { display: inline-flex; flex: none; align-items: center; gap: 3px; padding: 1px 6px; border-radius: 6px; color: var(--text-3); font-family: var(--font-mono); font-size: 11px; }

  button.sha:hover { background: rgb(255 255 255 / 0.06); color: var(--text-1); }

  .none { color: var(--text-3); }

  li.divider { gap: 6px; padding: 4px 10px; color: var(--text-3); font-family: var(--font-mono); font-size: 10.5px; }

  li.divider::after { flex: 1; height: 1px; background: rgb(255 255 255 / 0.07); content: ""; }

  li.divider:hover { background: none; }

  li.unreleased .subject { font-weight: 500; }

  .quiet { display: grid; place-items: center; min-height: 80px; color: var(--text-3); }

  .error { margin: 10px 14px; color: #ff9d9d; font-size: 12px; }

  .keys { display: flex; flex-wrap: wrap; gap: 14px; margin: auto 0 0; padding-top: 4px; color: var(--text-3); font-size: 11px; }

  kbd { padding: 0 5px; border: 1px solid rgb(255 255 255 / 0.12); border-bottom-width: 2px; border-radius: 5px; background: rgb(255 255 255 / 0.04); color: var(--text-2); font-family: var(--font-mono); font-size: 10.5px; }

  @container changes (max-width: 620px) {
    .action {
      padding: 9px 11px;
    }

    .action small {
      display: none;
    }
  }

  @container changes (max-width: 560px) {
    .meta {
      display: none;
    }
  }
</style>
