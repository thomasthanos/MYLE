<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import Archive from "@lucide/svelte/icons/archive";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Plus from "@lucide/svelte/icons/plus";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CloudOff from "@lucide/svelte/icons/cloud-off";
  import Database from "@lucide/svelte/icons/database";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import CustomGameModal from "./CustomGameModal.svelte";
  import GameSaveRow from "./GameSaveRow.svelte";
  import GameSavesToolbar from "./GameSavesToolbar.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";
  import SnapshotPicker from "./SnapshotPicker.svelte";
  import type { GameFailure, GameSavesTab } from "./api";
  import { formatBytes, formatDate, gameSavesState as state } from "./state.svelte";
  onMount(() => {
    void state.init();
  });
  /** Games that failed for the same reason share it: said once, not per game. */
  function byReason(items: GameFailure[]) {
    const groups = new Map<string, GameFailure[]>();
    for (const item of items) {
      const group = groups.get(item.reason);
      if (group) group.push(item);
      else groups.set(item.reason, [item]);
    }
    return [...groups].map(([reason, games]) => ({ reason, games }));
  }
  // Each new scan result may list games whose covers are not known yet.
  $effect(() => {
    if (state.scanResult) void state.loadCovers();
  });
  const tabs: { id: GameSavesTab; label: string; icon: typeof HardDrive }[] = [
    { id: "pc", label: "On this PC", icon: HardDrive },
    { id: "backup", label: "Your backups", icon: Archive },
  ];
  function tabKeydown(event: KeyboardEvent, current: GameSavesTab) {
    const index = tabs.findIndex((tab) => tab.id === current);
    const key = event.key;
    let next = index;
    if (key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
    else if (key === "Home") next = 0;
    else if (key === "End") next = tabs.length - 1;
    else return;
    event.preventDefault();
    state.setTab(tabs[next].id);
    requestAnimationFrame(() => document.getElementById(`game-saves-tab-${tabs[next].id}`)?.focus());
  }
</script>
<div class="page-root" inert={state.customDialogOpen || state.restoreDialogOpen}>
  <div class="top">
    <PageHeader
      title="Game Saves"
      subtitle="Back up your progress and restore it when you need."
    />
    <div class="top-right">
      <div class="metrics" aria-label="Game save summary">
        <span class="metric"><Gamepad2 size={13} /><strong>{state.localGames.toLocaleString()}</strong> games</span>
        <span class="metric"><HardDrive size={13} /><strong>{formatBytes(state.totalBytes)}</strong></span>
        <span class="metric database" title={state.page.engineVersion ? `Database engine ${state.page.engineVersion}` : "Game database"}>
          <Database size={13} /><strong>{state.databaseGames.toLocaleString()}</strong> games in database
        </span>
      </div>
      <button
        class="btn settings-button"
        class:active={state.settingsOpen}
        aria-expanded={state.settingsOpen}
        aria-controls="game-saves-settings"
        onclick={() => state.toggleSettings()}
      >
        <Settings2 size={15} /> Backup settings
      </button>
    </div>
  </div>
  {#if state.scanResult && state.page.engineAvailable}
    <div class="overview surface" aria-label="Backup overview" data-no-tooltip data-no-copy>
      <div class="overview-item pending">
        <Archive size={16} /><span><strong>{state.pendingGames.length}</strong> need backup</span>
      </div>
      <div class="overview-item protected">
        <ShieldCheck size={16} /><span>{#if state.scanResult.backupUnreachable}Backups offline{:else}<strong>{state.protectedGames}</strong> up to date{/if}</span>
      </div>
      <div class="overview-item location">
        <FolderOpen size={16} /><span class="folder-path">{state.page.settings.backupFolder ?? "No backup folder selected"}</span><span class="schedule-label">{state.page.settings.schedule === "off" ? "Manual" : state.page.settings.schedule === "daily" ? "Daily" : "Weekly"}</span>
      </div>
    </div>
  {/if}
  {#if state.settingsOpen}
    <div transition:slide={{ duration: 190 }}><SettingsPanel /></div>
  {/if}
  {#if !state.page.engineAvailable && !state.loading}
    <div class="banner warning surface" role="alert">
      <CircleAlert size={18} />
      <span>
        <strong>Game Saves engine is unavailable.</strong>
        Install or repair the bundled save database engine, then restart the app.
      </span>
    </div>
  {/if}
  {#if state.scanResult?.backupUnreachable && !state.error}
    <div class="banner warning surface" role="status">
      <CloudOff size={18} />
      <div class="unreachable-body">
        <strong>Your backups are out of reach</strong>
        <p>
          <code class="selectable">{state.scanResult.backupUnreachable}</code> is on a drive that is not connected. If it is
          in Google Drive, OneDrive or another cloud app, start the app. The saves on this PC are shown; their backups show
          again once the folder is back.
        </p>
      </div>
      <button class="btn small ghost" disabled={state.locked} onclick={() => state.refresh()}>
        <RefreshCw size={13} /> Try again
      </button>
    </div>
  {/if}
  {#if state.error}
    <div class="banner error surface" role="alert">
      <CircleAlert size={18} />
      <span>{state.error}</span>
      <button class="btn small ghost" disabled={state.locked} onclick={() => state.scanResult ? state.refresh() : state.scan(false)}><RefreshCw size={13} /> Retry scan</button>
    </div>
  {/if}
  {#if state.failures}
    {@const failures = state.failures}
    <div class="banner warning surface failures" role="alert">
      <CircleAlert size={18} />
      <div class="failures-body">
        <strong>
          {failures.items.length}
          {failures.items.length === 1 ? "game" : "games"} could not be {failures.kind === "backup" ? "backed up" : "restored"}
        </strong>
        <div class="failure-groups">
          {#each byReason(failures.items) as group (group.reason)}
            <section>
              <p class="selectable">{group.reason}</p>
              <ul>
                {#each group.games as failure (failure.game)}
                  <li>
                    <b>{failure.game}</b>
                    {#if failure.file}<span class="selectable file">{failure.file}</span>{/if}
                  </li>
                {/each}
              </ul>
            </section>
          {/each}
        </div>
      </div>
      <button class="btn small ghost" onclick={() => state.dismissFailures()}>Dismiss</button>
    </div>
  {/if}
  <div class="tabs-wrap">
    <div class="tabs" role="tablist" aria-label="Game save location">
      {#each tabs as tab (tab.id)}
        {@const Icon = tab.icon}
        <button
          id={`game-saves-tab-${tab.id}`}
          class="tab"
          class:active={state.tab === tab.id}
          role="tab"
          aria-selected={state.tab === tab.id}
          aria-controls={`game-saves-panel-${tab.id}`}
          disabled={state.selectionLocked}
          tabindex={state.tab === tab.id ? 0 : -1}
          onclick={() => state.setTab(tab.id)}
          onkeydown={(event) => tabKeydown(event, tab.id)}
        >
          <Icon size={15} />
          {tab.label}
          <span class="count">{tab.id === "pc" ? state.localGames : state.backupGames}</span>
        </button>
      {/each}
    </div>
    <p class="tab-hint">
      {state.tab === "pc"
        ? "Saves currently detected on this computer."
        : "Snapshots kept in your backup folder, including games no longer installed."}
    </p>
  </div>
  {#if state.page.undoRestore}
    <div class="undo surface">
      <span class="undo-icon"><ShieldCheck size={16} /></span>
      <span class="undo-text">
        <strong>Safety copy from {formatDate(state.page.undoRestore.createdAt)}</strong>
        <small>
          {state.page.undoRestore.games.length} {state.page.undoRestore.games.length === 1 ? "game" : "games"} ·
          available until {formatDate(state.page.undoRestore.expiresAt)}
        </small>
      </span>
      <button class="btn" disabled={state.locked} onclick={() => state.undoLastRestore()}>
        <Undo2 size={14} /> Undo last restore
      </button>
    </div>
  {/if}
  {#if state.loading && !state.scanResult}
    <div class="loading" role="status"><LoaderCircle size={17} class="spin" /> Loading Game Saves…</div>
  {:else if state.page.engineAvailable}
    <div
      id={`game-saves-panel-${state.tab}`}
      role="tabpanel"
      aria-labelledby={`game-saves-tab-${state.tab}`}
      tabindex="0"
    >
      <GameSavesToolbar />
      <div
        class="game-list"
        class:locked={state.listLocked}
        class:quietly={state.operation?.background && !state.discovering}
        inert={state.listLocked}
        aria-busy={state.listLocked}
      >
        {#each state.visibleGames as game (game.id)}
          <GameSaveRow {game} tab={state.tab} />
        {:else}
          <div class="empty surface">
            <span class="empty-icon">{#if state.tab === "pc"}<Gamepad2 size={25} />{:else}<Archive size={25} />{/if}</span>
            <strong>{state.query || state.filter !== "all" ? "No games match these filters" : state.tab === "pc" ? "No save files found yet" : "Your backup is empty"}</strong>
            <p>
              {state.query || state.filter !== "all"
                ? "Try another search or choose All statuses."
                : state.tab === "pc"
                  ? "Scan again after adding install folders in Backup settings."
                  : "Select games on the On this PC tab and create your first backup."}
            </p>
            {#if state.query || state.filter !== "all"}
              <button class="btn" onclick={() => { state.setQuery(""); state.setFilter("all"); }}>Clear filters</button>
            {:else if state.tab === "pc"}
              <div class="empty-actions">
                <button class="btn primary" disabled={state.locked} onclick={() => state.scan()}><RefreshCw size={14} /> Find games</button>
                <button class="btn" disabled={state.locked} onclick={() => state.openCustomDialog()}><Plus size={14} /> Add a game</button>
              </div>
            {:else}
              <button class="btn primary" disabled={state.locked} onclick={() => state.setTab("pc")}><Gamepad2 size={14} /> Choose games to back up</button>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>
{#if state.customDialogOpen}<CustomGameModal />{/if}
{#if state.restoreDialogOpen}<SnapshotPicker />{/if}
<style>
  .page-root {
    container-type: inline-size;
  }
  .overview { display: grid; grid-template-columns: auto auto minmax(0, 1fr); margin: 0 0 12px; padding: 3px; user-select: none; }
  .overview-item { display: flex; align-items: center; gap: 9px; min-width: 0; min-height: 38px; padding: 7px 16px; color: var(--text-2); font-size: 12px; cursor: default; }
  .overview-item.location { margin-left: auto; width: 100%; }
  .overview-item + .overview-item { border-left: 1px solid rgb(255 255 255 / .06); }
  .overview-item > :global(svg) { color: var(--accent); flex: none; }
  .protected > :global(svg) { color: #5dd6a4; }
  .overview-item strong { color: var(--text-1); font-variant-numeric: tabular-nums; }
  .folder-path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); font-size: 11px; user-select: none; }
  .schedule-label { margin-left: auto; padding: 3px 7px; border-radius: 5px; background: rgb(var(--accent-rgb) / .1); color: var(--accent); font-size: 10px; }
  .top :global(header) { margin-bottom: 14px; }
  .empty-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 8px; }
  .top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .top-right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 3px;
  }
  .metrics {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 5px;
  }
  .metric {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 27px;
    padding: 0 9px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 999px;
    background: rgb(255 255 255 / 0.025);
    color: var(--text-3);
    font-size: 10.75px;
    white-space: nowrap;
  }
  .metric :global(svg) {
    color: rgb(var(--accent-soft-rgb) / 0.72);
  }
  .metric strong {
    color: var(--text-2);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .metric.database {
    border-color: rgb(79 209 232 / 0.085);
  }
  .settings-button.active {
    border-color: rgb(var(--accent-rgb) / 0.25);
    background: var(--selected);
  }
  .banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 14px;
    padding: 11px 13px;
    font-size: 12px;
  }
  .banner span {
    display: grid;
  }
  .banner.warning {
    border-color: rgb(245 176 65 / 0.22);
    color: rgb(245 188 95 / 0.85);
  }
  .banner.error {
    border-color: rgb(229 72 77 / 0.28);
    color: rgb(255 145 145 / 0.82);
  }
  .unreachable-body {
    display: grid;
    flex: 1;
    gap: 4px;
    min-width: 0;
  }
  .unreachable-body p {
    margin: 0;
    color: var(--text-2);
    line-height: 1.5;
  }
  .unreachable-body code {
    color: rgb(245 188 95 / 0.9);
    font-family: var(--font-mono);
    font-size: 11.5px;
    word-break: break-all;
  }
  .failures-body {
    display: grid;
    flex: 1;
    gap: 6px;
    min-width: 0;
  }
  .failure-groups {
    display: grid;
    gap: 10px;
    max-height: 184px;
    overflow: auto;
  }
  .failure-groups section {
    display: grid;
    gap: 4px;
  }
  .failure-groups p {
    margin: 0;
    color: var(--text-2);
    font-size: 11.5px;
    line-height: 1.45;
  }
  .failures ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0 0 0 10px;
    border-left: 2px solid rgb(255 255 255 / 0.08);
    list-style: none;
  }
  .failures li {
    display: flex;
    gap: 8px;
    min-width: 0;
    font-size: 11.5px;
    line-height: 1.45;
  }
  .failures li b {
    flex: none;
    color: var(--text-1);
    font-weight: 600;
  }
  .failures li .file {
    overflow: hidden;
    color: var(--text-3);
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 10.5px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .tabs-wrap {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 10px;
    border-bottom: 1px solid rgb(255 255 255 / 0.055);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 38px;
    padding: 0 12px;
    color: var(--text-3);
    font-size: 12.5px;
    font-weight: 500;
  }
  .tab:hover {
    color: var(--text-2);
  }
  .tab.active {
    color: var(--text-1);
  }
  .tab.active::after {
    content: "";
    position: absolute;
    right: 9px;
    bottom: -1px;
    left: 9px;
    height: 2px;
    border-radius: 2px 2px 0 0;
    background: var(--accent-grad);
    box-shadow: 0 0 9px var(--accent-glow);
  }
  .count {
    min-width: 20px;
    padding: 1px 6px;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.05);
    color: var(--text-3);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
  .tab-hint {
    margin-left: auto;
    color: var(--text-3);
    font-size: 10.75px;
  }
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 180px;
    color: var(--text-3);
    font-size: 12.5px;
  }
  .undo {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-bottom: 10px;
    padding: 9px 10px;
    border-color: rgb(62 207 142 / 0.12);
    background: linear-gradient(90deg, rgb(62 207 142 / 0.04), rgb(255 255 255 / 0.018));
  }
  .undo-icon {
    display: grid;
    color: rgb(92 218 166 / 0.8);
  }
  .undo-text {
    display: grid;
    flex: 1;
    min-width: 0;
  }
  .undo-text strong {
    font-size: 11.5px;
  }
  .undo-text small {
    overflow: hidden;
    color: var(--text-3);
    font-size: 10.5px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  [role="tabpanel"]:focus {
    outline: none;
  }
  .game-list {
    display: grid;
    gap: 5px;
    transition: opacity var(--dur-med) var(--ease-out);
  }
  .game-list.locked {
    opacity: 0.55;
    cursor: progress;
  }
  /* The app's own check every ten minutes: a couple of seconds, barely seen. */
  .game-list.locked.quietly {
    opacity: 0.85;
  }
  .game-list > :global(article) {
    content-visibility: auto;
    contain-intrinsic-size: auto 92px;
  }
  .game-list > :global(article:focus-within) {
    content-visibility: visible;
  }
  .empty {
    display: grid;
    justify-items: center;
    gap: 7px;
    padding: 42px 20px;
    text-align: center;
  }
  .empty-icon {
    display: grid;
    place-items: center;
    width: 50px;
    height: 50px;
    margin-bottom: 2px;
    border: 1px solid rgb(var(--accent-rgb) / 0.12);
    border-radius: 15px;
    background: rgb(var(--accent-rgb) / 0.045);
    color: rgb(169 179 255 / 0.7);
  }
  .empty strong {
    font-size: 13.5px;
  }
  .empty p {
    max-width: 440px;
    color: var(--text-3);
    font-size: 11.5px;
  }
  @container (max-width: 760px) {
    .overview { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .overview-item.location { grid-column: 1 / -1; border-left: 0; border-top: 1px solid rgb(255 255 255 / .06); }
    .top-right {
      width: 100%;
      justify-content: flex-start;
      margin-top: -16px;
      margin-bottom: 20px;
    }
    .metrics {
      justify-content: flex-start;
    }
    .tabs-wrap {
      align-items: flex-end;
    }
    .tab-hint {
      display: none;
    }
  }
  @container (max-width: 470px) {
    .overview { grid-template-columns: minmax(0, 1fr); }
    .overview-item + .overview-item { border-left: 0; border-top: 1px solid rgb(255 255 255 / .06); }
    .metric.database {
      display: none;
    }
    .settings-button {
      width: 100%;
    }
    .tabs,
    .tab {
      flex: 1;
    }
    .tab {
      justify-content: center;
      padding-inline: 7px;
    }
  }
</style>
