// Page-level singleton: ContentArea destroys page components when navigating,
// so scan results and running work live here rather than in GameSaves.svelte.
import { isTauri } from "@tauri-apps/api/core";
import { SvelteSet } from "svelte/reactivity";
import { badges } from "../../../lib/badges.svelte";
import { confirm } from "../../../lib/confirm.svelte";
import { nav } from "../../../lib/nav.svelte";
import { notifyChange, readFlag, readJson, writeFlag, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import {
  CLOUD_PROVIDERS,
  gameSavesApi,
  type BackupSchedule,
  type CloudProvider,
  type CustomGame,
  type DetectedFolder,
  type GameSaveEntry,
  type GameSaveStatus,
  type GameSavesEvent,
  type GameFailure,
  type GameSavesOperationResult,
  type GameSavesPageState,
  type GameSavesScan,
  type GameSavesSettings,
  type GameSavesTab,
  type OperationKind,
  type OperationStage,
  type PathMapping,
  type RestoreSelection,
  type RootStore,
  type ScheduleWeekday,
} from "./api";
/** Windows paths, compared as Windows does: case and a trailing slash aside. */
export function samePath(a: string, b: string) {
  const normal = (path: string) => path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  return normal(a) === normal(b);
}
export type GameSavesFilter = "all" | "changed" | "notBackedUp" | "backedUp" | "problems";
interface OperationView {
  startedAt: number;
  kind: OperationKind;
  stage: OperationStage;
  done: number;
  total: number;
  current: string | null;
  note: string | null;
  /** A check the app started by itself (every ten minutes, on coming back to
   *  the window): a line in the toolbar, not the progress bar. */
  background?: boolean;
}
const KEY = {
  tab: "myle.gameSaves.tab",
  filter: "myle.gameSaves.filter",
  settingsOpen: "myle.gameSaves.settingsOpen",
  steamCovers: "myle.gameSaves.steamCovers",
};
const oneOf = <T extends string>(...values: T[]) => (value: unknown) => values.includes(value as T);
/** Settings that follow the account to other PCs (the rest name local folders). */
const SYNCED_SETTINGS = new Set(["schedule", "customGame"]);
/** Background check for saves changed by playing, while the app is open. */
const WATCH_EVERY_MS = 10 * 60 * 1000;
const WATCH_FIRST_MS = 20 * 1000;
/** Coming back to the window checks again, but not more often than this:
 *  every Alt+Tab back was one more scan. */
const FOCUS_CHECK_MS = 10 * 60 * 1000;
const emptySettings = (): GameSavesSettings => ({
  backupFolder: null,
  schedule: "off",
  scheduleTime: "03:00",
  scheduleWeekday: "sunday",
  lastScheduledAttempt: null,
  lastScheduledSuccess: null,
  lastScheduledResult: null,
  roots: [],
  autoBackupExcludedGameIds: [],
  customGames: [],
  pathMappings: [],
});
const emptyPage = (): GameSavesPageState => ({
  settings: emptySettings(),
  cloudFolders: [],
  engineAvailable: true,
  engineVersion: null,
  activeOperation: null,
  databaseGames: 0,
  databaseUpdatedAt: null,
  undoRestore: null,
  cachedScan: null,
});
function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
function matchesStatus(status: GameSaveStatus, filter: GameSavesFilter): boolean {
  switch (filter) {
    case "changed":
      return status === "changedSinceBackup";
    case "notBackedUp":
      return status === "notBackedUp";
    case "backedUp":
      return status === "backedUp" || status === "backupOnly";
    case "problems":
      return status === "error" || status === "unknown" || status === "needsLocation";
    default:
      return true;
  }
}
class GameSavesState {
  page = $state<GameSavesPageState>(emptyPage());
  scanResult = $state<GameSavesScan | null>(null);
  loading = $state(true);
  error = $state<string | null>(null);
  /** The games the last backup or restore could not handle, and why. Stays
   *  on the page until dismissed or the next operation. */
  failures = $state<{ kind: "backup" | "restore"; items: GameFailure[] } | null>(null);
  settingsOpen = $state(readFlag(KEY.settingsOpen, false));
  /** Covers by Steam app id, as `data:` URLs. */
  covers = $state<Record<number, string>>({});
  /** Covers missing from this PC may be fetched from Steam (it then learns which games). */
  steamCovers = $state(readFlag(KEY.steamCovers, true));
  /** App ids already asked for, with whether Steam was asked too. */
  #coversAsked = new Map<number, boolean>();
  tab = $state<GameSavesTab>(readJson(KEY.tab, "pc", oneOf("pc", "backup")));
  filter = $state<GameSavesFilter>(
    readJson(KEY.filter, "all", oneOf("all", "changed", "notBackedUp", "backedUp", "problems")),
  );
  query = $state("");
  readonly selected = new SvelteSet<string>();
  operation = $state<OperationView | null>(null);
  cancelling = $state(false);
  settingsBusy = $state<string | null>(null);
  readonly autoBackupPending = new SvelteSet<string>();
  #autoBackupDesired = new Map<string, boolean>();
  #autoBackupConfirmed = new Map<string, boolean>();
  #autoBackupSave: Promise<void> | null = null;
  customDialogOpen = $state(false);
  editingCustomGame = $state<CustomGame | null>(null);
  restoreDialogOpen = $state(false);
  snapshotChoices = $state<Record<string, string>>({});
  /** A full scan looking for newly installed games, in the background. */
  discovering = $state(false);
  #discovery: Promise<void> | null = null;
  #initialized = false;
  #watching = false;
  #lastRefresh = 0;
  /** Changed saves already announced; null until the first result. */
  #announced: number | null = null;
  #settingsRevision = 0;
  readonly games = $derived(this.tab === "pc" ? (this.scanResult?.onThisPc ?? []) : (this.scanResult?.inBackup ?? []));
  readonly visibleGames = $derived.by(() => {
    const q = this.query.trim().toLocaleLowerCase();
    return this.games.filter((game) => {
      if (!matchesStatus(game.status, this.filter)) return false;
      return !q || game.title.toLocaleLowerCase().includes(q) || game.platformBadges.some((badge) => badge.toLocaleLowerCase().includes(q));
    });
  });
  canSelect(game: GameSaveEntry): boolean {
    return this.tab === "pc" ? game.hasLocalData : game.hasBackup && game.snapshots.length > 0;
  }
  readonly selectableGames = $derived(this.visibleGames.filter((game) => this.canSelect(game)));
  readonly selectedGames = $derived(this.games.filter((game) => this.selected.has(game.id) && this.canSelect(game)));
  readonly allVisibleSelected = $derived(
    this.selectableGames.length > 0 && this.selectableGames.every((game) => this.selected.has(game.id)),
  );
  /** Some of the listed games are ticked, not all: the box in between. */
  readonly someVisibleSelected = $derived(
    !this.allVisibleSelected && this.selectableGames.some((game) => this.selected.has(game.id)),
  );
  /** How many games of this tab each status filter shows (the search aside). */
  readonly statusCounts = $derived.by(() => {
    const counts: Record<GameSavesFilter, number> = { all: 0, changed: 0, notBackedUp: 0, backedUp: 0, problems: 0 };
    for (const game of this.games) {
      for (const filter of Object.keys(counts) as GameSavesFilter[]) {
        if (matchesStatus(game.status, filter)) counts[filter]++;
      }
    }
    return counts;
  });
  readonly selectedBytes = $derived(this.selectedGames.reduce((sum, game) => sum + (this.tab === "backup" ? (game.snapshots.find((snapshot) => snapshot.id === this.snapshotFor(game))?.bytes ?? 0) : game.totalBytes), 0));
  readonly busy = $derived(this.operation !== null);
  readonly locked = $derived(this.busy || !!this.settingsBusy || this.loading);
  /** A row's automatic-backup preference does not change list contents. */
  readonly selectionLocked = $derived(this.busy || this.loading || (!!this.settingsBusy && this.settingsBusy !== "autoBackup"));
  readonly listLocked = $derived(this.busy);
  readonly pendingGames = $derived((this.scanResult?.onThisPc ?? []).filter((game) => game.hasLocalData && (game.status === "notBackedUp" || game.status === "changedSinceBackup")));
  readonly protectedGames = $derived((this.scanResult?.onThisPc ?? []).filter((game) => game.status === "backedUp").length);
  /** Saves never backed up, and saves changed since their last backup. */
  readonly newGames = $derived(this.pendingGames.filter((game) => game.status === "notBackedUp"));
  readonly changedGames = $derived(this.pendingGames.filter((game) => game.status === "changedSinceBackup"));
  readonly localGames = $derived(this.scanResult?.stats.localGames ?? 0);
  readonly backupGames = $derived(this.scanResult?.stats.backupGames ?? 0);
  readonly totalBytes = $derived(this.scanResult?.stats.totalBytes ?? 0);
  readonly databaseGames = $derived(this.scanResult?.stats.databaseGames ?? this.page.databaseGames);
  async init() {
    if (!isTauri()) {
      this.loading = false;
      this.page.engineAvailable = false;
      return;
    }
    if (this.#initialized) return;
    this.#initialized = true;
    this.loading = !this.scanResult;
    try {
      this.page = await gameSavesApi.getState();
      if (this.page.activeOperation) {
        this.operation = {
          startedAt: Date.now(),
          kind: this.page.activeOperation,
          stage: "preparing",
          done: 0,
          total: 0,
          current: null,
          note: "An existing Game Saves operation is still running.",
        };
        void this.#waitForAdopted();
      } else if (this.page.engineAvailable) {
        // The saved list shows at once; only the games on it are re-checked
        // (seconds). Without one, the full scan is the only way in.
        if (!this.scanResult && this.page.cachedScan) this.#applyScan(this.page.cachedScan);
        this.loading = false;
        if (this.scanResult) await this.refresh();
        else await this.scan(false);
      }
    } catch (error) {
      this.error = message(error);
      // Let the next visit try again instead of showing an empty page for good.
      this.#initialized = false;
    } finally {
      this.loading = false;
    }
  }
  /** The page was reloaded while an operation ran: its result cannot reach
   *  this page any more, so wait until the backend is idle and rescan. */
  async #waitForAdopted() {
    const adopted = this.operation;
    while (this.operation === adopted) {
      await new Promise((resolve) => setTimeout(resolve, 2000));
      try {
        const page = await gameSavesApi.getState();
        if (page.activeOperation) continue;
        this.page = page;
        if (this.operation !== adopted) return;
        this.operation = null;
        this.cancelling = false;
        if (!page.engineAvailable) return;
        if (!this.scanResult && page.cachedScan) this.#applyScan(page.cachedScan);
        if (this.scanResult) await this.refresh();
        else await this.scan(false);
        return;
      } catch {
        // Keep waiting; the next poll retries.
      }
    }
  }
  /** Re-reads the saved tab and filter (after account sync replaced them). */
  reloadChoices() {
    this.tab = readJson(KEY.tab, this.tab, oneOf("pc", "backup"));
    this.filter = readJson(KEY.filter, this.filter, oneOf("all", "changed", "notBackedUp", "backedUp", "problems"));
  }
  /** For the app's preload: the last saved list and its covers, without
   *  running the engine, so the page opens on them (and the sidebar badge
   *  counts changed saves from the start). Opening the page still checks. */
  async preload() {
    if (this.scanResult || this.#initialized) return;
    try {
      const page = await gameSavesApi.getState();
      // The page itself got there first: leave it to its own load.
      if (this.scanResult || this.#initialized) return;
      this.page = page;
      if (page.cachedScan && !page.activeOperation) this.#applyScan(page.cachedScan);
    } catch {
      return;
    }
    await this.loadCovers();
  }
  /** Asks for the covers of the listed games not asked for yet. Quietly:
   *  a game without one keeps its monogram. */
  async loadCovers() {
    const online = this.steamCovers;
    const ids = [...(this.scanResult?.onThisPc ?? []), ...(this.scanResult?.inBackup ?? [])]
      .map((game) => game.steamId)
      .filter((id): id is number => typeof id === "number" && id > 0 && !this.covers[id])
      .filter((id) => !this.#coversAsked.has(id) || (online && !this.#coversAsked.get(id)));
    const unique = [...new Set(ids)];
    if (!unique.length) return;
    for (const id of unique) this.#coversAsked.set(id, online);
    try {
      const found = await gameSavesApi.covers(unique, online);
      const next = { ...this.covers };
      for (const [id, url] of Object.entries(found)) next[Number(id)] = url;
      this.covers = next;
    } catch {
      // Covers only decorate; the list works without them.
      for (const id of unique) this.#coversAsked.delete(id);
    }
  }
  setSteamCovers(on: boolean) {
    this.steamCovers = on;
    writeFlag(KEY.steamCovers, on);
    if (on) void this.loadCovers();
  }
  toggleSettings() {
    this.settingsOpen = !this.settingsOpen;
    writeFlag(KEY.settingsOpen, this.settingsOpen);
  }
  setTab(tab: GameSavesTab) {
    if (this.selectionLocked) return;
    if (this.tab === tab) return;
    this.tab = tab;
    this.selected.clear();
    this.restoreDialogOpen = false;
    writeJson(KEY.tab, tab);
  }
  setFilter(filter: GameSavesFilter) {
    this.filter = filter;
    writeJson(KEY.filter, filter);
  }
  setQuery(value: string) {
    this.query = value;
  }
  isSelected(game: GameSaveEntry): boolean {
    return this.selected.has(game.id);
  }
  toggleSelected(game: GameSaveEntry) {
    if (this.selectionLocked || !this.canSelect(game)) return;
    if (this.selected.has(game.id)) this.selected.delete(game.id);
    else this.selected.add(game.id);
  }
  toggleAllVisible() {
    if (this.selectionLocked) return;
    if (this.allVisibleSelected) {
      for (const game of this.selectableGames) this.selected.delete(game.id);
    } else {
      for (const game of this.selectableGames) this.selected.add(game.id);
    }
  }
  clearSelection() {
    if (this.selectionLocked) return;
    this.selected.clear();
  }
  selectPending() {
    if (this.selectionLocked) return;
    this.setTab("pc");
    for (const game of this.pendingGames) this.selected.add(game.id);
    this.setFilter("all");
    this.setQuery("");
  }
  async openRestorePicker() {
    if (!this.selectedGames.length || this.locked) return;
    await this.#stopDiscovery();
    if (!this.selectedGames.length || this.locked) return;
    for (const game of this.selectedGames) {
      if (!this.snapshotChoices[game.id] && game.snapshots[0]) this.snapshotChoices[game.id] = game.snapshots[0].id;
    }
    this.restoreDialogOpen = true;
  }
  setSnapshot(gameId: string, snapshotId: string) {
    if (this.selectionLocked) return;
    this.snapshotChoices[gameId] = snapshotId;
  }
  snapshotFor(game: GameSaveEntry): string {
    const selected = this.snapshotChoices[game.id];
    return game.snapshots.some((snapshot) => snapshot.id === selected) ? selected : (game.snapshots[0]?.id ?? "");
  }
  mappingFor(game: GameSaveEntry): PathMapping | undefined {
    return this.page.settings.pathMappings.find(
      (mapping) => mapping.gameId === game.id && (!game.paths[0] || samePath(mapping.source, game.paths[0])),
    );
  }
  openCustomDialog(game: CustomGame | null = null) {
    if (this.locked) return;
    this.editingCustomGame = game;
    this.customDialogOpen = true;
  }
  closeCustomDialog() {
    if (!this.locked) {
      this.customDialogOpen = false;
      this.editingCustomGame = null;
    }
  }
  /** Full scan of every game in the database ("Scan again"). */
  async scan(announce = true) {
    if (this.locked || !this.page.engineAvailable) return;
    await this.#stopDiscovery();
    if (this.locked) return;
    this.beginOperation("scan", "scanning");
    this.error = null;
    try {
      const result = await gameSavesApi.scan("full", this.onEvent);
      this.#applyScan(result);
      this.#reloadPage();
      if (announce) toast.success(`Found ${result.stats.localGames} games on this PC.`);
    } catch (error) {
      this.fail("Scan failed", error);
    } finally {
      this.operation = null;
    }
  }
  /** Re-checks only the games already found: a couple of seconds. */
  async refresh(quiet = false) {
    if (this.locked || this.discovering || !this.page.engineAvailable || !this.scanResult) return;
    this.#lastRefresh = Date.now();
    this.beginOperation("scan", "scanning");
    if (quiet && this.operation) this.operation.background = true;
    let discoveryDue = false;
    try {
      const result = await gameSavesApi.scan("quick", this.onEvent);
      this.#applyScan(result);
      if (!quiet) this.#reloadPage();
      discoveryDue = result.discoveryDue;
    } catch (error) {
      if (!quiet) this.fail("Refresh failed", error);
    } finally {
      this.operation = null;
    }
    if (discoveryDue) void this.discover();
  }
  /**
   * Full scan in the background, for games installed since the last one.
   * The list stays usable; any action cancels it first.
   */
  discover(): Promise<void> {
    if (this.#discovery) return this.#discovery;
    if (this.locked || this.customDialogOpen || this.restoreDialogOpen || !this.page.engineAvailable) return Promise.resolve();
    this.discovering = true;
    this.#discovery = (async () => {
      try {
        const before = new Set((this.scanResult?.onThisPc ?? []).map((game) => game.id));
        const result = await gameSavesApi.scan("full", () => {});
        this.#applyScan(result);
        this.#reloadPage();
        const found = result.onThisPc.filter((game) => !before.has(game.id)).length;
        if (found) toast.info(`Found ${found} new ${found === 1 ? "game" : "games"} with saves.`);
      } catch {
        // Cancelled for an action, or failed: the next refresh asks again.
      } finally {
        this.discovering = false;
        this.#discovery = null;
      }
    })();
    return this.#discovery;
  }
  /** "Stop" on the background search for new games. */
  async stopDiscovery() {
    await this.#stopDiscovery();
  }
  async #stopDiscovery() {
    const running = this.#discovery;
    if (!running) return;
    await gameSavesApi.cancel().catch(() => {});
    await running;
  }
  /**
   * Starts the background check that notices saves changed by playing:
   * shortly after launch, every ten minutes, and on coming back to the window.
   */
  startWatcher() {
    if (this.#watching || !isTauri()) return;
    this.#watching = true;
    const check = () => void this.#backgroundCheck();
    const onFocus = () => {
      if (Date.now() - this.#lastRefresh > FOCUS_CHECK_MS) check();
    };
    const first = setTimeout(check, WATCH_FIRST_MS);
    const every = setInterval(check, WATCH_EVERY_MS);
    window.addEventListener("focus", onFocus);
    this.#stopWatching = () => {
      clearTimeout(first);
      clearInterval(every);
      window.removeEventListener("focus", onFocus);
      this.#watching = false;
    };
  }
  /** Ends the watcher: on a hot reload in development, the old copy of this
   *  module would otherwise go on scanning next to the new one, one more
   *  after every reload. */
  stopWatcher() {
    this.#stopWatching?.();
    this.#stopWatching = null;
  }
  #stopWatching: (() => void) | null = null;
  async #backgroundCheck() {
    if (this.busy || this.discovering || this.settingsBusy) return;
    try {
      if (!this.scanResult) {
        // The page has not been opened yet: start from the saved list. With
        // none, the user has not set Game Saves up, and nothing is scanned.
        const page = await gameSavesApi.getState();
        if (!page.cachedScan || page.activeOperation || !page.engineAvailable) return;
        if (!this.scanResult) {
          this.page = page;
          this.#applyScan(page.cachedScan);
          this.loading = false;
        }
      }
      await this.refresh(true);
    } catch {
      // A background check never interrupts; the next one retries.
    }
  }
  #applyScan(result: GameSavesScan) {
    this.scanResult = result;
    this.pruneSelection();
    this.#updateBadge(result);
  }
  /** Sidebar count of changed saves, and a toast when it goes up. */
  #updateBadge(result: GameSavesScan) {
    const changed = result.onThisPc.filter((game) => game.autoBackup && game.status === "changedSinceBackup").length;
    badges.set("game-saves", changed);
    if (this.#announced !== null && changed > this.#announced && nav.current !== "game-saves") {
      toast.info(`${changed} game ${changed === 1 ? "save has" : "saves have"} changed since the last backup.`, {
        label: "Review",
        run: () => {
          this.setTab("pc");
          this.setFilter("changed");
          nav.go("game-saves");
        },
      });
    }
    this.#announced = changed;
  }
  /** A scan can re-detect launcher folders; show the settings it used. */
  #reloadPage() {
    const revision = this.#settingsRevision;
    void gameSavesApi
      .getState()
      .then((page) => {
        if (revision === this.#settingsRevision && !this.settingsBusy) this.page = page;
      })
      .catch(() => {});
  }
  async updateDatabase() {
    if (this.locked) return;
    await this.#stopDiscovery();
    if (this.locked) return;
    this.beginOperation("updateDatabase", "updatingDatabase");
    this.error = null;
    try {
      const result = await gameSavesApi.updateDatabase(this.onEvent);
      this.page.databaseGames = result.games;
      this.page.databaseUpdatedAt = result.updatedAt;
      toast.success(`Game database updated · ${result.games.toLocaleString()} games in database.`);
    } catch (error) {
      this.fail("Database update failed", error);
      return;
    } finally {
      this.operation = null;
    }
    await this.scan(false);
  }
  async backupSelected() {
    await this.backup(this.selectedGames.map((game) => game.id));
  }
  /** Backs up, in one go, the games whose saves were never backed up
   *  ("new"), changed since their backup ("changed"), or both. */
  async backupPending(which: "both" | "new" | "changed") {
    const games = which === "new" ? this.newGames : which === "changed" ? this.changedGames : this.pendingGames;
    if (!games.length) return;
    const count = `${games.length} ${games.length === 1 ? "game" : "games"}`;
    const title = {
      both: `Back up ${count} with new saves or new progress?`,
      new: `Make the first backup of ${count}?`,
      changed: `Back up the new progress of ${count}?`,
    }[which];
    await this.backup(games.map((game) => game.id), title);
  }
  async backupChanged() {
    const ids = (this.scanResult?.onThisPc ?? [])
      .filter(
        (game) => game.autoBackup && (game.status === "changedSinceBackup" || game.status === "notBackedUp"),
      )
      .map((game) => game.id);
    if (!ids.length) {
      toast.info("No new or changed saves are enabled for automatic backup.");
      return;
    }
    await this.backup(ids);
  }
  private async backup(gameIds: string[], title?: string) {
    if (this.locked || !this.page.engineAvailable || !gameIds.length) return;
    if (!this.page.settings.backupFolder) {
      toast.info("Choose a backup folder first.");
      this.settingsOpen = true;
      return;
    }
    const ok = await confirm({
      title: title ?? `Back up ${gameIds.length} ${gameIds.length === 1 ? "game" : "games"}?`,
      message: "New and changed save files will be copied to your backup folder. Existing snapshots are kept according to the retention policy.",
      confirmLabel: "Back up now",
    });
    if (!ok || this.locked) return;
    await this.#stopDiscovery();
    if (this.locked) return;
    this.beginOperation("backup", "preparing");
    try {
      const result = await gameSavesApi.backup(gameIds, this.onEvent);
      this.reportResult(result);
      this.keepFailedSelection(gameIds, result);
    } catch (error) {
      this.fail("Backup failed", error);
    } finally {
      this.operation = null;
    }
    await this.refreshAfterOperation();
  }
  async restore(selections: RestoreSelection[]) {
    if (this.locked || !this.page.engineAvailable || !selections.length) return;
    this.restoreDialogOpen = false;
    const chosen = selections.flatMap((selection) => {
      const game = this.selectedGames.find((item) => item.id === selection.gameId);
      const snapshot = game?.snapshots.find((item) => item.id === selection.snapshotId);
      return game && snapshot ? [{ game, snapshot }] : [];
    });
    if (chosen.length !== selections.length) {
      toast.error("A selected snapshot is no longer available. Refresh the list and try again.");
      return;
    }
    const bytes = chosen.reduce((sum, item) => sum + item.snapshot.bytes, 0);
    const ok = await confirm({
      title: `Restore ${selections.length} ${selections.length === 1 ? "game" : "games"}?`,
      message:
        `${chosen.length} ${chosen.length === 1 ? "saved copy" : "saved copies"} · ${formatBytes(bytes)}\n\n` +
        "Close the selected games before continuing. Their current local saves will be replaced by the chosen snapshots. A safety backup is created first and can be undone for 7 days.",
      confirmLabel: "Restore saves",
      danger: true,
    });
    if (!ok || this.locked) return;
    await this.#stopDiscovery();
    if (this.locked) return;
    this.beginOperation("restore", "creatingSafetyBackup");
    try {
      const result = await gameSavesApi.restore(selections, this.onEvent);
      this.reportResult(result);
      this.keepFailedSelection(selections.map((selection) => selection.gameId), result);
    } catch (error) {
      this.fail("Restore failed", error);
    } finally {
      this.operation = null;
    }
    await this.refreshAfterOperation();
  }
  async undoLastRestore() {
    const undo = this.page.undoRestore;
    if (!undo || this.locked) return;
    const ok = await confirm({
      title: "Undo the last restore?",
      message:
        `${undo.games.length} ${undo.games.length === 1 ? "game" : "games"} will be returned to their state before the last restore.\n\n` +
        "Close those games before continuing. This safety copy is consumed only after a successful undo.",
      confirmLabel: "Undo restore",
      danger: true,
    });
    if (!ok || this.locked) return;
    await this.#stopDiscovery();
    if (this.locked) return;
    this.beginOperation("restore", "preparing");
    try {
      const result = await gameSavesApi.undoLastRestore(this.onEvent);
      if (result.failedGames.length) this.reportResult(result);
      else toast.success("The last restore was undone.");
    } catch (error) {
      this.fail("Could not undo the last restore", error);
    } finally {
      this.operation = null;
    }
    await this.refreshAfterOperation();
  }
  async cancel() {
    if (!this.busy || this.cancelling) return;
    this.cancelling = true;
    try {
      await gameSavesApi.cancel();
    } catch (error) {
      toast.error(`Could not cancel: ${message(error)}`);
    } finally {
      this.cancelling = false;
    }
  }
  async chooseBackupFolder() {
    let changed = false;
    await this.setting("backupFolder", async () => {
      const settings = await gameSavesApi.pickBackupFolder();
      if (settings) changed = this.applyBackupFolder(settings);
    });
    if (changed) await this.refresh();
  }
  private applyBackupFolder(settings: GameSavesSettings): boolean {
    const changed = !samePath(this.page.settings.backupFolder ?? "", settings.backupFolder ?? "");
    this.page.settings = settings;
    if (!changed || !this.scanResult) return false;
    // Old snapshots belong to the old folder. Never offer them for restore
    // while the new folder is being checked, including when that check fails.
    this.scanResult.inBackup = [];
    this.scanResult.stats.backupGames = 0;
    this.scanResult.backupUnreachable = null;
    for (const game of this.scanResult.onThisPc) {
      game.hasBackup = false; game.lastBackupAt = null; game.snapshots = [];
      if (game.status === "backedUp" || game.status === "changedSinceBackup") game.status = "notBackedUp";
    }
    this.snapshotChoices = {};
    if (this.tab === "backup") this.selected.clear();
    return true;
  }
  /** Backs up into a cloud folder: a detected one, or one the user points to. */
  async useCloudFolder(provider: CloudProvider, folder: DetectedFolder | null) {
    const before = this.page.settings.backupFolder;
    let changed = false;
    await this.setting("backupFolder", async () => {
      const settings = await gameSavesApi.useCloudFolder(provider, folder?.path ?? null);
      if (!settings) return;
      changed = this.applyBackupFolder(settings);
      const name = folder?.label ?? CLOUD_PROVIDERS.find((item) => item.id === provider)?.name ?? provider;
      const moved = !!before && !samePath(before, settings.backupFolder ?? "");
      toast.success(
        `Backups now go to ${name}: ${settings.backupFolder}` +
          (moved ? ". Earlier backups stay in the previous folder." : ""),
      );
    });
    if (changed) await this.refresh();
  }
  async openBackupFolder() {
    try {
      await gameSavesApi.openBackupFolder();
    } catch (error) {
      toast.error(`Could not open the backup folder: ${message(error)}`);
    }
  }
  async detectCloudFolders() {
    await this.setting("cloudFolders", async () => {
      this.page.cloudFolders = await gameSavesApi.detectCloudFolders();
      const count = this.page.cloudFolders.length;
      toast.info(count ? `Found ${count} cloud folder${count === 1 ? "" : "s"}.` : "No cloud folders were found on this PC.");
    });
  }
  async setSchedule(schedule: BackupSchedule, time: string, weekday: ScheduleWeekday) {
    await this.setting("schedule", async () => {
      this.page.settings = await gameSavesApi.setSchedule(schedule, time, weekday);
      toast.success(schedule === "off" ? "Automatic backups turned off." : `Automatic backups set to ${schedule}.`);
    });
  }
  async refreshRoots() {
    await this.setting("roots", async () => {
      this.page.settings = await gameSavesApi.refreshRoots();
      toast.success("Game install folders refreshed.");
    });
  }
  async addRoot(store: RootStore) {
    await this.setting("roots", async () => {
      const settings = await gameSavesApi.addRoot(store);
      if (settings) this.page.settings = settings;
    });
  }
  async removeRoot(rootId: string) {
    const root = this.page.settings.roots.find((item) => item.id === rootId);
    if (!root) return;
    const ok = await confirm({
      title: "Remove game folder?",
      message: `${root.path}\n\nGames in this folder will no longer be discovered automatically. Existing backups are not deleted.`,
      confirmLabel: "Remove folder",
      danger: true,
    });
    if (!ok) return;
    await this.setting("roots", async () => {
      this.page.settings = await gameSavesApi.removeRoot(rootId);
    });
  }
  async saveCustomGame(game: CustomGame) {
    await this.setting("customGame", async () => {
      this.page.settings = await gameSavesApi.upsertCustomGame(game);
      this.customDialogOpen = false;
      this.editingCustomGame = null;
      toast.success(`${game.name.trim()} saved.`);
    });
  }
  async pickFolder(title: string): Promise<string | null> {
    let path: string | null = null;
    await this.setting("folderPicker", async () => { path = await gameSavesApi.pickFolder(title); });
    return path;
  }
  async chooseRestoreLocation(game: GameSaveEntry) {
    await this.setting("pathMapping", async () => {
      const target = await gameSavesApi.pickFolder(`Choose where to restore ${game.title}`);
      if (!target) return;
      this.page.settings = await gameSavesApi.setPathMapping({
        gameId: game.id,
        source: game.paths[0] ?? game.id,
        target,
      });
    });
  }
  async removeRestoreLocation(game: GameSaveEntry) {
    const mapping = this.mappingFor(game);
    if (!mapping) return;
    await this.setting("pathMapping", async () => {
      this.page.settings = await gameSavesApi.removePathMapping(mapping.gameId, mapping.source);
    });
  }
  async removeCustomGame(gameId: string) {
    const game = this.page.settings.customGames.find((item) => item.id === gameId);
    if (!game) return;
    const ok = await confirm({
      title: `Remove ${game.name}?`,
      message: "This removes the custom definition only. Save files and backups are not deleted.",
      confirmLabel: "Remove game",
      danger: true,
    });
    if (!ok) return;
    await this.setting("customGame", async () => {
      this.page.settings = await gameSavesApi.removeCustomGame(gameId);
      toast.success(`${game.name} removed.`);
    });
  }
  async setAutoBackup(game: GameSaveEntry, enabled: boolean) {
    if (this.selectionLocked || !game.hasLocalData) return;
    if (!this.#autoBackupConfirmed.has(game.id)) this.#autoBackupConfirmed.set(game.id, game.autoBackup);
    this.#autoBackupDesired.set(game.id, enabled);
    this.autoBackupPending.add(game.id);
    this.#mirrorAutoBackup(game.id, enabled);
    if (!this.#autoBackupSave) {
      this.settingsBusy = "autoBackup";
      this.#settingsRevision++;
      this.#autoBackupSave = this.#saveAutoBackup().finally(() => {
        this.#autoBackupSave = null;
        this.settingsBusy = null;
      });
    }
    await this.#autoBackupSave;
  }
  #mirrorAutoBackup(id: string, enabled: boolean) {
    for (const game of [...(this.scanResult?.onThisPc ?? []), ...(this.scanResult?.inBackup ?? [])]) {
      if (game.id === id) game.autoBackup = enabled;
    }
  }
  async #saveAutoBackup() {
    let saved = false;
    try {
      await this.#stopDiscovery();
      // One writer for every row. Repeated clicks replace the queued value;
      // an in-flight write finishes before the most recent choice is saved.
      while (this.#autoBackupDesired.size) {
        const [id, enabled] = this.#autoBackupDesired.entries().next().value!;
        this.#autoBackupDesired.delete(id);
        this.#mirrorAutoBackup(id, enabled);
        try {
          const settings = await gameSavesApi.setGameAutoBackup(id, enabled);
          // Patch only this preference. Replacing the entire settings/scan
          // snapshot on each click invalidates unrelated UI and cached rows.
          this.page.settings.autoBackupExcludedGameIds = settings.autoBackupExcludedGameIds;
          for (const game of this.page.settings.customGames) {
            const updated = settings.customGames.find((item) => item.id === game.id);
            if (updated) game.autoBackup = updated.autoBackup;
          }
          this.#autoBackupConfirmed.set(id, enabled);
          saved = true;
          if (this.#autoBackupDesired.get(id) === enabled) this.#autoBackupDesired.delete(id);
        } catch (error) {
          const title = this.scanResult?.onThisPc.find((game) => game.id === id)?.title ?? "this game";
          toast.error(`Could not update ${title}: ${message(error)}`);
        }
        const queued = this.#autoBackupDesired.get(id);
        this.#mirrorAutoBackup(id, queued ?? this.#autoBackupConfirmed.get(id)!);
        if (queued === undefined) {
          this.autoBackupPending.delete(id);
          this.#autoBackupConfirmed.delete(id);
        }
      }
    } finally {
      this.autoBackupPending.clear();
      this.#autoBackupDesired.clear();
      this.#autoBackupConfirmed.clear();
      if (this.scanResult) this.#updateBadge(this.scanResult);
      if (saved) notifyChange("gameSaves");
    }
  }
  async openGameFolder(game: GameSaveEntry) {
    try {
      await gameSavesApi.openGameFolder(game.id);
    } catch (error) {
      toast.error(`Could not open ${game.title}: ${message(error)}`);
    }
  }
  private async setting(key: string, action: () => Promise<void>) {
    if (this.locked) return;
    this.settingsBusy = key;
    this.#settingsRevision++;
    try {
      // Settings cannot change under a running scan; a background one yields.
      await this.#stopDiscovery();
      await action();
      if (SYNCED_SETTINGS.has(key)) notifyChange("gameSaves");
    } catch (error) {
      toast.error(message(error));
    } finally {
      this.settingsBusy = null;
    }
  }
  private beginOperation(kind: OperationKind, stage: OperationStage) {
    this.operation = { startedAt: Date.now(), kind, stage, done: 0, total: 0, current: null, note: null };
    this.error = null;
    if (kind !== "scan") this.failures = null;
  }
  private readonly onEvent = (event: GameSavesEvent) => {
    if (!this.operation) return;
    if (event.event === "stage") {
      if (this.operation.stage !== event.data.stage) {
        // A completed count belongs only to the stage that produced it. In
        // particular, restore preview must not leave the safety-copy stage
        // looking frozen at 100%.
        this.operation.stage = event.data.stage;
        this.operation.done = 0;
        this.operation.total = 0;
        this.operation.current = null;
        this.operation.note = null;
      }
    } else if (event.event === "progress") {
      this.operation.done = event.data.done;
      this.operation.total = event.data.total;
      this.operation.current = event.data.current;
      this.operation.note = null;
    } else this.operation.note = event.data.text;
  };
  private reportResult(result: GameSavesOperationResult) {
    const verb = result.kind === "backup" ? "Backed up" : "Restored";
    const failed = result.failedGames.length;
    if (result.processedGames) toast.success(`${verb} ${result.processedGames} ${result.processedGames === 1 ? "game" : "games"}.`);
    if (!failed) {
      this.failures = null;
      return;
    }
    const reasons = new Map((result.failures ?? []).map((failure) => [failure.game, failure]));
    this.failures = {
      kind: result.kind,
      items: result.failedGames.map((game) => reasons.get(game) ?? { game, reason: "No reason was given." }),
    };
    toast.error(`${failed} ${failed === 1 ? "game" : "games"} could not be ${result.kind === "backup" ? "backed up" : "restored"}. See the details on the page.`);
  }
  private keepFailedSelection(ids: string[], result: GameSavesOperationResult) {
    const failed = new Set(result.failedGames);
    // IPC returns failure titles, not IDs. Ambiguous duplicate titles stay
    // selected rather than hiding a failure from the user.
    const games = this.games;
    for (const id of ids) {
      const game = games.find((entry) => entry.id === id);
      if (game && !failed.has(game.title)) this.selected.delete(id);
    }
  }
  dismissFailures() {
    this.failures = null;
  }
  private fail(label: string, error: unknown) {
    this.error = `${label}: ${message(error)}`;
    toast.error(this.error);
  }
  private pruneSelection() {
    const known = new Set([...(this.scanResult?.onThisPc ?? []), ...(this.scanResult?.inBackup ?? [])].map((game) => game.id));
    for (const id of [...this.selected]) if (!known.has(id)) this.selected.delete(id);
    for (const game of this.scanResult?.inBackup ?? []) {
      const choice = this.snapshotChoices[game.id];
      if (!game.snapshots.some((snapshot) => snapshot.id === choice)) {
        if (game.snapshots[0]) this.snapshotChoices[game.id] = game.snapshots[0].id;
        else delete this.snapshotChoices[game.id];
      }
    }
  }
  /** After a backup or restore only the known games can have changed. */
  private async refreshAfterOperation() {
    try {
      this.page = await gameSavesApi.getState();
    } catch {
      // The operation result is still valid; a later visit/scan retries state.
    }
    if (this.scanResult) await this.refresh();
    else await this.scan(false);
  }
}
export const gameSavesState = new GameSavesState();
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    gameSavesState.stopWatcher();
    void gameSavesState.stopDiscovery();
  });
}
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const decimals = unit === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(decimals)} ${units[unit]}`;
}
function toDate(value: number | string): Date {
  return new Date(typeof value === "number" && value < 10_000_000_000 ? value * 1000 : value);
}
const dateTimeFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const dayFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
export function formatDate(value: number | string | null): string {
  if (value === null) return "Never";
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  return dateTimeFormat.format(date);
}
/** "5 minutes ago", "yesterday", "3 days ago"; the day itself after a month. */
export function formatRelative(value: number | string | null): string {
  if (value === null) return "Never";
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  const seconds = (Date.now() - date.getTime()) / 1000;
  if (seconds < 0 || seconds >= 30 * 86_400) {
    return dayFormat.format(date);
  }
  if (seconds < 60) return "Just now";
  const relative = relativeFormat;
  const text =
    seconds < 3_600
      ? relative.format(-Math.round(seconds / 60), "minute")
      : seconds < 86_400
        ? relative.format(-Math.round(seconds / 3_600), "hour")
        : relative.format(-Math.round(seconds / 86_400), "day");
  return text.charAt(0).toUpperCase() + text.slice(1);
}
