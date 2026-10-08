<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import Archive from "@lucide/svelte/icons/archive";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CloudUpload from "@lucide/svelte/icons/cloud-upload";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderCode from "@lucide/svelte/icons/folder-code";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import GitCompare from "@lucide/svelte/icons/git-compare";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import Play from "@lucide/svelte/icons/play";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { readFlag, writeFlag } from "../../../lib/storage";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import CloudLogo from "../../../lib/components/CloudLogo.svelte";
  import ComparePanel from "./ComparePanel.svelte";
  import ExclusionsPanel from "./ExclusionsPanel.svelte";
  import ProjectEditor from "./ProjectEditor.svelte";
  import { SOURCE_ID, type BackupProvider, type Project, type ProjectResult } from "./api";
  import {
    formatBytes,
    formatDate,
    projectBackupsState as pb,
    providerNames,
    samePath,
    stageHints,
    stageLabels,
    stageStep,
  } from "./state.svelte";

  let searchInput = $state<HTMLInputElement>();

  onMount(() => {
    void pb.init();
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
  const tiles = $derived(
    providers.map((provider) => ({
      provider,
      folders: pb.page.clouds.filter((cloud) => cloud.provider === provider),
    })),
  );

  const progress = $derived(
    pb.operation?.totalBytes
      ? Math.max(0, Math.min(100, (pb.operation.doneBytes / pb.operation.totalBytes) * 100))
      : null,
  );

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

  const lastAttempt = (project: Project) => {
    const result = project.lastResult;
    if (!result?.at) return null;
    // A good backup is already told by the "Last backup" line.
    if (result.ok) return null;
    return result;
  };

  const inUse = (provider: BackupProvider) => pb.provider === provider && !!pb.page.settings.cloudFolder;
  const backupsOf = (project: Project) => pb.backups[project.id] ?? [];
</script>

<svelte:window onkeydown={onKeydown} />

<div class="page-root" inert={!!pb.editor || !!pb.compare}>
  <div class="top">
    <PageHeader
      title="Project Backups"
      subtitle="Zip your projects into Google Drive or Dropbox, checked before they are kept."
    />
    <div class="top-right">
      <div class="metrics" aria-label="Project backup summary">
        <span class="metric"><FolderCode size={13} /><strong>{pb.projects.length}</strong> {pb.projects.length === 1 ? "project" : "projects"}</span>
      </div>
      <button
        class="btn settings-button"
        class:active={pb.exclusionsOpen}
        aria-expanded={pb.exclusionsOpen}
        aria-controls="project-backups-exclusions"
        onclick={() => (pb.exclusionsOpen = !pb.exclusionsOpen)}
      >
        <ListFilter size={15} /> Exclusions
      </button>
    </div>
  </div>

  <section class="destination surface" aria-labelledby="destination-heading">
    <div class="destination-head">
      <h2 id="destination-heading">Back up to</h2>
      {#if pb.page.backupRoot}
        <span class="root selectable" title={pb.page.backupRoot}>{pb.page.backupRoot}</span>
        <button class="icon-btn" title="Open the backups folder" aria-label="Open the backups folder" onclick={() => pb.open("root")}>
          <FolderOpen size={15} />
        </button>
      {/if}
    </div>
    <div class="cloud-list">
      {#each tiles as tile (tile.provider)}
        {@const selected = inUse(tile.provider)}
        <button
          class="cloud-tile"
          class:found={tile.folders.length > 0}
          class:in-use={selected}
          aria-pressed={selected}
          disabled={pb.locked}
          title={tile.folders.length
            ? `Backups go to ${tile.folders[0].path}\\Projects Backup`
            : `${providerNames[tile.provider]} was not found on this PC. Choose its folder.`}
          onclick={() => chooseTile(tile.provider)}
        >
          <span class="cloud-logo"><CloudLogo provider={tile.provider} size={20} /></span>
          <span class="cloud-text">
            <strong>{providerNames[tile.provider]}</strong>
            <small>
              {#if selected}Selected{:else if tile.folders.length > 1}{tile.folders.length} folders found{:else if tile.folders.length}Detected{:else}Browse…{/if}
            </small>
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
            <button class="chip" class:active={current} disabled={pb.locked} title={folder.path}
              onclick={() => pb.useCloud(tile.provider, folder.path)}>
              {folder.label || providerNames[tile.provider]} · {folder.path}
            </button>
          {/each}
          <button class="chip" disabled={pb.locked} onclick={() => pb.useCloud(tile.provider, null)}>Other folder…</button>
        </div>
      {/if}
    {/each}
    {#if pb.provider === "googleDrive" && pb.cloud && !pb.cloud.available}
      <p class="helper">Google Drive is not running now; it starts by itself when a backup begins.</p>
    {/if}
  </section>

  {#if pb.exclusionsOpen}
    <div transition:slide={{ duration: 190 }}><ExclusionsPanel /></div>
  {/if}

  {#if pb.error}
    <div class="banner error surface" role="alert">
      <CircleAlert size={18} />
      <span>{pb.error}</span>
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
                  <button class="btn small" class:primary={action.primary} disabled={pb.locked} onclick={action.run}>
                    <action.icon size={13} /> {action.label}
                  </button>
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
          {#if !pb.projects.length}
            <button class="btn small" disabled={pb.locked} onclick={() => pb.openEditor(null)}><Plus size={13} /> Add</button>
          {/if}
        </li>
        <li class:done={hasBackup}>
          <span class="step-mark">{#if hasBackup}<Check size={12} />{:else}3{/if}</span>
          <span class="step-text">
            <strong>Back up</strong>
            <small>Each zip is checked twice (after zipping and after the copy) before it is kept. Compare any two backups later.</small>
          </span>
          {#if hasDestination && pb.projects.length && !hasBackup}
            <button class="btn small primary" disabled={pb.locked} onclick={() => pb.backup(pb.projects.map((project) => project.id))}>
              <CloudUpload size={13} /> Back up now
            </button>
          {/if}
        </li>
      </ol>
    </section>
  {/if}

  <div class="toolbar">
    <button class="btn primary" disabled={pb.locked || !pb.projects.length}
      title={pb.query && pb.filtered.length !== pb.projects.length ? "Backs up every project, also those hidden by the search" : undefined}
      onclick={() => pb.backup(pb.projects.map((project) => project.id))}>
      <CloudUpload size={15} /> Back up all
    </button>
    <button class="btn" disabled={pb.locked} onclick={() => pb.openEditor(null)}><Plus size={15} /> Add project</button>
    {#if pb.projects.length > 1}
      <label class="search">
        <Search size={13} />
        <input bind:this={searchInput} bind:value={pb.query} class="input" type="search" placeholder="Search projects  /"
          aria-label="Search projects" aria-keyshortcuts="/ Control+F" />
      </label>
    {/if}
    <span class="spacer"></span>
    <button class="btn ghost" disabled={pb.locked} title="Bring in the projects of the Backup Projects app"
      onclick={() => pb.importProjects()}>
      <Download size={14} /> Import from Backup Projects
    </button>
  </div>

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
      {#if operation.totalFiles}
        <span class="numbers">{operation.doneFiles.toLocaleString()} / {operation.totalFiles.toLocaleString()} files</span>
      {/if}
      {#if operation.totalBytes}
        <span class="numbers">{formatBytes(operation.doneBytes)} / {formatBytes(operation.totalBytes)}{progress !== null ? ` · ${Math.floor(progress)}%` : ""}</span>
      {/if}
      {#if elapsed >= 2}<span class="numbers">{formatElapsed(elapsed)}</span>{/if}
      <button class="btn small" disabled={pb.cancelling} onclick={() => pb.cancel()}>
        {pb.cancelling ? "Cancelling…" : "Cancel"}
      </button>
      <span class="progress" class:indeterminate={progress === null} aria-hidden="true">
        <span style:width={progress === null ? undefined : `${progress}%`}></span>
      </span>
    </div>
  {/if}

  {#if pb.loading}
    <div class="loading" role="status"><LoaderCircle size={17} class="spin" /> Loading Project Backups…</div>
  {:else}
    <div class="project-list">
      {#each pb.filtered as project (project.id)}
        {@const missing = pb.isMissing(project)}
        {@const expanded = pb.expanded === project.id}
        {@const attempt = lastAttempt(project)}
        {@const age = pb.daysSinceBackup(project)}
        <article class="project surface" class:expanded>
          <div class="project-main">
            <span class="project-icon"><FolderCode size={18} /></span>
            <div class="project-text">
              <div class="title-row">
                <strong>{project.name}</strong>
                <span class="tag" title={`Backups are named ${project.appName}_D<day>_V<number>.zip`}>{project.appName}</span>
                {#if missing}<span class="tag warning">Folder not found</span>{/if}
                {#if project.closeApp}<span class="tag" title="Closed before each backup">Closes {project.closeApp}</span>{/if}
                {#if attempt && !attempt.cancelled}
                  <span class="tag danger" title={attempt.error ?? "The last backup failed"}><CircleX size={11} /> Last try failed {pb.ago(attempt.at)}</span>
                {:else if attempt?.cancelled}
                  <span class="tag" title="The last backup was cancelled">Cancelled {pb.ago(attempt.at)}</span>
                {:else if project.lastBackup && age !== null && age < 7}
                  <span class="tag ok" title="The last backup was checked and kept"><CircleCheck size={11} /> Up to date</span>
                {:else if age !== null && age >= 30}
                  <span class="tag warning" title="No backup for a month or more">{age} days old</span>
                {/if}
              </div>
              <span class="source selectable" title={project.sourcePath}>{project.sourcePath || "No folder chosen"}</span>
              <small>
                {#if project.lastBackup}
                  Last backup <span title={formatDate(project.lastBackup.createdAt)}>{pb.ago(project.lastBackup.createdAt)}</span> · {project.lastBackup.name} ·
                  {formatBytes(project.lastBackup.zipSize)} · {project.lastBackup.fileCount.toLocaleString()} files
                {:else}
                  Not backed up from MYLE yet
                {/if}
              </small>
            </div>
            <div class="project-actions">
              {#if missing}
                <button class="btn small" disabled={pb.locked} onclick={() => pb.openEditor(project, true)}>
                  <FolderSearch size={13} /> Find folder
                </button>
              {:else}
                <button class="btn small primary" disabled={pb.locked} onclick={() => pb.backup([project.id])}>
                  <CloudUpload size={13} /> Back up
                </button>
              {/if}
              <button class="btn small" aria-expanded={expanded} onclick={() => pb.toggleBackups(project.id)}>
                <Archive size={13} /> Backups <ChevronDown size={13} class={expanded ? "flip" : ""} />
              </button>
              <button class="icon-btn" title="Open the project folder" aria-label={`Open ${project.name}'s folder`}
                disabled={missing} onclick={() => pb.open("source", project.id)}><FolderOpen size={15} /></button>
              <button class="icon-btn" title="Edit" aria-label={`Edit ${project.name}`} disabled={pb.locked}
                onclick={() => pb.openEditor(project)}><Pencil size={14} /></button>
              <button class="icon-btn remove" title="Remove" aria-label={`Remove ${project.name}`} disabled={pb.locked}
                onclick={() => pb.removeProject(project)}><Trash2 size={14} /></button>
            </div>
          </div>

          {#if expanded}
            <div class="backups" transition:slide={{ duration: 160 }}>
              {#if pb.backupsLoading === project.id && !pb.backups[project.id]}
                <p class="muted"><LoaderCircle size={13} class="spin" /> Reading the backups…</p>
              {:else if !backupsOf(project).length}
                <p class="muted">No backups yet in {`${pb.page.backupRoot ?? "the backups folder"}\\${project.appName}`}.</p>
              {:else}
                <div class="backups-head">
                  <span>{backupsOf(project).length} {backupsOf(project).length === 1 ? "backup" : "backups"}</span>
                  <span class="spacer"></span>
                  <button class="btn small" disabled={pb.picked.length !== 2}
                    title="Tick two backups to compare them"
                    onclick={() => pb.openCompare(project.id, pb.picked[0], pb.picked[1])}>
                    <GitCompare size={13} /> Compare selected
                  </button>
                  <button class="icon-btn" title="Read the list again" aria-label="Read the list again"
                    onclick={() => pb.loadBackups(project.id)}>
                    <RefreshCw size={13} class={pb.backupsLoading === project.id ? "spin" : ""} />
                  </button>
                  <button class="icon-btn" title="Open in Explorer" aria-label="Open the project's backups folder"
                    onclick={() => pb.open("backups", project.id)}><FolderOpen size={14} /></button>
                </div>
                <ul class="backup-rows">
                  {#each backupsOf(project) as backup (backup.id)}
                    <li class:broken={backup.broken}>
                      <input class="check" type="checkbox" aria-label={`Select ${backup.name}`} disabled={backup.broken}
                        checked={pb.picked.includes(backup.id)} onchange={() => pb.togglePick(backup.id)} />
                      <span class="backup-name selectable">{backup.name}</span>
                      <span class="backup-meta">{backup.monthFolder}</span>
                      {#if backup.kind === "folder"}<span class="tag">Folder</span>{/if}
                      {#if backup.broken}<span class="tag warning" title="Too small to be a zip: a copy that never finished">Broken</span>{/if}
                      <span class="backup-meta right">{backup.size !== null ? formatBytes(backup.size) : ""}</span>
                      <span class="backup-meta date">{backup.modified ? formatDate(backup.modified) : ""}</span>
                      <button class="btn small ghost" disabled={backup.broken || missing}
                        title="Compare this backup with the project folder as it is now"
                        onclick={() => pb.openCompare(project.id, backup.id, SOURCE_ID)}>
                        <GitCompare size={13} /> With folder
                      </button>
                      <button class="icon-btn" title="Show in Explorer" aria-label={`Show ${backup.name} in Explorer`}
                        onclick={() => pb.open("backups", project.id, backup.id)}><FolderOpen size={13} /></button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        </article>
      {:else}
        {#if pb.projects.length}
          <div class="empty surface small">
            <strong>No project matches “{pb.query}”</strong>
            <p>The search looks at names, backup names and folders.</p>
            <div class="empty-actions"><button class="btn small" onclick={() => (pb.query = "")}><X size={13} /> Clear the search</button></div>
          </div>
        {:else}
        <div class="empty surface">
          <span class="empty-icon"><FolderCode size={25} /></span>
          <strong>No projects yet</strong>
          <p>Add a project folder, or bring in the projects you back up with the Backup Projects app.</p>
          <div class="empty-actions">
            <button class="btn primary" disabled={pb.locked} onclick={() => pb.openEditor(null)}><Plus size={14} /> Add project</button>
            <button class="btn" disabled={pb.locked} onclick={() => pb.importProjects()}><Download size={14} /> Import</button>
          </div>
        </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

{#if pb.editor}<ProjectEditor />{/if}
{#if pb.compare}<ComparePanel />{/if}

<style>
  .page-root { container-type: inline-size; }
  .top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .top :global(header) { margin-bottom: 14px; }
  .top-right { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 8px; margin-top: 3px; }
  .metrics { display: flex; align-items: center; gap: 5px; }
  .metric {
    display: inline-flex; align-items: center; gap: 5px; min-height: 27px; padding: 0 9px;
    border: 1px solid rgb(255 255 255 / 0.06); border-radius: 999px; background: rgb(255 255 255 / 0.025);
    color: var(--text-3); font-size: 10.75px; white-space: nowrap;
  }
  .metric :global(svg) { color: rgb(var(--accent-soft-rgb) / 0.72); }
  .metric strong { color: var(--text-2); font-weight: 600; font-variant-numeric: tabular-nums; }
  .settings-button.active { border-color: rgb(var(--accent-rgb) / 0.25); background: var(--selected); }

  .destination { display: grid; gap: 10px; margin-bottom: 12px; padding: 12px 13px; }
  .destination-head { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .destination-head h2 { flex: none; color: var(--text-2); font-size: 12px; font-weight: 600; }
  .root { flex: 1; min-width: 0; overflow: hidden; color: var(--text-3); font-family: var(--font-mono); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; text-align: right; }
  .cloud-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .cloud-tile {
    display: flex; align-items: center; gap: 9px; min-width: 0; min-height: 52px; padding: 9px 10px;
    border: 1px dashed rgb(255 255 255 / 0.12); border-radius: 9px; background: rgb(0 0 0 / 0.06);
    text-align: left; transition: background 140ms, border-color 140ms;
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
  .helper { color: var(--text-3); font-size: 11px; }

  .banner { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; padding: 11px 13px; font-size: 12px; }
  .banner.warning { border-color: rgb(245 176 65 / 0.22); color: rgb(245 188 95 / 0.85); }
  .banner.error { border-color: rgb(229 72 77 / 0.28); color: rgb(255 145 145 / 0.82); }
  .banner > span { flex: 1; }
  .failures-body { display: grid; flex: 1; gap: 6px; min-width: 0; }
  .failures ul { display: grid; gap: 6px; margin: 0; padding: 0 0 0 10px; border-left: 2px solid rgb(255 255 255 / 0.08); list-style: none; }
  .failures li { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; font-size: 11.5px; line-height: 1.45; }
  .failures li b { color: var(--text-1); font-weight: 600; }
  .failures li span { color: var(--text-2); overflow-wrap: anywhere; }

  .banner-actions { display: flex; flex: none; flex-wrap: wrap; gap: 6px; }
  .failure-actions { display: inline-flex; flex-wrap: wrap; gap: 5px; }
  .failures li .failure-actions { flex-basis: 100%; }

  .guide { display: grid; gap: 9px; margin-bottom: 12px; padding: 12px 13px; }
  .guide-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .guide-head h2 { color: var(--text-2); font-size: 12px; font-weight: 600; }
  .guide ol { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .guide li { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 7px 9px; border-radius: 9px; background: rgb(0 0 0 / 0.1); }
  .guide li.done { opacity: 0.62; }
  .step-mark {
    display: grid; place-items: center; flex: none; width: 21px; height: 21px; border-radius: 999px;
    border: 1px solid rgb(var(--accent-rgb) / 0.3); color: var(--accent); font-size: 11px; font-weight: 700;
  }
  .guide li.done .step-mark { border-color: rgb(62 207 142 / 0.35); color: #98dfbd; }
  .step-text { display: grid; flex: 1; gap: 1px; min-width: 0; }
  .step-text strong { font-size: 12px; font-weight: 600; }
  .step-text small { color: var(--text-3); font-size: 10.75px; line-height: 1.45; }

  .search { display: flex; align-items: center; gap: 6px; color: var(--text-3); }
  .search input { width: 210px; height: 30px; font-size: 12px; }

  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-bottom: 10px; }
  .spacer { flex: 1; }

  .operation {
    position: relative; display: flex; align-items: center; gap: 8px; min-width: 0; margin-bottom: 10px;
    padding: 7px 8px 9px; border-radius: 9px; background: rgb(var(--accent-rgb) / 0.045); color: var(--accent);
  }
  .operation-text { display: grid; flex: 1; gap: 1px; min-width: 0; }
  .operation-line { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; min-width: 0; }
  .operation-text strong { font-size: 11.5px; font-weight: 600; white-space: nowrap; }
  .hint-line { color: var(--text-3); }
  .operation-text small { overflow: hidden; color: var(--text-3); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
  .numbers { margin-left: auto; color: var(--text-2); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .numbers + .numbers { margin-left: 0; }
  .progress { position: absolute; right: 8px; bottom: 3px; left: 8px; height: 2px; overflow: hidden; border-radius: 999px; background: rgb(255 255 255 / 0.06); }
  .progress > span { display: block; height: 100%; border-radius: inherit; background: var(--accent-grad); transition: width var(--dur-med) var(--ease-out); }
  .progress.indeterminate > span { width: 30%; animation: sweep 1.3s var(--ease-in-out) infinite; }
  @keyframes sweep { from { transform: translateX(-100%); } to { transform: translateX(340%); } }

  .loading { display: flex; align-items: center; justify-content: center; gap: 8px; min-height: 180px; color: var(--text-3); font-size: 12.5px; }
  .project-list { display: grid; gap: 6px; }
  .project { display: grid; padding: 11px 12px; }
  .project-main { display: flex; align-items: center; gap: 11px; min-width: 0; }
  .project-icon {
    display: grid; place-items: center; flex: none; width: 36px; height: 36px; border-radius: 10px;
    border: 1px solid rgb(var(--accent-rgb) / 0.12); background: rgb(var(--accent-rgb) / 0.05); color: rgb(169 179 255 / 0.8);
  }
  .project-text { display: grid; flex: 1; gap: 2px; min-width: 0; }
  .title-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; min-width: 0; }
  .title-row strong { font-size: 13px; }
  .tag { padding: 1px 6px; border-radius: 5px; background: rgb(255 255 255 / 0.05); color: var(--text-3); font-size: 10px; white-space: nowrap; }
  .tag.warning { background: rgb(245 176 65 / 0.1); color: rgb(245 188 95 / 0.9); }
  .tag.danger { background: rgb(229 72 77 / 0.1); color: rgb(255 145 145 / 0.9); }
  .tag.ok { background: rgb(62 207 142 / 0.08); color: #98dfbd; }
  .tag { display: inline-flex; align-items: center; gap: 3px; }
  .source { overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
  .project-text small { overflow: hidden; color: var(--text-3); font-size: 10.75px; white-space: nowrap; text-overflow: ellipsis; }
  .project-actions { display: flex; align-items: center; flex-wrap: wrap; justify-content: flex-end; gap: 5px; }
  .project-actions :global(.flip) { transform: rotate(180deg); }
  .remove { color: rgb(255 145 145 / 0.68); }

  .backups { display: grid; gap: 7px; margin-top: 10px; padding-top: 10px; border-top: 1px solid rgb(255 255 255 / 0.055); }
  .backups-head { display: flex; align-items: center; gap: 6px; color: var(--text-3); font-size: 11px; }
  .muted { display: flex; align-items: center; gap: 6px; color: var(--text-3); font-size: 11.5px; }
  .backup-rows { display: grid; gap: 3px; max-height: 320px; margin: 0; padding: 0; overflow: auto; list-style: none; }
  .backup-rows li {
    display: flex; align-items: center; gap: 8px; min-width: 0; padding: 4px 6px;
    border-radius: 7px; background: rgb(0 0 0 / 0.1); font-size: 11.5px;
  }
  .backup-rows li.broken { opacity: 0.65; }
  .backup-name { min-width: 0; overflow: hidden; color: var(--text-1); font-family: var(--font-mono); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; }
  .backup-meta { color: var(--text-3); font-size: 10.5px; white-space: nowrap; }
  .backup-meta.right { margin-left: auto; font-variant-numeric: tabular-nums; }

  .empty.small { padding: 22px 20px; }
  .empty { display: grid; justify-items: center; gap: 7px; padding: 42px 20px; text-align: center; }
  .empty-icon {
    display: grid; place-items: center; width: 50px; height: 50px; margin-bottom: 2px;
    border: 1px solid rgb(var(--accent-rgb) / 0.12); border-radius: 15px; background: rgb(var(--accent-rgb) / 0.045); color: rgb(169 179 255 / 0.7);
  }
  .empty strong { font-size: 13.5px; }
  .empty p { max-width: 440px; color: var(--text-3); font-size: 11.5px; }
  .empty-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 8px; }

  button:disabled { opacity: 0.45; pointer-events: none; }

  @container (max-width: 760px) {
    .project-main { flex-wrap: wrap; }
    .project-actions { width: 100%; justify-content: flex-start; }
    .backup-meta.date { display: none; }
    .operation { flex-wrap: wrap; }
    .search input { width: 160px; }
  }
  @container (max-width: 470px) {
    .cloud-list { grid-template-columns: minmax(0, 1fr); }
    .backup-meta { display: none; }
  }
</style>
