<script lang="ts">
  // GitHub Releases: your repositories, their changes, builds and releases.
  import { onMount } from "svelte";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Download from "@lucide/svelte/icons/download";
  import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import Info from "@lucide/svelte/icons/info";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Search from "@lucide/svelte/icons/search";
  import Settings from "@lucide/svelte/icons/settings";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import X from "@lucide/svelte/icons/x";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf } from "./api";
  import FoundDialog from "./FoundDialog.svelte";
  import GithubMark from "./GithubMark.svelte";
  import ProjectPanel from "./ProjectPanel.svelte";
  import SettingsDialog from "./SettingsDialog.svelte";
  import { buildKindLabels, githubReleases as gr, installTool, type ListItem } from "./state.svelte";

  let searchInput = $state<HTMLInputElement>();

  onMount(() => {
    void gr.init();
    const timer = setInterval(() => (gr.clock = Date.now()), 30_000);
    // Back from the editor or a terminal: the files may have changed.
    const onFocus = () => {
      if (document.visibilityState === "visible") void gr.refreshSelected(false);
    };
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  });

  /** "/" or Ctrl+F jumps to the search; Escape clears it. */
  function onKeydown(event: KeyboardEvent) {
    if (gr.settingsOpen || gr.found || event.defaultPrevented) return;
    const target = event.target as HTMLElement | null;
    const typing = !!target?.closest("input, textarea, select, [contenteditable='true']");
    if ((event.key === "/" && !typing) || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f")) {
      if (!searchInput) return;
      event.preventDefault();
      gr.showList = true;
      searchInput.focus();
      searchInput.select();
    } else if (event.key === "Escape" && target === searchInput && gr.query) {
      event.preventDefault();
      gr.query = "";
    }
  }

  const account = $derived(gr.page?.account ?? null);
  const gbr = $derived(gr.page?.gbrImport ?? null);

  async function dismissImport() {
    try {
      await api.dismissImport();
      if (gr.page) gr.page.gbrImport = null;
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  function subtitle(item: ListItem): string {
    const status = item.status;
    if (!status) return item.repo.path;
    if (status.problem) return status.problem;
    const remote = status.remote?.owner ? `${status.remote.owner}/${status.remote.repo}` : "No GitHub remote";
    if (item.entry?.monorepo) return `${item.repo.name} · ${remote}`;
    return remote;
  }

  const showBanner = $derived(!!gr.page && (!gr.page.git || !account || (gbr && !gbr.dismissed)));
</script>

<svelte:window onkeydown={onKeydown} />

<PageHeader title="GitHub Releases" subtitle="Commit, build and release your projects to GitHub, with AI help for the words." />

{#if gr.loading && !gr.page}
  <div class="loading"><LoaderCircle size={20} class="spin" /></div>
{:else if gr.error && !gr.page}
  <div class="loading">
    <CircleAlert size={22} />
    <span>{gr.error}</span>
    <button class="btn small" onclick={() => void gr.init()}>Try again</button>
  </div>
{:else if gr.page}
  <div class="releases">
    <div class="toolbar">
      <label class="search">
        <Search size={15} aria-hidden="true" />
        <input bind:this={searchInput} bind:value={gr.query} placeholder="Search projects  ( / )" aria-label="Search projects" />
        {#if gr.query}
          <button class="icon-btn clear" aria-label="Clear the search" onclick={() => (gr.query = "")}><X size={13} /></button>
        {/if}
      </label>
      <button class="btn" onclick={() => void gr.addFolder()} title="Add a git repository"><FolderPlus size={15} /> Add</button>
      <button class="btn" onclick={() => void gr.scanFolder()} disabled={gr.scanning} title="Find the git repositories in a folder">
        {#if gr.scanning}<LoaderCircle size={15} class="spin" />{:else}<FolderSearch size={15} />{/if} Scan folder
      </button>
      <button
        class="account-chip"
        class:signed-out={!account}
        onclick={() => (gr.settingsOpen = "account")}
        title={account ? `Signed in to GitHub as ${account.login}` : "Connect your GitHub account"}
      >
        {#if account?.avatarUrl}
          <img src={account.avatarUrl} alt="" width="22" height="22" />
        {:else}
          <GithubMark size={16} />
        {/if}
        <span>{account ? account.login : "Connect GitHub"}</span>
      </button>
      <button class="icon-btn settings" aria-label="AI and account settings" title="AI and account settings" onclick={() => (gr.settingsOpen = "ai")}>
        <Settings size={16} />
      </button>
    </div>

    {#if showBanner}
      <div class="banners">
        {#if !gr.page.git}
          <div class="banner warn">
            <CircleAlert size={16} />
            <span><strong>Git is not installed.</strong> This page runs git for every project.</span>
            <button class="btn small primary" onclick={() => void installTool("git")}><Download size={13} /> Install Git</button>
          </div>
        {/if}
        {#if !account && gr.repos.length}
          <div class="banner">
            <GithubMark size={16} />
            <span><strong>Connect your GitHub account</strong> to see releases and Actions runs, and to publish.</span>
            <button class="btn small primary" onclick={() => (gr.settingsOpen = "account")}>Connect</button>
          </div>
        {/if}
        {#if gbr && !gbr.dismissed}
          <div class="banner info">
            <Info size={16} />
            <span>
              <strong>Brought over from Github-Build-Release.</strong>
              {#if gbr.project}Its last project was added{gbr.deepseekKey ? " and" : "."}{/if}
              {#if gbr.deepseekKey}{gbr.project ? " its" : "Its"} DeepSeek key is now kept encrypted here{gbr.plaintextRemoved ? "; the plain-text copy in its settings file was deleted." : "."}{/if}
              {#if gbr.error}<em>{gbr.error}</em>{/if}
              {#if gbr.configPath}<small class="selectable">{gbr.configPath}</small>{/if}
            </span>
            <button class="icon-btn" aria-label="Dismiss" onclick={() => void dismissImport()}><X size={14} /></button>
          </div>
        {/if}
      </div>
    {/if}

    {#if !gr.repos.length}
      <div class="welcome glass">
        <div class="welcome-symbol" aria-hidden="true"><FolderGit2 size={34} strokeWidth={1.5} /></div>
        <div>
          <h2>Add your first project</h2>
          <p>
            Pick a folder with a git repository, or scan a folder that holds several. Monorepos show each app on its own, with its own
            versions and tags.
          </p>
          <div class="welcome-actions">
            <button class="btn primary" onclick={() => void gr.addFolder()}><FolderPlus size={15} /> Add a project</button>
            <button class="btn" onclick={() => void gr.scanFolder()}><FolderSearch size={15} /> Scan a folder</button>
          </div>
          {#if !gr.aiReady}
            <p class="hint">
              <Sparkles size={13} /> Commit messages and release notes can be written by AI.
              <button class="link" onclick={() => (gr.settingsOpen = "ai")}>Set up a free Groq key</button>
            </p>
          {/if}
        </div>
      </div>
    {:else}
      <div class="split" class:has-selection={!gr.showList && !!gr.selected}>
        <div class="list glass">
          <div class="list-head">
            <div>
              <h2>Projects</h2>
              <span>{gr.visible.length === gr.items.length ? `${gr.items.length} in ${gr.repos.length} ${gr.repos.length === 1 ? "repository" : "repositories"}` : `${gr.visible.length} of ${gr.items.length} shown`}</span>
            </div>
          </div>
          <div class="list-scroll" role="listbox" aria-label="Projects">
            {#each gr.visible as item (item.id)}
              {@const status = item.status}
              {@const entry = item.entry}
              {@const active = gr.running(item.repo.id)}
              <button
                class="item"
                class:selected={gr.selected?.id === item.id}
                class:missing={!!status?.problem}
                role="option"
                aria-selected={gr.selected?.id === item.id}
                onclick={() => gr.select(item.id)}
              >
                <span class="text">
                  <strong>
                    {entry?.name ?? item.repo.name}
                    {#if entry?.versions.current}<span class="ver">{entry.versions.current}</span>{/if}
                  </strong>
                  <small>{subtitle(item)}</small>
                  {#if entry?.buildKinds.length}
                    <span class="kinds">{#each entry.buildKinds.slice(0, 3) as kind (kind)}<span>{buildKindLabels[kind]}</span>{/each}</span>
                  {/if}
                </span>
                <span class="marks">
                  {#if active || gr.refreshing[item.repo.id]}
                    <LoaderCircle size={13} class="spin" />
                  {/if}
                  {#if (entry ? entry.changes : status?.changes) }
                    <span class="count" title="Uncommitted changes">{entry ? entry.changes : status?.changes}</span>
                  {/if}
                  {#if status?.branch.ahead}<span class="sync" title="Commits to push"><ArrowUp size={11} />{status.branch.ahead}</span>{/if}
                  {#if status?.branch.behind}<span class="sync behind" title="Commits to pull"><ArrowDown size={11} />{status.branch.behind}</span>{/if}
                  {#if entry?.versions.mismatched.length}<span class="warn-dot" title="The version files disagree"><CircleAlert size={13} /></span>{/if}
                </span>
              </button>
            {:else}
              <div class="empty">
                <Search size={22} aria-hidden="true" />
                <strong>No matching projects</strong>
                <button class="btn small" onclick={() => (gr.query = "")}>Clear the search</button>
              </div>
            {/each}
          </div>
        </div>

        <div class="panel glass">
          {#if gr.selected}
            {#key gr.selected.id}
              <ProjectPanel item={gr.selected} />
            {/key}
          {:else}
            <div class="pick">
              <FolderGit2 size={28} strokeWidth={1.5} />
              <strong>Choose a project</strong>
              <span>Its changes, builds and releases show here.</span>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}

{#if gr.settingsOpen}
  <SettingsDialog section={gr.settingsOpen} onclose={() => (gr.settingsOpen = null)} />
{/if}
{#if gr.found}
  <FoundDialog found={gr.found} onclose={() => (gr.found = null)} />
{/if}

<style>
  .loading {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 10px;
    height: 220px;
    color: var(--text-3);
    font-size: 12.5px;
  }

  .releases {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
    min-height: 0;
    container: releases / inline-size;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
  }

  .search {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 40px;
    padding: 0 8px 0 14px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 10px;
    background: rgb(0 0 0 / 0.2);
    color: var(--text-3);
  }

  .search:focus-within {
    border-color: rgb(var(--accent-rgb) / 0.55);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--text-1);
    font: inherit;
    font-size: 13.5px;
  }

  .clear {
    width: 26px;
    height: 26px;
  }

  .toolbar > :global(.btn) {
    flex: none;
    height: 40px;
  }

  .account-chip {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
    max-width: 200px;
    height: 40px;
    padding: 0 12px 0 9px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 999px;
    background: rgb(255 255 255 / 0.04);
    color: var(--text-1);
    font-size: 12.5px;
  }

  .account-chip:hover {
    background: var(--hover);
  }

  .account-chip.signed-out {
    border-color: rgb(var(--accent-rgb) / 0.4);
    color: #c9cffb;
  }

  .account-chip img {
    border-radius: 50%;
  }

  .account-chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .settings {
    flex: none;
    width: 40px;
    height: 40px;
  }

  .banners {
    display: grid;
    gap: 8px;
  }

  .banner {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 10px 12px 10px 14px;
    border: 1px solid rgb(var(--accent-rgb) / 0.28);
    border-radius: 12px;
    background: rgb(var(--accent-rgb) / 0.08);
    color: #c9cffb;
    font-size: 12.5px;
  }

  .banner > span {
    flex: 1;
    min-width: 0;
    color: var(--text-2);
    line-height: 1.45;
  }

  .banner strong {
    color: var(--text-1);
  }

  .banner small {
    display: block;
    overflow: hidden;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .banner em {
    display: block;
    color: #ffb4b4;
    font-style: normal;
  }

  .banner.warn {
    border-color: rgb(255 180 84 / 0.3);
    background: rgb(255 180 84 / 0.08);
    color: #ffd08a;
  }

  .banner.info {
    border-color: rgb(62 207 142 / 0.28);
    background: rgb(62 207 142 / 0.07);
    color: #8fe6bf;
  }

  .welcome {
    display: flex;
    align-items: center;
    gap: clamp(22px, 4vw, 44px);
    padding: clamp(26px, 4vw, 48px);
    border-radius: var(--radius-xl);
  }

  .welcome-symbol {
    display: grid;
    place-items: center;
    width: 104px;
    aspect-ratio: 1;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.27);
    border-radius: 28px;
    background: linear-gradient(145deg, rgb(var(--accent-rgb) / 0.21), rgb(62 207 142 / 0.07));
    color: #c5cafd;
  }

  .welcome h2 {
    margin: 0 0 8px;
    font-family: var(--font-brand);
    font-size: 24px;
    font-weight: 600;
  }

  .welcome p {
    max-width: 60ch;
    color: var(--text-2);
    font-size: 13px;
    line-height: 1.55;
  }

  .welcome-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 9px;
    margin-top: 16px;
  }

  .hint {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 14px;
    color: var(--text-3) !important;
    font-size: 12px !important;
  }

  .link {
    padding: 0;
    color: #b7befa;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .split {
    display: grid;
    flex: 1 1 auto;
    grid-template-columns: minmax(280px, 340px) minmax(0, 1fr);
    gap: 12px;
    min-height: 360px;
  }

  @container releases (min-width: 1300px) {
    .split {
      grid-template-columns: 380px minmax(0, 1fr);
    }
  }

  .list,
  .panel {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border-radius: var(--radius-lg);
  }

  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 58px;
    padding: 11px 18px;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
  }

  .list-head h2 {
    margin: 0 0 2px;
    font-size: 14px;
    font-weight: 650;
  }

  .list-head span {
    color: var(--text-3);
    font-size: 11.5px;
  }

  .list-scroll {
    display: grid;
    flex: 1;
    align-content: start;
    gap: 2px;
    min-height: 0;
    padding: 7px;
    overflow: auto;
    scrollbar-width: thin;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 56px;
    padding: 8px 11px;
    border: 1px solid transparent;
    border-radius: 10px;
    text-align: left;
    transition: background var(--dur-fast), border-color var(--dur-fast);
  }

  .item:hover {
    background: var(--hover);
  }

  .item.selected {
    border-color: rgb(var(--accent-rgb) / 0.33);
    background: rgb(var(--accent-rgb) / 0.13);
  }

  .item.missing small {
    color: #ffb27a;
  }

  .text {
    display: grid;
    flex: 1;
    gap: 2px;
    min-width: 0;
  }

  .text strong,
  .text small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text strong {
    font-size: 13.2px;
    font-weight: 620;
  }

  .ver {
    margin-left: 5px;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
  }

  .text small {
    color: var(--text-2);
    font-size: 11.5px;
  }

  .kinds {
    display: flex;
    gap: 4px;
    margin-top: 2px;
  }

  .kinds span {
    padding: 0 6px;
    border-radius: 5px;
    background: rgb(255 255 255 / 0.06);
    color: var(--text-3);
    font-size: 10px;
    line-height: 16px;
  }

  .marks {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .count {
    min-width: 19px;
    padding: 0 5px;
    border-radius: 999px;
    background: rgb(255 196 92 / 0.16);
    color: #ffd08a;
    line-height: 18px;
    text-align: center;
  }

  .sync {
    display: inline-flex;
    align-items: center;
    color: #9fb4ff;
  }

  .sync.behind {
    color: #7fd8ff;
  }

  .warn-dot {
    display: inline-flex;
    color: #ffb84d;
  }

  .empty,
  .pick {
    display: grid;
    justify-items: center;
    align-content: center;
    gap: 8px;
    min-height: 220px;
    padding: 30px 18px;
    color: var(--text-3);
    font-size: 12.5px;
    text-align: center;
  }

  .pick {
    flex: 1;
  }

  .empty strong,
  .pick strong {
    color: var(--text-1);
    font-size: 14px;
  }

  @container releases (max-width: 820px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
    }

    .split.has-selection .list,
    .split:not(.has-selection) .panel {
      display: none;
    }
  }

  @container releases (max-width: 640px) {
    .toolbar > :global(.btn) {
      padding: 0 11px;
    }

    .account-chip span {
      display: none;
    }
  }
</style>
