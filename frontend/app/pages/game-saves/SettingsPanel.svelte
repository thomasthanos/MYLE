<script lang="ts">
  import Archive from "@lucide/svelte/icons/archive";
  import Clock3 from "@lucide/svelte/icons/clock-3";
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import Check from "@lucide/svelte/icons/check";
  import Cloud from "@lucide/svelte/icons/cloud";
  import CloudOff from "@lucide/svelte/icons/cloud-off";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { CLOUD_PROVIDERS, type BackupSchedule, type CloudProvider, type DetectedFolder, type RootStore, type ScheduleWeekday } from "./api";
  import Select, { type SelectOption } from "../../../lib/components/Select.svelte";
  import CloudLogo from "../../../lib/components/CloudLogo.svelte";
  import { formatDate, gameSavesState as gs, samePath } from "./state.svelte";
  const scheduleOptions: SelectOption<BackupSchedule>[] = [{ value: "off", label: "Off" }, { value: "daily", label: "Daily" }, { value: "weekly", label: "Weekly" }];
  const storeOptions: SelectOption<RootStore>[] = [
    { value: "steam", label: "Steam" }, { value: "epic", label: "Epic Games" }, { value: "gog", label: "GOG" }, { value: "gogGalaxy", label: "GOG Galaxy" },
    { value: "uplay", label: "Ubisoft Connect" }, { value: "origin", label: "EA app / Origin" }, { value: "otherWindows", label: "Other folder" },
  ];
  const weekdays: SelectOption<ScheduleWeekday>[] = [
    { value: "monday", label: "Monday" }, { value: "tuesday", label: "Tuesday" }, { value: "wednesday", label: "Wednesday" }, { value: "thursday", label: "Thursday" },
    { value: "friday", label: "Friday" }, { value: "saturday", label: "Saturday" }, { value: "sunday", label: "Sunday" },
  ];
  const BASE_TIMES = Array.from({ length: 48 }, (_, i) => `${String(Math.floor(i / 2)).padStart(2, "0")}:${i % 2 ? "30" : "00"}`);
  const timeOptions = $derived.by((): SelectOption<string>[] => {
    const current = gs.page.settings.scheduleTime;
    const times = current && !BASE_TIMES.includes(current) ? [...BASE_TIMES, current].sort() : BASE_TIMES;
    return times.map((time) => ({ value: time, label: time }));
  });
  /** One tile per detected folder (a provider can have several accounts), and
   *  one for each provider not found, which asks where its folder is. */
  interface CloudTile { key: string; provider: CloudProvider; name: string; folder: DetectedFolder | null }
  const cloudTiles = $derived.by(() =>
    CLOUD_PROVIDERS.flatMap(({ id, name }): CloudTile[] => {
      const found = gs.page.cloudFolders.filter((folder) => folder.provider === id);
      return found.length ? found.map((folder) => ({ key: folder.path, provider: id, name: folder.label, folder })) : [{ key: id, provider: id, name, folder: null }];
    }),
  );
  const storeLabel = (store: RootStore) => storeOptions.find((item) => item.value === store)?.label ?? store;
  let newRootStore = $state<RootStore>("steam");
  const scheduledFailure = $derived(
    gs.page.settings.lastScheduledResult?.error ?? (gs.page.settings.lastScheduledResult?.failedGames.length ? `Failed: ${gs.page.settings.lastScheduledResult.failedGames.join(", ")}` : null),
  );
  const lastAttemptFailed = $derived(
    scheduledFailure !== null ||
      (gs.page.settings.lastScheduledAttempt !== null && (gs.page.settings.lastScheduledSuccess === null || gs.page.settings.lastScheduledAttempt > gs.page.settings.lastScheduledSuccess)),
  );
  const settingsLocked = $derived(gs.locked);
  const scheduleEnabled = $derived(gs.page.settings.schedule !== "off");
  const scheduleSummary = $derived(
    gs.page.settings.schedule === "weekly"
      ? "Every " + (weekdays.find((day) => day.value === gs.page.settings.scheduleWeekday)?.label ?? gs.page.settings.scheduleWeekday) + " at " + gs.page.settings.scheduleTime
      : "Every day at " + gs.page.settings.scheduleTime,
  );
</script>
<div class="settings-frame">
  <section id="game-saves-settings" class="settings" aria-label="Backup settings">
    <article class="setting-card surface" aria-labelledby="backup-folder-heading">
      <div class="card-head">
        <span class="card-icon"><Cloud size={19} /></span>
        <div class="card-title">
          <h2 id="backup-folder-heading">Backup folder</h2>
          <p>Choose where your safe copies live.</p>
        </div>
      </div>
      <div class="folder-setting">
        <div class="section-meta">
          <span class="sub-label">Save backups to</span>
          <span class="status"
            class:ready={!!gs.page.settings.backupFolder && !gs.scanResult?.backupUnreachable}
            class:warning={!!gs.scanResult?.backupUnreachable}>
            <span class="status-dot"></span>
            {gs.scanResult?.backupUnreachable ? "Offline" : gs.page.settings.backupFolder ? "Selected" : "Not set"}
          </span>
        </div>
        <div class="path" class:empty={!gs.page.settings.backupFolder}>
          <FolderOpen size={17} />
          <span class="selectable" title={gs.page.settings.backupFolder ?? ""}>
            {gs.page.settings.backupFolder ?? "Choose a folder to start backing up"}
          </span>
        </div>
        {#if gs.scanResult?.backupUnreachable}
          <small class="unreachable"><CloudOff size={13} /> Connect its drive to resume backups.</small>
        {/if}
        <div class="button-row">
          <button class="btn folder-choose" disabled={settingsLocked} onclick={() => gs.chooseBackupFolder()}>
            <FolderSearch size={15} /> {gs.page.settings.backupFolder ? "Change folder" : "Choose folder"}
          </button>
          <button class="btn ghost" disabled={!gs.page.settings.backupFolder} onclick={() => gs.openBackupFolder()}>
            <FolderOpen size={15} /> Open
          </button>
        </div>
      </div>
      <div class="clouds">
        <div class="clouds-head">
          <span class="sub-label">Use a cloud folder</span>
          <button class="icon-btn" title="Look for cloud folders again" aria-label="Look for cloud folders again"
            disabled={settingsLocked} onclick={() => gs.detectCloudFolders()}>
            <RefreshCw size={14} class={gs.settingsBusy === "cloudFolders" ? "spin" : ""} />
          </button>
        </div>
        <div class="cloud-list">
          {#each cloudTiles as tile (tile.key)}
            {@const inUse = !!tile.folder && !!gs.page.settings.backupFolder && samePath(tile.folder.backupPath, gs.page.settings.backupFolder)}
            <button class="cloud-tile" class:found={!!tile.folder} class:in-use={inUse} aria-pressed={inUse}
              title={tile.folder
                ? "Backups go to " + tile.folder.backupPath
                : tile.name + " was not found on this PC. Choose its folder and a backup folder is made inside it."}
              disabled={settingsLocked} onclick={() => gs.useCloudFolder(tile.provider, tile.folder)}>
              <span class="cloud-logo"><CloudLogo provider={tile.provider} size={20} /></span>
              <span class="cloud-text">
                <strong>{tile.name}</strong>
                <small>{inUse ? "Selected" : tile.folder ? "Detected" : "Browse…"}</small>
              </span>
              {#if inUse}<Check size={14} class="cloud-check" />{/if}
            </button>
          {/each}
        </div>
        <p class="helper">Your cloud app syncs these backups across devices.</p>
      </div>
    </article>
    <article class="setting-card surface" aria-labelledby="automatic-backup-heading">
      <div class="card-head">
        <span class="card-icon"><CalendarClock size={19} /></span>
        <div class="card-title">
          <h2 id="automatic-backup-heading">Automatic backup</h2>
          <p>Keep new and changed saves protected.</p>
        </div>
      </div>
      <div class="schedule-setting">
        <div class="section-meta">
          <span class="sub-label">Backup frequency</span>
          <span class="status" class:ready={scheduleEnabled}>
            <span class="status-dot"></span>{scheduleEnabled ? "Enabled" : "Manual"}
          </span>
        </div>
        <div class="frequency" role="group" aria-label="Backup frequency">
          {#each scheduleOptions as option (option.value)}
            <button class:active={gs.page.settings.schedule === option.value}
              aria-pressed={gs.page.settings.schedule === option.value} disabled={settingsLocked}
              onclick={() => void gs.setSchedule(option.value, gs.page.settings.scheduleTime, gs.page.settings.scheduleWeekday)}
            >{option.label}</button>
          {/each}
        </div>
        {#if scheduleEnabled}
          <div class="schedule-fields">
            {#if gs.page.settings.schedule === "weekly"}
              <div class="field">
                <span>Day</span>
                <Select value={gs.page.settings.scheduleWeekday} options={weekdays} ariaLabel="Backup day"
                  fullWidth minWidth="0" disabled={settingsLocked}
                  onchange={(weekday) => void gs.setSchedule(gs.page.settings.schedule, gs.page.settings.scheduleTime, weekday)} />
              </div>
            {/if}
            <div class="field">
              <span>Time</span>
              <Select value={gs.page.settings.scheduleTime} options={timeOptions} ariaLabel="Backup time"
                fullWidth minWidth="0" disabled={settingsLocked}
                onchange={(time) => void gs.setSchedule(gs.page.settings.schedule, time, gs.page.settings.scheduleWeekday)} />
            </div>
          </div>
        {/if}
        <p class="helper">{scheduleEnabled ? scheduleSummary : "Choose Daily or Weekly to back up automatically."}</p>
      </div>
      <div class="last-run" class:failed={lastAttemptFailed}>
        <span class="run-icon"><Clock3 size={17} /></span>
        <div class="run-details">
          <span class="sub-label">{lastAttemptFailed ? "Last attempt failed" : "Last automatic backup"}</span>
          <strong>{formatDate(lastAttemptFailed ? gs.page.settings.lastScheduledAttempt : gs.page.settings.lastScheduledSuccess)}</strong>
          {#if scheduledFailure}<small class="selectable">{scheduledFailure}</small>{/if}
        </div>
      </div>
      <div class="schedule-footer">
        <button class="btn backup-now" disabled={settingsLocked || !gs.page.settings.backupFolder} onclick={() => gs.backupChanged()}>
          <Archive size={16} /> Back up changed saves now
        </button>
        <p class="helper">{gs.page.settings.backupFolder ? "Only new and changed saves are copied." : "Choose a backup folder first."}</p>
      </div>
    </article>
    <article class="setting-card surface folders-card" aria-labelledby="game-folders-heading">
      <div class="card-head">
        <span class="card-icon"><FolderSearch size={19} /></span>
        <div class="card-title">
          <h2 id="game-folders-heading">Game install folders</h2>
          <p>Help find games outside the usual locations.</p>
        </div>
        <button class="icon-btn refresh" title="Detect launcher folders again" aria-label="Detect launcher folders again"
          disabled={settingsLocked} onclick={() => gs.refreshRoots()}>
          <RefreshCw size={15} class={gs.settingsBusy === "roots" ? "spin" : ""} />
        </button>
      </div>
      <div class="section-meta roots-meta">
        <span class="sub-label">Search locations</span>
        <span class="folder-count">{gs.page.settings.roots.length} {gs.page.settings.roots.length === 1 ? "folder" : "folders"}</span>
      </div>
      <ul class="root-list" aria-label="Game search folders">
        {#each gs.page.settings.roots as root (root.id)}
          <li class="root">
            <div class="root-info">
              <div class="root-title">
                <span class="root-store">{storeLabel(root.store)}</span>
                <span class="source" class:manual={root.source === "manual"}>{root.source === "manual" ? "Added by you" : "Detected"}</span>
              </div>
              <span class="root-path selectable" title={root.path}>{root.path}</span>
            </div>
            {#if root.source === "manual"}
              <button class="remove icon-btn" title="Remove folder" aria-label={"Remove " + root.path}
                disabled={settingsLocked} onclick={() => gs.removeRoot(root.id)}><Trash2 size={14} /></button>
            {/if}
          </li>
        {:else}
          <li class="empty">No folders found yet. Refresh to detect launchers, or add a folder below.</li>
        {/each}
      </ul>
      <div class="folders-footer">
        <div class="add-root">
          <Select bind:value={newRootStore} options={storeOptions} ariaLabel="Launcher for new folder"
            minWidth="0" fullWidth disabled={settingsLocked} />
          <button class="btn" disabled={settingsLocked} onclick={() => gs.addRoot(newRootStore)}>
            <Plus size={15} /> Add folder
          </button>
        </div>
        <button class="btn ghost custom-add" disabled={settingsLocked} onclick={() => gs.openCustomDialog()}>
          <Plus size={15} /> Add custom game
        </button>
      </div>
      {#if gs.page.settings.customGames.length}
        <div class="custom-games">
          <span class="sub-label">Custom games</span>
          {#each gs.page.settings.customGames as game (game.id)}
            <div class="custom-game">
              <span>
                <strong>{game.name}</strong>
                <small class="selectable" title={game.paths.join("\n")}>{game.paths.join(" · ")}</small>
              </span>
              <button class="icon-btn" title="Edit custom game" aria-label={"Edit " + game.name}
                disabled={settingsLocked} onclick={() => gs.openCustomDialog(game)}><Pencil size={14} /></button>
              <button class="icon-btn remove" title="Remove custom game" aria-label={"Remove " + game.name}
                disabled={settingsLocked} onclick={() => gs.removeCustomGame(game.id)}><Trash2 size={14} /></button>
            </div>
          {/each}
        </div>
      {/if}
    </article>
  </section>
</div>
<style>
  .settings-frame { container: game-save-settings / inline-size; margin-bottom: 20px; }
  .settings { display: grid; grid-template-columns: minmax(0, 1fr); align-items: stretch; gap: 14px; }
  @container game-save-settings (min-width: 680px) {
    .settings { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .folders-card { grid-column: 1 / -1; }
  }
  @container game-save-settings (min-width: 1040px) {
    .settings { grid-template-columns: repeat(3, minmax(0, 1fr)); }
    .folders-card { grid-column: auto; }
  }
  .setting-card { --settings-muted: rgb(200 210 240 / 0.68); display: flex; flex-direction: column; gap: 20px; min-width: 0; padding: 20px; border-radius: 14px; container-type: inline-size; }
  .card-head { display: flex; align-items: flex-start; gap: 11px; min-width: 0; }
  .card-title { flex: 1; min-width: 0; }
  .card-icon { display: grid; place-items: center; width: 38px; height: 38px; flex: none; border: 1px solid rgb(var(--accent-rgb) / 0.2); border-radius: 11px; background: rgb(var(--accent-rgb) / 0.09); color: rgb(var(--accent-soft-rgb)); }
  h2 { font-size: 14px; line-height: 1.5; }
  .card-title p { margin-top: 3px; color: var(--settings-muted); font-size: 12px; line-height: 1.5; }
  .refresh { margin: 3px -5px 0 0; }
  .folder-setting, .schedule-setting { display: grid; gap: 10px; }
  .section-meta, .clouds-head { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 6px; }
  .sub-label, .field > span { color: var(--settings-muted); font-size: 11.5px; font-weight: 500; }
  .status { display: inline-flex; align-items: center; gap: 5px; padding: 3px 7px; border-radius: 6px; background: rgb(255 255 255 / 0.04); color: var(--settings-muted); font-size: 10.5px; font-weight: 500; white-space: nowrap; }
  .status-dot { width: 5px; height: 5px; flex: none; border-radius: 50%; background: currentColor; }
  .status.ready { color: #98dfbd; background: rgb(62 207 142 / 0.08); }
  .status.warning { color: #efc38a; background: rgb(245 188 95 / 0.08); }
  .path { display: flex; align-items: flex-start; gap: 10px; min-height: 64px; padding: 12px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 10px; background: rgb(0 0 0 / 0.14); color: var(--text-1); }
  .path > :global(svg) { flex: none; margin-top: 2px; color: rgb(var(--accent-soft-rgb) / 0.8); }
  .path > span { min-width: 0; overflow-wrap: anywhere; font-family: var(--font-mono); font-size: 11px; line-height: 1.7; }
  .path.empty > span { font-family: var(--font-sans); font-size: 12px; color: var(--settings-muted); }
  .unreachable { display: flex; align-items: flex-start; gap: 6px; color: #efc38a; font-size: 11.5px; }
  .unreachable > :global(svg) { flex: none; margin-top: 2px; }
  .button-row { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; }
  .setting-card .btn { height: 35px; font-size: 12px; }
  .folder-choose { flex: 1; }
  .clouds { display: grid; gap: 10px; margin-top: auto; padding-top: 15px; border-top: 1px solid rgb(255 255 255 / 0.065); }
  .clouds-head { margin: -5px 0 -3px; }
  .cloud-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .cloud-tile { display: flex; align-items: center; gap: 9px; min-width: 0; min-height: 59px; padding: 10px; border: 1px dashed rgb(255 255 255 / 0.12); border-radius: 9px; background: rgb(0 0 0 / 0.06); text-align: left; transition: background 140ms, border-color 140ms; }
  .cloud-tile.found { border-style: solid; border-color: rgb(255 255 255 / 0.09); background: rgb(255 255 255 / 0.025); }
  .cloud-tile:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.45); background: rgb(var(--accent-rgb) / 0.08); }
  .cloud-tile.in-use { border-style: solid; border-color: rgb(62 207 142 / 0.32); background: rgb(62 207 142 / 0.07); }
  .cloud-logo { display: grid; place-items: center; flex: none; }
  .cloud-tile:not(.found) .cloud-logo { opacity: 0.65; }
  .cloud-text { display: grid; gap: 2px; min-width: 0; }
  .cloud-text strong { overflow: hidden; color: var(--text-1); font-size: 12px; font-weight: 500; white-space: nowrap; text-overflow: ellipsis; }
  .cloud-text small { color: var(--settings-muted); font-size: 10.5px; }
  .cloud-tile.in-use small { color: #98dfbd; }
  .cloud-tile :global(.cloud-check) { flex: none; margin-left: auto; color: #98dfbd; }
  .helper { margin: 0; color: var(--settings-muted); font-size: 11px; line-height: 1.6; }
  .frequency { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; padding: 4px; border: 1px solid rgb(255 255 255 / 0.075); border-radius: 10px; background: rgb(0 0 0 / 0.14); }
  .frequency button { min-height: 33px; border: 1px solid transparent; border-radius: 7px; color: var(--settings-muted); font-size: 12px; font-weight: 500; transition: background 140ms, color 140ms, border-color 140ms; }
  .frequency button:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  .frequency button.active { border-color: rgb(var(--accent-rgb) / 0.26); background: rgb(var(--accent-rgb) / 0.18); color: var(--text-1); box-shadow: 0 1px 3px rgb(0 0 0 / 0.12); }
  .schedule-fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 10px; margin-top: 3px; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .last-run { display: flex; align-items: flex-start; gap: 10px; padding: 13px; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 10px; background: rgb(255 255 255 / 0.02); }
  .run-icon { display: grid; place-items: center; flex: none; margin-top: 2px; color: var(--settings-muted); }
  .run-details { display: grid; gap: 5px; min-width: 0; }
  .last-run strong { color: var(--text-1); font-size: 12px; font-weight: 500; }
  .last-run small { overflow-wrap: anywhere; color: #efc38a; font-size: 11px; line-height: 1.5; }
  .last-run.failed { border-color: rgb(245 188 95 / 0.15); background: rgb(245 188 95 / 0.04); }
  .last-run.failed .run-icon, .last-run.failed strong { color: #efc38a; }
  .schedule-footer { display: grid; gap: 9px; margin-top: auto; padding-top: 15px; border-top: 1px solid rgb(255 255 255 / 0.065); }
  .setting-card .backup-now { width: 100%; height: auto; min-height: 38px; padding: 8px 10px; border-color: rgb(var(--accent-rgb) / 0.32); background: rgb(var(--accent-rgb) / 0.16); color: var(--text-1); white-space: normal; line-height: 1.5; }
  .backup-now > :global(svg) { flex: none; }
  .backup-now:hover:not(:disabled) { background: rgb(var(--accent-rgb) / 0.25); }
  .roots-meta { margin-bottom: -10px; }
  .folder-count { color: var(--settings-muted); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .root-list { display: grid; gap: 7px; max-height: 224px; overflow: auto; margin: 0; padding: 0 3px 0 0; list-style: none; overscroll-behavior: contain; }
  .root { display: flex; align-items: center; gap: 6px; min-width: 0; padding: 9px 10px; border: 1px solid rgb(255 255 255 / 0.045); border-radius: 8px; background: rgb(255 255 255 / 0.02); }
  .root-info { display: grid; gap: 5px; flex: 1; min-width: 0; }
  .root-title { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 4px 8px; }
  .root-store { color: var(--text-1); font-size: 12px; font-weight: 500; }
  .source { padding: 2px 5px; border-radius: 4px; background: rgb(255 255 255 / 0.035); color: var(--settings-muted); font-size: 9.5px; }
  .source.manual { color: rgb(var(--accent-soft-rgb)); background: rgb(var(--accent-rgb) / 0.1); }
  .root-path { overflow-wrap: anywhere; color: var(--settings-muted); font-family: var(--font-mono); font-size: 10.5px; line-height: 1.5; }
  .remove { color: #e6a1a5; }
  .remove:hover:not(:disabled) { background: rgb(229 72 77 / 0.1); color: #ffb4b8; }
  .empty { padding: 16px; border: 1px dashed rgb(255 255 255 / 0.09); border-radius: 9px; color: var(--settings-muted); font-size: 12px; line-height: 1.6; }
  .folders-footer { display: grid; gap: 8px; margin-top: auto; }
  .add-root { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 8px; }
  .custom-add { width: 100%; color: var(--settings-muted); }
  .custom-games { display: grid; gap: 7px; max-height: 170px; overflow: auto; padding-top: 14px; border-top: 1px solid rgb(255 255 255 / 0.065); }
  .custom-game { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 5px; padding: 8px; border-radius: 8px; background: rgb(255 255 255 / 0.02); }
  .custom-game > span { display: grid; gap: 3px; min-width: 0; }
  .custom-game strong { overflow-wrap: anywhere; color: var(--text-1); font-size: 12px; font-weight: 500; }
  .custom-game small { overflow-wrap: anywhere; color: var(--settings-muted); font-family: var(--font-mono); font-size: 10px; }
  button:disabled { opacity: 0.45; pointer-events: none; }
  @container (max-width: 280px) {
    .cloud-list { grid-template-columns: minmax(0, 1fr); }
  }
  @media (prefers-reduced-motion: reduce) {
    .cloud-tile, .frequency button { transition: none; }
  }
</style>
