<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import Archive from "@lucide/svelte/icons/archive";
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CloudOff from "@lucide/svelte/icons/cloud-off";
  import CloudUpload from "@lucide/svelte/icons/cloud-upload";
  import Database from "@lucide/svelte/icons/database";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderCode from "@lucide/svelte/icons/folder-code";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import Gauge from "@lucide/svelte/icons/gauge";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import CloudLogo from "../../../lib/components/CloudLogo.svelte";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { readFlag, writeFlag } from "../../../lib/storage";
  import ComparePanel from "./ComparePanel.svelte";
  import ExclusionsPanel from "./ExclusionsPanel.svelte";
  import ProjectEditor from "./ProjectEditor.svelte";
  import ProjectRow from "./ProjectRow.svelte";
  import StatTile from "./StatTile.svelte";
  import type { BackupProvider, ProjectResult } from "./api";
  import {
    formatBytes,
    projectBackupsState as pb,
    providerNames,
    samePath,
    stageHints,
    stageLabels,
    stageStep,
    type Group,
  } from "./state.svelte";

  let searchInput = $state<HTMLInputElement>();

  onMount(() => {
    void pb.init().then(() => pb.startWatching());
    const timer = setInterval(() => (pb.clock = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  /** "/" or Ctrl+F jumps to the project search; Escape clears it. */
  function onKeydown(event: KeyboardEvent) {
    if (pb.editor || pb.compare || event.defaultPrevented) return;
    const target = event.target as HTMLElement | null;
    const typing = !!target?.closest("input, textarea, select, [contenteditable='true']");
    if ((event.key === "/" && !typing) || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f")) {
      if (!searchInput) return;
      event.preventDefault();
      searchInput.focus();
      searchInput.select();
    } else if (event.key === "Escape" && target === searchInput && pb.query) {
      event.preventDefault();
      pb.query = "";
    }
  }

  const GUIDE_KEY = "myle.projectBackups.guideHidden";
  let guideHidden = $state(readFlag(GUIDE_KEY, false));
  const hasDestination = $derived(!!pb.provider && !!pb.page.settings.cloudFolder);
  const hasBackup = $derived(pb.projects.some((project) => !!project.lastBackup));
  /** First-run steps, until the first backup is made (or the user hides them). */
  const showGuide = $derived(!pb.loading && !guideHidden && (!hasDestination || !pb.projects.length || !hasBackup));

  function hideGuide() {
    guideHidden = true;
    writeFlag(GUIDE_KEY, true);
  }

  function focusDestination() {
    const tile = document.querySelector<HTMLButtonElement>(".cloud-tile");
    tile?.scrollIntoView({ behavior: "smooth", block: "center" });
    tile?.focus();
  }

  function reviewExclusions() {
    pb.exclusionsOpen = true;
    requestAnimationFrame(() =>
      document.getElementById("project-backups-exclusions")?.scrollIntoView({ behavior: "smooth", block: "start" }),
    );
  }

  const otherProvider = (provider: BackupProvider | null): BackupProvider =>
    provider === "dropbox" ? "googleDrive" : "dropbox";

  interface FailureAction {
    label: string;
    icon: typeof Play;
    run: () => void;
    primary?: boolean;
  }

  /** What the user can do about a failed backup, by its code. */
  function actionsFor(failure: ProjectResult): FailureAction[] {
    const project = pb.projects.find((item) => item.id === failure.projectId);
    const retry: FailureAction = { label: "Try again", icon: RotateCw, run: () => void pb.retryFailures([failure.projectId]) };
    const provider = pb.provider;
    switch (failure.code) {
      case "SOURCE_MISSING":
        return project ? [{ label: "Find the folder", icon: FolderSearch, run: () => pb.openEditor(project, true), primary: true }] : [];
      case "NO_PROVIDER":
        return [{ label: "Choose where backups go", icon: CloudUpload, run: focusDestination, primary: true }];
      case "DESTINATION_MISSING":
        return [
          { label: "Choose the folder again", icon: FolderSearch, run: () => provider && void pb.useCloud(provider, null), primary: true },
          retry,
        ];
      case "CLOUD_NOT_INSTALLED":
        return [
          {
            label: `Back up to ${providerNames[otherProvider(provider)]}`,
            icon: CloudUpload,
            run: () => chooseTile(otherProvider(provider)),
            primary: true,
          },
          {
            label: "Get Google Drive",
            icon: ExternalLink,
            run: () => void openUrl("https://www.google.com/drive/download/"),
          },
        ];
      case "CLOUD_NOT_READY":
        return [
          ...(provider ? [{ label: `Start ${providerNames[provider]}`, icon: Play, run: () => void pb.startCloud(provider), primary: true }] : []),
          retry,
        ];
      case "NO_FILES":
        return [
          { label: "Review exclusions", icon: ListFilter, run: reviewExclusions, primary: true },
          ...(project ? [{ label: "Edit project", icon: Pencil, run: () => pb.openEditor(project) }] : []),
        ];
      case "NAME_TAKEN":
        return [{ ...retry, label: "Back up again", primary: true }];
      default:
        return [retry];
    }
  }

  const step = $derived(pb.operation ? stageStep(pb.operation.stage) : null);
  const providers: BackupProvider[] = ["googleDrive", "dropbox"];
  /** One tile per provider: its folders found on this PC, or a prompt to choose one. */
  const tiles = $derived(providers.map((provider) => ({ provider, folders: pb.page.clouds.filter((cloud) => cloud.provider === provider) })));
  const progress = $derived(pb.operation?.totalBytes ? Math.max(0, Math.min(1, pb.operation.doneBytes / pb.operation.totalBytes)) : null);

  let now = $state(Date.now());
  $effect(() => {
    if (!pb.operation) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const elapsed = $derived(Math.max(0, Math.round((now - (pb.operation?.startedAt ?? now)) / 1000)));

  function formatElapsed(seconds: number) {
    const minutes = Math.floor(seconds / 60);
    return minutes ? `${minutes}:${String(seconds % 60).padStart(2, "0")}` : `${seconds}s`;
  }

  function chooseTile(provider: BackupProvider) {
    const folders = pb.page.clouds.filter((cloud) => cloud.provider === provider);
    // A single detected folder is used as is; otherwise the user points at it.
    void pb.useCloud(provider, folders.length === 1 ? folders[0].path : null);
  }

  const inUse = (provider: BackupProvider) => pb.provider === provider && !!pb.page.settings.cloudFolder;
  const projectsOf = (group: Group) => group.projects.map((project) => project.id);
  const summary = $derived(pb.summary);
  /** The one next step for the whole page, said in a line. */
  const headline = $derived.by(() => {
    if (!pb.projects.length) return "Add your first project folder to start backing up.";
    if (!hasDestination) return "Choose where backups go — Google Drive or Dropbox.";
    if (summary.missing) return `${summary.missing} ${summary.missing === 1 ? "folder was" : "folders were"} not found.`;
    if (summary.attention) return `${summary.attention} of ${summary.projects} ${summary.attention === 1 ? "project wants" : "projects want"} a look.`;
    return "Everything is backed up and checked.";
  });

  const filters: { id: "all" | Group["key"]; label: string }[] = [
    { id: "all", label: "All" }, { id: "attention", label: "Did not finish" }, { id: "never", label: "Not backed up" },
    { id: "stale", label: "Getting old" }, { id: "ok", label: "Up to date" }, { id: "missing", label: "Folder missing" },
  ];

  /** How many projects each filter would show. */
  const counts = $derived.by(() => {
    const map: Record<string, number> = { all: pb.projects.length };
    for (const group of pb.allGroups) map[group.key] = group.projects.length;
    for (const id of ["never", "stale", "ok", "missing", "attention"] as const) map[id] ??= 0;
    return map;
  });

  /** What the list shows: every group, or the one the filter picked. */
  const visibleGroups = $derived(pb.groups);
  const shown = $derived(visibleGroups.reduce((sum, group) => sum + group.projects.length, 0));
</script>

<svelte:window onkeydown={onKeydown} />

<div class="page-root" inert={!!pb.editor || !!pb.compare}>
  <div class="top">
    <PageHeader
      title="Project Backups"
      subtitle="Zip your projects into Google Drive or Dropbox, checked before they are kept."
    />
    <div class="top-right">
      <button
        class="btn settings-button"
        class:active={pb.exclusionsOpen}
        aria-expanded={pb.exclusionsOpen}
        aria-controls="project-backups-exclusions"
        onclick={() => (pb.exclusionsOpen = !pb.exclusionsOpen)}
      >
        <ListFilter size={15} /> What is left out
      </button>
    </div>
  </div>

  {#if !pb.loading}
    <section class="summary surface" aria-label="Project backup summary">
      <div class="stats">
        <StatTile label={summary.projects === 1 ? "project" : "projects"} value={String(summary.projects)} tone="accent" />
        <StatTile label="up to date" value={String(summary.ok)} tone={summary.ok ? "ok" : "quiet"} />
        <StatTile label="want a look" value={String(summary.attention)} tone={summary.attention ? (summary.missing ? "danger" : "warn") : "quiet"} />
        <StatTile label="in latest backups" value={summary.knownSizes ? formatBytes(summary.backupBytes) : "—"} icon={Database} title="Each project's latest zip, together (older backups are not counted)" />
        <StatTile
          label="last backup"
          value={summary.newest === null ? "never" : pb.ago(summary.newest)}
          icon={Archive}
          title={summary.oldest === null ? "No backup yet" : `The project backed up longest ago: ${pb.ago(summary.oldest)}`}
        />
      </div>
      <p class="headline" class:good={summary.attention === 0 && !!pb.projects.length && hasDestination}>{headline}</p>
      {#if hasDestination}
        <button class="root-chip" title={pb.page.backupRoot ?? ""} onclick={() => pb.open("root")}>
          <FolderOpen size={13} /><span class="selectable">{pb.page.backupRoot ?? "Backups folder"}</span>
        </button>
      {/if}
    </section>
  {/if}

  <section class="destination surface" aria-labelledby="destination-heading">
    <div class="destination-head">
      <h2 id="destination-heading">Back up to</h2>
      {#if !hasDestination}<span class="needed">Not chosen yet</span>{/if}
    </div>
    <div class="cloud-list">
      {#each tiles as tile (tile.provider)}
        {@const selected = inUse(tile.provider)}
        <button
          class="cloud-tile" class:found={tile.folders.length > 0} class:in-use={selected} aria-pressed={selected} disabled={pb.locked}
          title={tile.folders.length ? `Backups go to ${tile.folders[0].path}\\Projects Backup` : `${providerNames[tile.provider]} was not found on this PC. Choose its folder.`}
          onclick={() => chooseTile(tile.provider)}
        >
          <span class="cloud-logo"><CloudLogo provider={tile.provider} size={20} /></span>
          <span class="cloud-text">
            <strong>{providerNames[tile.provider]}</strong>
            <small>{#if selected}Selected{:else if tile.folders.length > 1}{tile.folders.length} folders found{:else if tile.folders.length}Found on this PC{:else}Browse…{/if}</small>
          </span>
          {#if selected}<Check size={14} class="cloud-check" />{/if}
        </button>
      {/each}
    </div>
    {#each tiles as tile (tile.provider)}
      {#if tile.folders.length > 1}
        <div class="folder-choices" aria-label={`${providerNames[tile.provider]} folders`}>
          {#each tile.folders as folder (folder.path)}
            {@const current = !!pb.page.settings.cloudFolder && samePath(folder.path, pb.page.settings.cloudFolder)}
            <button class="chip" class:active={current} disabled={pb.locked} title={folder.path} onclick={() => pb.useCloud(tile.provider, folder.path)}>
              {folder.label || providerNames[tile.provider]} · {folder.path}
            </button>
          {/each}
          <button class="chip" disabled={pb.locked} onclick={() => pb.useCloud(tile.provider, null)}>Other folder…</button>
        </div>
      {/if}
    {/each}
    {#if pb.cloud && !pb.cloud.available && pb.provider === "googleDrive"}
      <p class="helper"><CloudOff size={12} /> Google Drive is not running now; it starts by itself when a backup begins.</p>
    {/if}
  </section>

  {#if pb.exclusionsOpen}
    <div id="project-backups-exclusions" transition:slide={{ duration: 190 }}><ExclusionsPanel /></div>
  {/if}

  {#if pb.error}
    <div class="banner error surface" role="alert">
      <CircleAlert size={18} /><span>{pb.error}</span>
      <button class="btn small ghost" onclick={() => pb.refresh()}><RefreshCw size={13} /> Try again</button>
    </div>
  {/if}

  {#if pb.failures.length}
    <div class="banner warning surface failures" role="alert">
      <CircleAlert size={18} />
      <div class="failures-body">
        <strong>{pb.failures.length} {pb.failures.length === 1 ? "project was" : "projects were"} not backed up</strong>
        <ul>
          {#each pb.failures as failure (failure.projectId)}
            {@const project = pb.projects.find((item) => item.id === failure.projectId)}
            <li>
              <b>{project?.name ?? failure.name}</b>
              <span class="selectable">{failure.error}</span>
              <span class="failure-actions">
                {#each actionsFor(failure) as action (action.label)}
                  <button class="btn small" class:primary={action.primary} disabled={pb.locked} onclick={action.run}><action.icon size={13} /> {action.label}</button>
                {/each}
              </span>
            </li>
          {/each}
        </ul>
      </div>
      <span class="banner-actions">
        {#if pb.failures.length > 1}
          <button class="btn small" disabled={pb.locked} onclick={() => pb.retryFailures()}><RotateCw size={13} /> Try all again</button>
        {/if}
        <button class="btn small ghost" onclick={() => pb.dismissFailures()}>Dismiss</button>
      </span>
    </div>
  {/if}

  {#if showGuide}
    <section class="guide surface" aria-labelledby="guide-heading" transition:slide={{ duration: 160 }}>
      <div class="guide-head">
        <h2 id="guide-heading">Get started in three steps</h2>
        <button class="icon-btn" title="Hide these steps" aria-label="Hide the getting started steps" onclick={hideGuide}><X size={14} /></button>
      </div>
      <ol>
        <li class:done={hasDestination}>
          <span class="step-mark">{#if hasDestination}<Check size={12} />{:else}1{/if}</span>
          <span class="step-text">
            <strong>Choose where backups go</strong>
            <small>{hasDestination ? `Backups go to ${pb.page.backupRoot ?? "your cloud folder"}.` : "Pick Google Drive or Dropbox above. Google Drive is started for you when a backup begins."}</small>
          </span>
          {#if !hasDestination}<button class="btn small" onclick={focusDestination}>Choose</button>{/if}
        </li>
        <li class:done={pb.projects.length > 0}>
          <span class="step-mark">{#if pb.projects.length}<Check size={12} />{:else}2{/if}</span>
          <span class="step-text">
            <strong>Add a project folder</strong>
            <small>Build output, dependencies and caches (node_modules, target, .venv, bin/obj and more) are left out by themselves; .env files are kept.</small>
          </span>
          {#if !pb.projects.length}<button class="btn small" disabled={pb.locked} onclick={() => pb.openEditor(null)}><Plus size={13} /> Add</button>{/if}
        </li>
        <li class:done={hasBackup}>
          <span class="step-mark">{#if hasBackup}<Check size={12} />{:else}3{/if}</span>
          <span class="step-text">
            <strong>Back up</strong>
            <small>Each zip is checked twice (after zipping and after the copy) before it is kept. Compare any two backups later.</small>
          </span>
          {#if hasDestination && pb.projects.length && !hasBackup}
            <button class="btn small primary" disabled={pb.locked} onclick={() => pb.backup(pb.projects.map((project) => project.id))}><CloudUpload size={13} /> Back up now</button>
          {/if}
        </li>
      </ol>
    </section>
  {/if}

  <section class="projects surface">
    <div class="toolbar">
      <button class="btn primary" disabled={pb.locked || !pb.projects.length}
        title={pb.query && pb.filtered.length !== pb.projects.length ? "Backs up every project, also those hidden by the search" : undefined}
        onclick={() => pb.backup(pb.projects.map((project) => project.id))}>
        <CloudUpload size={15} /> Back up all
      </button>
      <button class="btn" disabled={pb.locked || !pb.changedIds.length} title="Backs up only the projects whose folders changed since their last backup" onclick={() => pb.backup(pb.changedIds)}>
        <CloudUpload size={15} /> Back up changed{pb.changedIds.length ? ` (${pb.changedIds.length})` : ""}
      </button>
      <button class="icon-btn" title="Check every project folder for changes now" aria-label="Check for changes" disabled={pb.checking.length > 0} onclick={() => pb.checkChanges(undefined, true)}>
        <RefreshCw size={14} class={pb.checking.length ? "spin" : ""} />
      </button>
      <button class="btn" disabled={pb.locked} onclick={() => pb.openEditor(null)}><Plus size={15} /> Add project</button>
      {#if pb.projects.length > 1}
        <label class="search">
          <Search size={13} />
          <input bind:this={searchInput} bind:value={pb.query} class="input" type="search" placeholder="Search projects  /" aria-label="Search projects" aria-keyshortcuts="/ Control+F" />
        </label>
      {/if}
      <span class="spacer"></span>
      <button class="btn ghost" disabled={pb.locked} title="Bring in the projects of the Backup Projects app" onclick={() => pb.importProjects()}>
        <Download size={14} /> Import from Backup Projects
      </button>
    </div>

    {#if pb.projects.length > 1}
      <div class="filters" role="group" aria-label="Show projects by state">
        {#each filters as filter (filter.id)}
          <button class="chip small" class:active={pb.filter === filter.id} onclick={() => pb.setFilter(filter.id)} disabled={filter.id !== "all" && !counts[filter.id]}>
            {filter.label}<span class="count">{counts[filter.id] ?? 0}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if pb.operation}
      {@const operation = pb.operation}
      <div class="operation" aria-live="polite">
        <LoaderCircle size={14} class="spin" />
        <span class="operation-text">
          <span class="operation-line">
            <strong>{stageLabels[operation.stage]}</strong>
            {#if operation.projectName}<small class="project-name">{operation.projectName}{operation.total > 1 ? ` · project ${operation.index + 1} of ${operation.total}` : ""}</small>{/if}
            {#if step}<small>step {step.step} of {step.of}</small>{/if}
          </span>
          <small class="hint-line">{operation.note ?? stageHints[operation.stage]}</small>
        </span>
        {#if operation.totalFiles}<span class="numbers">{operation.doneFiles.toLocaleString()} / {operation.totalFiles.toLocaleString()} files</span>{/if}
        {#if operation.totalBytes}
          <span class="numbers">{formatBytes(operation.doneBytes)} / {formatBytes(operation.totalBytes)}{progress !== null ? ` · ${Math.floor(progress * 100)}%` : ""}</span>
        {/if}
        {#if elapsed >= 2}<span class="numbers">{formatElapsed(elapsed)}</span>{/if}
        <button class="btn small" disabled={pb.cancelling} onclick={() => pb.cancel()}>{pb.cancelling ? "Cancelling…" : "Cancel"}</button>
        <div class="operation-bar"><ProgressBar value={progress} label="Backup progress" /></div>
      </div>
    {/if}

    {#if pb.loading}
      <div class="loading" role="status"><LoaderCircle size={17} class="spin" /> Loading Project Backups…</div>
    {:else if !pb.projects.length}
      <div class="empty">
        <span class="empty-icon"><FolderCode size={25} /></span>
        <strong>No projects yet</strong>
        <p>Add a project folder, or bring in the projects you back up with the Backup Projects app.</p>
        <div class="empty-actions">
          <button class="btn primary" disabled={pb.locked} onclick={() => pb.openEditor(null)}><Plus size={14} /> Add project</button>
          <button class="btn" disabled={pb.locked} onclick={() => pb.importProjects()}><Download size={14} /> Import</button>
        </div>
      </div>
    {:else if !shown}
      <div class="empty small">
        <span class="empty-icon"><Gauge size={22} /></span>
        <strong>{pb.query ? `No project matches “${pb.query}”` : "Nothing in this group right now"}</strong>
        <p>{pb.query ? "The search looks at names, backup names and folders." : "Every project here is in another group; try All."}</p>
        <div class="empty-actions">
          <button class="btn small" onclick={() => { pb.query = ""; pb.setFilter("all"); }}><X size={13} /> Show everything</button>
        </div>
      </div>
    {:else}
      {#each visibleGroups as group (group.key)}
        {#if group.key === "attention"}
          <div class="banner warning surface inline" role="status">
            <CircleAlert size={17} />
            <span><strong>{group.projects.length} {group.projects.length === 1 ? "backup did" : "backups did"} not finish.</strong> The reason and the fix are on each project below.</span>
          </div>
        {/if}
        <div class="group-head">
          <h3>{group.title}</h3>
          <span class="group-note">{group.note}</span>
          <span class="group-count">{group.projects.length}</span>
          {#if group.key === "never" || group.key === "stale" || group.key === "attention"}
            <button class="btn small" disabled={pb.locked} onclick={() => pb.backup(projectsOf(group))}>
              <CloudUpload size={13} /> Back up {group.key === "attention" ? "these" : "all"}
            </button>
          {/if}
        </div>
        {#each group.projects as project (project.id)}
          <ProjectRow {project} />
        {/each}
      {/each}
    {/if}
  </section>
</div>

{#if pb.editor}<ProjectEditor />{/if}
{#if pb.compare}<ComparePanel />{/if}

<style>
  .page-root { container-type: inline-size; }
  .top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .top :global(header) { margin-bottom: 14px; }
  .top-right { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 8px; margin-top: 3px; }
  .settings-button.active { border-color: rgb(var(--accent-rgb) / 0.25); background: var(--selected); }
  /* The summary: the numbers, one line about where the page stands, and the folder everything goes to. */
  .summary { display: flex; align-items: center; flex-wrap: wrap; gap: 9px 14px; margin-bottom: 12px; padding: 12px 13px; }
  /* The numbers take the whole first line; the line about the page follows under them. */
  .stats { display: flex; flex-wrap: wrap; gap: 8px; flex: 1 0 100%; }
  .stats > :global(*) { flex: 1 1 150px; }
  .headline { color: var(--text-2); font-size: 12px; }
  .headline.good { color: #98dfbd; }
  .root-chip { display: inline-flex; align-items: center; gap: 7px; min-width: 0; max-width: 100%; height: 27px; padding: 0 10px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 999px; background: rgb(255 255 255 / 0.025); color: var(--text-3); font-size: 10.75px; }
  .root-chip:hover { background: var(--hover); color: var(--text-2); }
  .root-chip span { overflow: hidden; font-family: var(--font-mono); white-space: nowrap; text-overflow: ellipsis; }
  .destination { display: grid; gap: 10px; margin-bottom: 12px; padding: 12px 13px; }
  .destination-head { display: flex; align-items: center; gap: 10px; }
  .destination-head h2 { color: var(--text-2); font-size: 12px; font-weight: 600; }
  .needed { padding: 1px 7px; border-radius: 999px; background: rgb(245 176 65 / 0.1); color: rgb(245 188 95 / 0.9); font-size: 10px; }
  .cloud-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .cloud-tile {
    display: flex; align-items: center; gap: 9px; min-width: 0; min-height: 52px; padding: 9px 10px; border: 1px dashed rgb(255 255 255 / 0.12);
    border-radius: 9px; background: rgb(0 0 0 / 0.06); text-align: left; transition: background 140ms, border-color 140ms;
  }
  .cloud-tile.found { border-style: solid; border-color: rgb(255 255 255 / 0.09); background: rgb(255 255 255 / 0.025); }
  .cloud-tile:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.45); background: rgb(var(--accent-rgb) / 0.08); }
  .cloud-tile.in-use { border-style: solid; border-color: rgb(62 207 142 / 0.32); background: rgb(62 207 142 / 0.07); }
  .cloud-logo { display: grid; place-items: center; flex: none; }
  .cloud-tile:not(.found) .cloud-logo { opacity: 0.65; }
  .cloud-text { display: grid; gap: 2px; min-width: 0; }
  .cloud-text strong { overflow: hidden; color: var(--text-1); font-size: 12px; font-weight: 500; white-space: nowrap; text-overflow: ellipsis; }
  .cloud-text small { color: var(--text-3); font-size: 10.5px; }
  .cloud-tile.in-use small { color: #98dfbd; }
  .cloud-tile :global(.cloud-check) { flex: none; margin-left: auto; color: #98dfbd; }
  .folder-choices { display: flex; flex-wrap: wrap; gap: 6px; }
  .helper { display: flex; align-items: center; gap: 6px; color: var(--text-3); font-size: 11px; }
  .banner { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; padding: 11px 13px; font-size: 12px; }
  .banner.inline { align-items: center; margin: 4px 0 0; padding: 9px 11px; background: rgb(245 176 65 / 0.05); }
  .banner.warning { border-color: rgb(245 176 65 / 0.22); color: rgb(245 188 95 / 0.85); }
  .banner.error { border-color: rgb(229 72 77 / 0.28); color: rgb(255 145 145 / 0.82); }
  .banner > span { flex: 1; }
  .failures-body { display: grid; flex: 1; gap: 6px; min-width: 0; }
  .failures ul { display: grid; gap: 6px; margin: 0; padding: 0 0 0 10px; border-left: 2px solid rgb(255 255 255 / 0.08); list-style: none; }
  .failures li { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; font-size: 11.5px; line-height: 1.45; }
  .failures li b { color: var(--text-1); font-weight: 600; }
  .failures li span { overflow-wrap: anywhere; }
  .banner-actions { display: flex; flex: none; flex-wrap: wrap; gap: 6px; }
  .failure-actions { display: inline-flex; flex-wrap: wrap; gap: 5px; }
  .failures li .failure-actions { flex-basis: 100%; }
  .guide { display: grid; gap: 9px; margin-bottom: 12px; padding: 12px 13px; }
  .guide-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .guide-head h2 { color: var(--text-2); font-size: 12px; font-weight: 600; }
  .guide ol { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .guide li { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 7px 9px; border-radius: 9px; background: rgb(0 0 0 / 0.1); }
  .guide li.done { opacity: 0.62; }
  .step-mark { display: grid; place-items: center; flex: none; width: 21px; height: 21px; border-radius: 999px; border: 1px solid rgb(var(--accent-rgb) / 0.3); color: var(--accent); font-size: 11px; font-weight: 700; }
  .guide li.done .step-mark { border-color: rgb(62 207 142 / 0.35); color: #98dfbd; }
  .step-text { display: grid; flex: 1; gap: 1px; min-width: 0; }
  .step-text strong { font-size: 12px; font-weight: 600; }
  .step-text small { color: var(--text-3); font-size: 10.75px; line-height: 1.45; }
  /* The list lives in one panel, so the page reads as: summary, destination, then the projects. */
  .projects { display: grid; gap: 7px; padding: 12px 13px; background: var(--grain), linear-gradient(180deg, rgb(255 255 255 / 0.045), rgb(255 255 255 / 0.012)); }
  .search { display: flex; align-items: center; gap: 6px; color: var(--text-3); }
  .search input { width: 210px; height: 30px; font-size: 12px; }
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .spacer { flex: 1; }
  .filters { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; padding-bottom: 9px; border-bottom: 1px solid rgb(255 255 255 / 0.055); }
  .chip.small { height: 26px; padding: 0 10px; font-size: 11.5px; }
  .chip .count { font-variant-numeric: tabular-nums; }
  .group-head { display: flex; align-items: center; gap: 9px; margin-top: 6px; padding: 2px 2px 4px; }
  .group-head h3 { flex: none; font-size: 12px; font-weight: 600; color: var(--text-1); }
  .group-note { overflow: hidden; color: var(--text-3); font-size: 10.75px; white-space: nowrap; text-overflow: ellipsis; }
  .group-count { margin-left: auto; padding: 1px 7px; border-radius: 999px; background: rgb(255 255 255 / 0.05); color: var(--text-3); font-size: 10px; font-variant-numeric: tabular-nums; }
  .group-head + :global(article) { margin-top: 0; }
  .operation { position: relative; display: flex; align-items: center; gap: 8px; min-width: 0; padding: 8px 8px 10px; border-radius: 9px; background: rgb(var(--accent-rgb) / 0.045); color: var(--accent); }
  .operation-text { display: grid; flex: 1; gap: 1px; min-width: 0; }
  .operation-line { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; min-width: 0; }
  .operation-text strong { font-size: 11.5px; font-weight: 600; white-space: nowrap; }
  .hint-line { color: var(--text-3); }
  .operation-text small { overflow: hidden; color: var(--text-3); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
  .numbers { margin-left: auto; color: var(--text-2); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .numbers + .numbers { margin-left: 0; }
  .operation-bar { position: absolute; right: 8px; bottom: 4px; left: 8px; }
  .loading { display: flex; align-items: center; justify-content: center; gap: 8px; min-height: 180px; color: var(--text-3); font-size: 12.5px; }
  .empty { display: grid; justify-items: center; gap: 7px; padding: 42px 20px; text-align: center; }
  .empty.small { padding: 26px 20px; }
  .empty-icon { display: grid; place-items: center; width: 50px; height: 50px; margin-bottom: 2px; border: 1px solid rgb(var(--accent-rgb) / 0.12); border-radius: 15px; background: rgb(var(--accent-rgb) / 0.045); color: rgb(169 179 255 / 0.7); }
  .empty strong { font-size: 13.5px; }
  .empty p { max-width: 440px; color: var(--text-3); font-size: 11.5px; }
  .empty-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 8px; }

  @container (max-width: 860px) { .stats > :global(*) { flex-basis: 132px; } }
  @container (max-width: 620px) {
    .cloud-list { grid-template-columns: minmax(0, 1fr); }
    .search input { width: 150px; }
    .operation { flex-wrap: wrap; }
    .group-note { display: none; }
  }
  @container (max-width: 400px) { .settings-button { width: 100%; } }
</style>
