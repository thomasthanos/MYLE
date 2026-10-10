<script lang="ts">
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Select, { type SelectOption } from "../../../lib/components/Select.svelte";
  import type { GameSaveEntry, GameSaveStatus, GameSavesTab } from "./api";
  import { formatBytes, formatDate, formatRelative, gameSavesState as state } from "./state.svelte";
  let { game, tab }: { game: GameSaveEntry; tab: GameSavesTab } = $props();
  const uid = $props.id();
  const selected = $derived(state.isSelected(game));
  const savingAutoBackup = $derived(state.autoBackupPending.has(game.id));
  const snapshotId = $derived(state.snapshotFor(game));
  const chosenSnapshot = $derived(game.snapshots.find((snapshot) => snapshot.id === snapshotId));
  const snapshotOptions = $derived<SelectOption<string>[]>(
    game.snapshots.map((snapshot) => ({
      value: snapshot.id,
      label: `${formatDate(snapshot.timestamp)}${snapshot.label ? ` · ${snapshot.label}` : ""}${snapshot.isSafety ? " · Safety" : ""}`,
    })),
  );
  /** The backend lists at most this many of a game's save folders. */
  const PATHS_LISTED = 8;
  /** The folder a game's save folders are all in: "…/DeathStrandingDC/1122762396"
   *  for its eight autosave folders, rather than the first one and "+7",
   *  which read as seven changes. Nothing when they share only the drive. */
  function commonFolder(paths: string[]): string | null {
    const split = paths.map((path) => path.split(/[\\/]/));
    const first = split[0] ?? [];
    let shared = 0;
    while (shared < first.length && split.every((parts) => parts[shared]?.toLowerCase() === first[shared].toLowerCase())) {
      shared++;
    }
    return shared > 1 ? first.slice(0, shared).join(paths[0].includes("\\") ? "\\" : "/") : null;
  }
  const shownPath = $derived(
    game.paths.length > 1 ? (commonFolder(game.paths) ?? game.paths[0]) : (game.paths[0] ?? "No local path available"),
  );
  const folderCount = $derived(
    game.paths.length > 1 ? `${game.paths.length >= PATHS_LISTED ? `${PATHS_LISTED}+` : game.paths.length} folders` : null,
  );
  /** Two letters and a colour of its own for each game, so the list is not
   *  one icon repeated: "AC" for Assassin's Creed. */
  const monogram = $derived.by(() => {
    const words = game.title.match(/[\p{L}\p{N}]+/gu) ?? ["?"];
    const letters = (words.length > 1 ? words[0][0] + words[1][0] : words[0].slice(0, 2)).toUpperCase();
    let hash = 0;
    for (const char of game.title) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
    return { letters, hue: hash % 360 };
  });
  const statusInfo: Record<GameSaveStatus, { label: string; tone: string }> = {
    notBackedUp: { label: "First backup needed", tone: "warning" },
    backedUp: { label: "Backed up", tone: "success" },
    changedSinceBackup: { label: "New progress", tone: "changed" },
    backupOnly: { label: "Backup only", tone: "neutral" },
    needsLocation: { label: "Choose restore location", tone: "warning" },
    unknown: { label: "Unknown", tone: "neutral" },
    error: { label: "Needs attention", tone: "danger" },
  };
</script>
<article class="card surface" class:selected class:problem={game.status === "error"}>
  <input
    id="{uid}-check"
    class="check"
    type="checkbox"
    checked={selected}
    disabled={state.selectionLocked || !state.canSelect(game)}
    aria-label={`Select ${game.title}`}
    onchange={() => state.toggleSelected(game)}
  />
  <div class="identity">
    {#if game.steamId && state.covers[game.steamId]}
      <img class="cover" src={state.covers[game.steamId]} alt="" aria-hidden="true" loading="lazy" decoding="async" />
    {:else}
      <span class="monogram" style:--hue={monogram.hue} aria-hidden="true">{monogram.letters}</span>
    {/if}
    <span class="title-wrap">
      <span class="title-line">
        <label class="title" title={game.title} for="{uid}-check">{game.title}</label>
        {#each game.platformBadges as badge (badge)}
          <span class="platform">{badge}</span>
        {/each}
      </span>
      <span class="meta-line">
        <span class="status {statusInfo[game.status].tone}"><i aria-hidden="true"></i>{statusInfo[game.status].label}</span>
        <span class="path selectable" title={game.paths.join("\n")}>{shownPath}</span>
        {#if folderCount}<span class="more-paths" title={game.paths.join("\n")}>{folderCount}</span>{/if}
      </span>
      {#if game.error}
        <span class="error" title={game.error}><TriangleAlert size={13} /> {game.error}</span>
      {/if}
    </span>
  </div>
  <dl class="stats" aria-label={`Save details for ${game.title}`}>
    <div>
      <dt>Size</dt>
      <dd>{formatBytes(tab === "backup" ? (chosenSnapshot?.bytes ?? game.totalBytes) : game.totalBytes)}{#if tab === "pc"}<small> · {game.fileCount.toLocaleString()} {game.fileCount === 1 ? "file" : "files"}</small>{/if}</dd>
    </div>
    <div>
      <dt>Last save</dt>
      <dd title={formatDate(game.lastSaveAt)}>{#if game.hasLocalData}<small aria-hidden="true">Saved</small> {formatRelative(game.lastSaveAt)}{:else}Not on this PC{/if}</dd>
    </div>
    <div>
      <dt>Last backup</dt>
      <dd class:never={!game.lastBackupAt} title={formatDate(game.lastBackupAt)}>{#if game.lastBackupAt}<small aria-hidden="true">Backed up</small> {formatRelative(game.lastBackupAt)}{:else}Never backed up{/if}</dd>
    </div>
  </dl>
  <div class="actions">
    {#if tab === "backup" && game.snapshots.length}
      <div class="snapshot">
        <span>Snapshot</span>
        <Select
          value={snapshotId}
          options={snapshotOptions}
          ariaLabel={`Snapshot for ${game.title}`}
          size="sm"
          minWidth="156px"
          disabled={state.selectionLocked}
          onchange={(id) => state.setSnapshot(game.id, id)}
        />
      </div>
    {/if}
    <label
      class="auto"
      class:on={game.autoBackup}
      class:saving={savingAutoBackup}
      title={game.autoBackup
        ? "Backed up by the schedule. Turn off to back it up only when you choose."
        : "Backed up only when you choose. Turn on to include it in the schedule."}
    >
      <span class="auto-label">{game.autoBackup ? "Scheduled" : "Manual"}</span>
      <input
        class="switch"
        type="checkbox"
        checked={game.autoBackup}
        disabled={state.selectionLocked || !game.hasLocalData}
        aria-busy={savingAutoBackup}
        aria-label={`Automatic backup for ${game.title}`}
        onchange={(event) => state.setAutoBackup(game, event.currentTarget.checked)}
      />
    </label>
    <button
      class="icon-btn"
      disabled={!game.paths.length || !game.hasLocalData}
      title="Open save folder"
      aria-label={`Open save folder for ${game.title}`}
      onclick={() => state.openGameFolder(game)}
    >
      <FolderOpen size={16} />
    </button>
  </div>
</article>
<style>
  /* Identity takes what is left; the stats keep the same widths on every
     row, so the columns line up down the list. */
  .card { display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; grid-template-areas: "check identity stats actions"; align-items: center; gap: 6px 14px; min-width: 0; padding: 7px 12px; transition: border-color var(--dur-fast), background var(--dur-fast); }
  .card:hover { border-color: rgb(255 255 255 / 0.11); }
  .card.selected { border-color: rgb(var(--accent-rgb) / 0.34); background: linear-gradient(180deg, rgb(var(--accent-rgb) / 0.085), rgb(var(--accent-rgb) / 0.025)); }
  .card.problem { border-color: rgb(229 72 77 / 0.2); }
  .check { grid-area: check; }
  .identity { grid-area: identity; display: flex; align-items: center; gap: 10px; min-width: 0; }
  /* Steam's portrait art; the monogram takes the same place without one. */
  .cover { width: 28px; height: 42px; flex: none; border-radius: 7px; object-fit: cover; background: rgb(255 255 255 / 0.04); box-shadow: 0 0 0 1px rgb(255 255 255 / 0.08), 0 6px 14px -8px rgb(0 0 0 / 0.8); }
  .monogram { display: grid; place-items: center; width: 28px; height: 42px; flex: none; border: 1px solid hsl(var(--hue) 70% 70% / 0.18); border-radius: 6px; background: linear-gradient(145deg, hsl(var(--hue) 55% 55% / 0.3), hsl(var(--hue) 55% 35% / 0.12)); color: hsl(var(--hue) 85% 86%); font-family: var(--font-display); font-size: 11.5px; font-weight: 700; letter-spacing: 0.02em; box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.07); }
  .title-wrap { display: grid; gap: 3px; min-width: 0; }
  .title-line,
  .meta-line { display: flex; align-items: center; gap: 7px; min-width: 0; }
  .title { cursor: pointer; min-width: 0; overflow: hidden; font-size: 13.5px; font-weight: 600; white-space: nowrap; text-overflow: ellipsis; }
  .status,
  .platform { display: inline-flex; flex: none; align-items: center; gap: 5px; min-height: 18px; padding: 1px 7px; border: 1px solid rgb(255 255 255 / 0.055); border-radius: 999px; background: rgb(255 255 255 / 0.02); color: var(--text-3); font-size: 10.5px; font-weight: 600; line-height: 1; white-space: nowrap; }
  .platform { font-weight: 500; }
  .status i { width: 6px; height: 6px; border-radius: 50%; background: currentColor; opacity: 0.9; }
  .status.success { border-color: rgb(62 207 142 / 0.14); background: rgb(62 207 142 / 0.05); color: rgb(92 218 166 / 0.85); }
  .status.warning { border-color: rgb(245 176 65 / 0.16); background: rgb(245 176 65 / 0.05); color: rgb(245 188 95 / 0.86); }
  .status.changed { border-color: rgb(77 163 255 / 0.16); background: rgb(77 163 255 / 0.05); color: rgb(112 183 255 / 0.86); }
  .status.danger { border-color: rgb(229 72 77 / 0.2); background: rgb(229 72 77 / 0.05); color: rgb(255 145 145 / 0.86); }
  .path { min-width: 0; overflow: hidden; color: rgb(200 210 240 / 0.4); font-family: var(--font-mono); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
  .more-paths { flex: none; padding: 1px 7px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 999px; color: var(--text-3); font-size: 10px; white-space: nowrap; }
  .error { display: inline-flex; align-items: center; gap: 5px; min-width: 0; overflow: hidden; color: rgb(255 145 145 / 0.8); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; }
  .stats { grid-area: stats; display: grid; grid-template-columns: 112px 128px 150px; gap: 12px; margin: 0; }
  .stats div { min-width: 0; }
  /* For screen readers: each value says what it is on screen. */
  dt { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  dd { margin: 0; overflow: hidden; color: var(--text-2); font-size: 12px; font-weight: 500; font-variant-numeric: tabular-nums; white-space: nowrap; text-overflow: ellipsis; }
  dd small { color: var(--text-3); font-size: 11px; font-weight: 400; }
  dd.never { color: rgb(245 188 95 / 0.75); }
  .actions { grid-area: actions; display: flex; align-items: center; justify-content: flex-end; gap: 10px; }
  .auto { display: flex; align-items: center; gap: 7px; padding: 2px 3px 2px 9px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 999px; background: rgb(255 255 255 / 0.02); color: var(--text-3); font-size: 11px; font-weight: 600; cursor: pointer; transition: border-color var(--dur-fast), background var(--dur-fast), color var(--dur-fast); }
  .auto.on { border-color: rgb(var(--accent-rgb) / 0.2); background: rgb(var(--accent-rgb) / 0.06); color: rgb(var(--accent-soft-rgb) / 0.95); }
  .auto.saving { border-color: rgb(var(--accent-rgb) / .4); }
  /* Both words take the same room, so the switch does not move. */
  .auto-label { min-width: 58px; text-align: right; }
  .snapshot { display: grid; gap: 3px; color: var(--text-3); font-size: 10.5px; }
  button:disabled { opacity: 0.35; pointer-events: none; }
  /* Narrower: the values go under the name, on one short line. */
  @container (max-width: 860px) {
    .card {
      grid-template-columns: auto minmax(0, 1fr) auto;
      grid-template-areas:
        "check identity actions"
        ". stats stats";
    }
    .stats {
      display: flex;
      flex-wrap: wrap;
      gap: 4px 16px;
      padding-left: 38px;
    }
  }
  /* Very narrow: the switch and the folder go under the values. */
  @container (max-width: 480px) {
    .card {
      grid-template-columns: auto minmax(0, 1fr);
      grid-template-areas:
        "check identity"
        ". stats"
        ". actions";
    }
    .stats {
      padding-left: 0;
    }
    .actions {
      justify-content: flex-start;
      flex-wrap: wrap;
    }
  }
</style>
