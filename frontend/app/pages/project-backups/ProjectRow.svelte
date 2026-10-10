<script lang="ts">
  import { slide } from "svelte/transition";
  import Archive from "@lucide/svelte/icons/archive";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import Clock from "@lucide/svelte/icons/clock";
  import CloudUpload from "@lucide/svelte/icons/cloud-upload";
  import FolderCode from "@lucide/svelte/icons/folder-code";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import GitCompare from "@lucide/svelte/icons/git-compare";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Pencil from "@lucide/svelte/icons/pencil";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { SOURCE_ID, type Project } from "./api";
  import { formatBytes, formatDate, projectBackupsState as pb } from "./state.svelte";

  let { project }: { project: Project } = $props();

  const health = $derived(pb.health(project));
  const missing = $derived(health === "missing");
  const expanded = $derived(pb.expanded === project.id);
  const failure = $derived(pb.lastFailure(project));
  const backups = $derived(pb.backups[project.id] ?? []);
  const age = $derived(pb.daysSinceBackup(project));
  const change = $derived(pb.changes[project.id]);
  const checkingNow = $derived(pb.checking.includes(project.id));
  let filesOpen = $state(false);
  const changeCount = $derived(change ? change.added + change.modified + change.deleted : 0);

  /** The badge next to the name: what happened, not just a colour. */
  const badge = $derived.by(() => {
    switch (health) {
      case "missing":
        return { tone: "warn", label: "Folder not found", title: project.sourcePath || "No folder chosen yet", icon: FolderSearch };
      case "failed":
        return {
          tone: "danger",
          label: `Last try failed ${pb.ago(failure?.at ?? Date.now())}`,
          title: failure?.error ?? "The last backup failed",
          icon: CircleX,
        };
      case "cancelled":
        return { tone: "quiet", label: `Cancelled ${pb.ago(failure?.at ?? Date.now())}`, title: "The last backup was cancelled", icon: Clock };
      case "never":
        return { tone: "warn", label: "Never backed up", title: "This project has no backup yet", icon: CircleAlert };
      case "stale":
        return { tone: "warn", label: `${age} days old`, title: "The last backup is a month old or more", icon: Clock };
      default:
        return { tone: "ok", label: "Up to date", title: "The last backup was checked and kept", icon: CircleCheck };
    }
  });
</script>

<article class="project surface" class:expanded class:missing class:failed={health === "failed"}>
  <div class="project-main">
    <span class="project-icon {badge.tone}"><FolderCode size={18} /></span>
    <div class="project-text">
      <div class="title-row">
        <strong>{project.name}</strong>
        <span class="tag {badge.tone}" title={badge.title}>
          <badge.icon size={11} />
          {badge.label}
        </span>
        {#if checkingNow && !change}
          <span class="tag quiet"><LoaderCircle size={11} class="spin" /> Checking…</span>
        {:else if change?.state === "changed"}
          <button class="tag warn change" title="Show what changed since the last backup" aria-expanded={filesOpen} onclick={() => (filesOpen = !filesOpen)}>
            <CircleAlert size={11} /> Changes detected · {changeCount}
            <ChevronDown size={11} class={filesOpen ? "flip" : ""} />
          </button>
        {:else if change?.state === "upToDate"}
          <span class="tag ok" title={`Same as ${change.backupId ?? "the last backup"}, checked ${pb.ago(change.checkedAt)}`}><CircleCheck size={11} /> No changes</span>
        {/if}
        <span class="tag" title={`Backups are named ${project.appName}_D<day>_V<number>.zip`}>{project.appName}</span>
        {#if project.closeApp}<span class="tag" title="Closed before each backup">Closes {project.closeApp}</span>{/if}
      </div>
      <span class="source selectable" title={project.sourcePath}>{project.sourcePath || "No folder chosen"}</span>
      <small>
        {#if project.lastBackup}
          Last backup <span title={formatDate(project.lastBackup.createdAt)}>{pb.ago(project.lastBackup.createdAt)}</span> ·
          {project.lastBackup.name} · {formatBytes(project.lastBackup.zipSize)} ·
          {project.lastBackup.fileCount.toLocaleString()} files
        {:else}
          No backup from MYLE yet
        {/if}
      </small>
    </div>
    <div class="project-actions">
      {#if missing}
        <button class="btn small primary" disabled={pb.locked} onclick={() => pb.openEditor(project, true)}>
          <FolderSearch size={13} /> Find folder
        </button>
      {:else}
        <button class="btn small primary" disabled={pb.locked} onclick={() => pb.backup([project.id])}>
          <CloudUpload size={13} /> Back up
        </button>
      {/if}
      <button class="btn small" aria-expanded={expanded} onclick={() => pb.toggleBackups(project.id)}>
        <Archive size={13} /> {backups.length ? `${backups.length} backups` : "Backups"}
        <ChevronDown size={13} class={expanded ? "flip" : ""} />
      </button>
      <button
        class="icon-btn"
        title="Open the project folder"
        aria-label={`Open ${project.name}'s folder`}
        disabled={missing}
        onclick={() => pb.open("source", project.id)}><FolderOpen size={15} /></button
      >
      <button class="icon-btn" title="Edit" aria-label={`Edit ${project.name}`} disabled={pb.locked} onclick={() => pb.openEditor(project)}>
        <Pencil size={14} />
      </button>
      <button
        class="icon-btn remove"
        title="Remove"
        aria-label={`Remove ${project.name}`}
        disabled={pb.locked}
        onclick={() => pb.removeProject(project)}><Trash2 size={14} /></button
      >
    </div>
  </div>

  {#if filesOpen && change?.state === "changed"}
    <div class="changes" transition:slide={{ duration: 160 }}>
      <p class="muted">
        Since {change.backupId ?? "the last backup"}: {change.added} new · {change.modified} modified · {change.deleted} deleted
      </p>
      <ul>
        {#each change.files as f (f.path + f.status)}
          <li class="c-{f.status}"><b>{f.status === "added" ? "+" : f.status === "deleted" ? "−" : "~"}</b><span class="selectable" title={f.path}>{f.path}</span>{#if f.size !== null}<small>{formatBytes(f.size)}</small>{/if}</li>
        {/each}
      </ul>
      {#if changeCount > change.files.length}<p class="muted">…and {changeCount - change.files.length} more</p>{/if}
    </div>
  {/if}

  {#if expanded}
    <div class="backups" transition:slide={{ duration: 160 }}>
      {#if pb.backupsLoading === project.id && !pb.backups[project.id]}
        <p class="muted"><LoaderCircle size={13} class="spin" /> Reading the backups…</p>
      {:else if !backups.length}
        <p class="muted">No backups yet in {`${pb.page.backupRoot ?? "the backups folder"}\\${project.appName}`}.</p>
      {:else}
        <div class="backups-head">
          <span>{backups.length} {backups.length === 1 ? "backup" : "backups"} kept</span>
          <span class="spacer"></span>
          <button
            class="btn small"
            disabled={pb.picked.length !== 2}
            title="Tick two backups to compare them"
            onclick={() => pb.openCompare(project.id, pb.picked[0], pb.picked[1])}>
            <GitCompare size={13} /> Compare selected
          </button>
          <button class="icon-btn" title="Read the list again" aria-label="Read the list again" onclick={() => pb.loadBackups(project.id)}>
            <RefreshCw size={13} class={pb.backupsLoading === project.id ? "spin" : ""} />
          </button>
          <button
            class="icon-btn"
            title="Open in Explorer"
            aria-label="Open the project's backups folder"
            onclick={() => pb.open("backups", project.id)}><FolderOpen size={14} /></button
          >
        </div>
        <ul class="backup-rows">
          {#each backups as backup (backup.id)}
            <li class:broken={backup.broken}>
              <input
                class="check"
                type="checkbox"
                aria-label={`Select ${backup.name}`}
                disabled={backup.broken}
                checked={pb.picked.includes(backup.id)}
                onchange={() => pb.togglePick(backup.id)}
              />
              <span class="backup-name selectable">{backup.name}</span>
              <span class="backup-meta">{backup.monthFolder}</span>
              {#if backup.kind === "folder"}<span class="tag">Folder</span>{/if}
              {#if backup.broken}<span class="tag warn" title="Too small to be a zip: a copy that never finished">Broken</span>{/if}
              <span class="backup-meta right">{backup.size !== null ? formatBytes(backup.size) : ""}</span>
              <span class="backup-meta date">{backup.modified ? formatDate(backup.modified) : ""}</span>
              <button
                class="btn small ghost"
                disabled={backup.broken || missing}
                title="Compare this backup with the project folder as it is now"
                onclick={() => pb.openCompare(project.id, backup.id, SOURCE_ID)}>
                <GitCompare size={13} /> With folder
              </button>
              <button
                class="icon-btn"
                title="Show in Explorer"
                aria-label={`Show ${backup.name} in Explorer`}
                onclick={() => pb.open("backups", project.id, backup.id)}><FolderOpen size={13} /></button
              >
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</article>

<style>
  .project {
    display: grid;
    padding: 11px 12px;
    transition: border-color var(--dur-fast);
  }

  .project.missing {
    border-color: rgb(245 176 65 / 0.16);
  }

  .project.failed {
    border-color: rgb(229 72 77 / 0.18);
  }

  .project-main {
    display: flex;
    align-items: center;
    gap: 11px;
    min-width: 0;
  }

  .project-icon {
    display: grid;
    place-items: center;
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    border: 1px solid rgb(var(--accent-rgb) / 0.12);
    background: rgb(var(--accent-rgb) / 0.05);
    color: rgb(169 179 255 / 0.8);
  }

  /* The badge's colour on the icon too, as in the GitHub Releases list
     (lost when the stash conflict was resolved). */
  .project-icon.ok {
    border-color: rgb(62 207 142 / 0.28);
    background: rgb(62 207 142 / 0.08);
    color: #6fdba5;
  }

  .project-icon.warn {
    border-color: rgb(255 198 107 / 0.3);
    background: rgb(255 198 107 / 0.08);
    color: #ffc66b;
  }

  .project-icon.danger {
    border-color: rgb(255 143 143 / 0.3);
    background: rgb(255 143 143 / 0.08);
    color: #ff8f8f;
  }

  .project-text {
    display: grid;
    flex: 1;
    gap: 2px;
    min-width: 0;
  }

  .title-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    min-width: 0;
  }

  .title-row strong {
    font-size: 13px;
  }

  .tag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border-radius: 5px;
    background: rgb(255 255 255 / 0.05);
    color: var(--text-3);
    font-size: 10px;
    white-space: nowrap;
  }

  .tag.warn {
    background: rgb(245 176 65 / 0.1);
    color: rgb(245 188 95 / 0.9);
  }

  .tag.danger {
    background: rgb(229 72 77 / 0.1);
    color: rgb(255 145 145 / 0.9);
  }

  .tag.ok {
    background: rgb(62 207 142 / 0.08);
    color: #98dfbd;
  }

  .source {
    overflow: hidden;
    color: var(--text-2);
    font-family: var(--font-mono);
    font-size: 10.5px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .project-text small {
    overflow: hidden;
    color: var(--text-3);
    font-size: 10.75px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .project-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 5px;
  }

  .project-actions :global(.flip) {
    transform: rotate(180deg);
  }

  .remove {
    color: rgb(255 145 145 / 0.68);
  }

  .backups {
    display: grid;
    gap: 7px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid rgb(255 255 255 / 0.055);
  }

  .backups-head {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
    font-size: 11px;
  }

  .spacer {
    flex: 1;
  }

  .muted {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
    font-size: 11.5px;
  }

  .backup-rows {
    display: grid;
    gap: 3px;
    max-height: 320px;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  .backup-rows li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 4px 6px;
    border-radius: 7px;
    background: rgb(0 0 0 / 0.1);
    font-size: 11.5px;
  }

  .backup-rows li.broken {
    opacity: 0.65;
  }

  .backup-name {
    min-width: 0;
    overflow: hidden;
    color: var(--text-1);
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .backup-meta {
    color: var(--text-3);
    font-size: 10.5px;
    white-space: nowrap;
  }

  .backup-meta.right {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }

  button:disabled {
    opacity: 0.45;
    pointer-events: none;
  }

  @container (max-width: 760px) {
    .project-main {
      flex-wrap: wrap;
    }

    .project-actions {
      width: 100%;
      justify-content: flex-start;
    }

    .backup-meta.date {
      display: none;
    }
  }

  @container (max-width: 470px) {
    .backup-meta {
      display: none;
    }
  }

  .tag.change {
    border: 0;
    cursor: pointer;
    font: inherit;
  }

  .changes {
    display: grid;
    gap: 4px;
    margin-top: 8px;
    padding: 8px 10px;
    border-top: 1px solid var(--btn-border);
  }

  .changes ul {
    display: grid;
    max-height: 220px;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
    font-size: 11.5px;
  }

  .changes li {
    display: grid;
    grid-template-columns: 14px minmax(0, 1fr) auto;
    gap: 6px;
    padding: 1px 0;
  }

  .changes li span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .changes small {
    color: var(--text-3);
  }

  .c-added b {
    color: #6fd3a0;
  }

  .c-modified b {
    color: #e8c46a;
  }

  .c-deleted b {
    color: #f08a8d;
  }
</style>
