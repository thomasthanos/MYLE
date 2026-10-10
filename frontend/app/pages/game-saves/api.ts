// Typed bridge to the Game Saves commands in backend/src/game_saves/.
// Filesystem paths are discovered and validated by the backend; destructive
// actions are addressed by stable game/root/snapshot ids.
import { Channel, invoke } from "@tauri-apps/api/core";
export type GameSavesTab = "pc" | "backup";
export type GameSaveStatus =
  | "notBackedUp"
  | "backedUp"
  | "changedSinceBackup"
  | "backupOnly"
  | "needsLocation"
  | "unknown"
  | "error";
export type BackupSchedule = "off" | "daily" | "weekly";
/** quick: only the games already found (seconds); full: the whole database. */
export type ScanMode = "quick" | "full";
export type ScheduleWeekday =
  | "sunday"
  | "monday"
  | "tuesday"
  | "wednesday"
  | "thursday"
  | "friday"
  | "saturday";
export type RootStore = "steam" | "epic" | "gog" | "gogGalaxy" | "uplay" | "origin" | "otherWindows";
export type RootSource = "automatic" | "manual";
export type OperationKind = "scan" | "updateDatabase" | "backup" | "restore" | "scheduledBackup";
export type OperationStage =
  | "preparing"
  | "scanning"
  | "updatingDatabase"
  | "creatingSafetyBackup"
  | "backingUp"
  | "waitingForOneDrive"
  | "restoring"
  | "finishing";
export interface GameRoot {
  id: string;
  path: string;
  store: RootStore;
  source: RootSource;
}
export interface CustomGame {
  id: string;
  name: string;
  paths: string[];
  installPath: string | null;
  autoBackup: boolean;
}
export interface GameSavesSettings {
  backupFolder: string | null;
  schedule: BackupSchedule;
  scheduleTime: string;
  scheduleWeekday: ScheduleWeekday;
  lastScheduledAttempt: number | null;
  lastScheduledSuccess: number | null;
  lastScheduledResult: ScheduledBackupResult | null;
  roots: GameRoot[];
  autoBackupExcludedGameIds: string[];
  customGames: CustomGame[];
  pathMappings: PathMapping[];
}
export interface ScheduledBackupResult {
  attemptedAt: number;
  completedAt: number;
  processedGames: number;
  failedGames: string[];
  error: string | null;
}
export interface PathMapping {
  gameId: string;
  source: string;
  target: string;
}
export type CloudProvider = "dropbox" | "googleDrive" | "mega" | "oneDrive";
export const CLOUD_PROVIDERS: { id: CloudProvider; name: string }[] = [
  { id: "dropbox", name: "Dropbox" },
  { id: "googleDrive", name: "Google Drive" },
  { id: "mega", name: "MEGA" },
  { id: "oneDrive", name: "OneDrive" },
];
export interface DetectedFolder {
  provider: CloudProvider;
  /** Which account, when a provider has several ("Dropbox Business"). */
  label: string;
  /** The provider's synced folder. */
  path: string;
  /** Where backups go inside it (created when chosen). */
  backupPath: string;
}
export interface GameSavesPageState {
  settings: GameSavesSettings;
  cloudFolders: DetectedFolder[];
  engineAvailable: boolean;
  engineVersion: string | null;
  activeOperation: OperationKind | null;
  databaseGames: number;
  databaseUpdatedAt: number | null;
  undoRestore: UndoRestore | null;
  /** The last scan saved on disk, shown at once while a fresh one runs. */
  cachedScan: GameSavesScan | null;
}
export interface UndoRestore {
  id: string;
  createdAt: number;
  expiresAt: number;
  games: string[];
}
export interface GameSavesStats {
  localGames: number;
  backupGames: number;
  totalBytes: number;
  databaseGames: number;
}
export interface BackupSnapshot {
  id: string;
  timestamp: string;
  bytes: number;
  label: string | null;
  isSafety: boolean;
}
export interface GameSaveEntry {
  id: string;
  title: string;
  status: GameSaveStatus;
  platformBadges: string[];
  fileCount: number;
  totalBytes: number;
  lastSaveAt: number | null;
  lastBackupAt: string | null;
  paths: string[];
  autoBackup: boolean;
  hasLocalData: boolean;
  hasBackup: boolean;
  error: string | null;
  snapshots: BackupSnapshot[];
  /** Its Steam app id in the save database, for its cover. */
  steamId?: number | null;
}
export interface GameSavesScan {
  generatedAt: number;
  stats: GameSavesStats;
  onThisPc: GameSaveEntry[];
  inBackup: GameSaveEntry[];
  /** Unix seconds of the last full scan; a quick refresh keeps it. */
  fullScanAt: number;
  quick: boolean;
  /** New games may be missing: a full scan should run in the background. */
  discoveryDue: boolean;
  /** The backup folder, when its drive is not connected (Google Drive not running). */
  backupUnreachable?: string | null;
}
export interface RestoreSelection {
  gameId: string;
  snapshotId: string;
}
export interface GameFailure {
  game: string;
  reason: string;
  /** The end of the path of the file it happened to. */
  file?: string | null;
}
export interface GameSavesOperationResult {
  kind: "backup" | "restore";
  processedGames: number;
  processedBytes: number;
  failedGames: string[];
  /** Why each failed game failed, as far as the engine said. */
  failures: GameFailure[];
  safetyBackupPath: string | null;
}
export interface DatabaseUpdate {
  games: number;
  updatedAt: number;
}
export type GameSavesEvent =
  | { event: "stage"; data: { stage: OperationStage } }
  | { event: "progress"; data: { done: number; total: number; current: string | null } }
  | { event: "message"; data: { text: string } };
function channel(onEvent: (event: GameSavesEvent) => void): Channel<GameSavesEvent> {
  const value = new Channel<GameSavesEvent>();
  value.onmessage = onEvent;
  return value;
}
export const gameSavesApi = {
  getState: () => invoke<GameSavesPageState>("game_saves_get_state"),
  pickBackupFolder: () => invoke<GameSavesSettings | null>("game_saves_pick_backup_folder"),
  /** Backs up into `<folder>\\Make Your Life Easier\\Game Saves Backups`, creating it. Without a
   *  detected `path` the user is asked where the provider's folder is; null when cancelled. */
  useCloudFolder: (provider: CloudProvider, path: string | null) =>
    invoke<GameSavesSettings | null>("game_saves_use_cloud_folder", { provider, path }),
  openBackupFolder: () => invoke<void>("game_saves_open_backup_folder"),
  openGameFolder: (gameId: string) => invoke<void>("game_saves_open_game_folder", { gameId }),
  detectCloudFolders: () => invoke<DetectedFolder[]>("game_saves_detect_cloud_folders"),
  refreshRoots: () => invoke<GameSavesSettings>("game_saves_refresh_roots"),
  addRoot: (store: RootStore) => invoke<GameSavesSettings | null>("game_saves_add_root", { store }),
  removeRoot: (rootId: string) => invoke<GameSavesSettings>("game_saves_remove_root", { rootId }),
  setSchedule: (schedule: BackupSchedule, time: string, weekday: ScheduleWeekday) =>
    invoke<GameSavesSettings>("game_saves_set_schedule", { schedule, time, weekday }),
  setGameAutoBackup: (gameId: string, enabled: boolean) =>
    invoke<GameSavesSettings>("game_saves_set_game_auto_backup", { gameId, enabled }),
  pickFolder: (title: string) => invoke<string | null>("game_saves_pick_folder", { title }),
  upsertCustomGame: (game: CustomGame) =>
    invoke<GameSavesSettings>("game_saves_upsert_custom_game", { game }),
  removeCustomGame: (gameId: string) =>
    invoke<GameSavesSettings>("game_saves_remove_custom_game", { gameId }),
  setPathMapping: (mapping: PathMapping) =>
    invoke<GameSavesSettings>("game_saves_set_path_mapping", { mapping }),
  removePathMapping: (gameId: string, source: string) =>
    invoke<GameSavesSettings>("game_saves_remove_path_mapping", { gameId, source }),
  scan: (mode: ScanMode, onEvent: (event: GameSavesEvent) => void) =>
    invoke<GameSavesScan>("game_saves_scan", { mode, onEvent: channel(onEvent) }),
  updateDatabase: (onEvent: (event: GameSavesEvent) => void) =>
    invoke<DatabaseUpdate>("game_saves_update_database", { onEvent: channel(onEvent) }),
  backup: (gameIds: string[], onEvent: (event: GameSavesEvent) => void) =>
    invoke<GameSavesOperationResult>("game_saves_backup", { gameIds, onEvent: channel(onEvent) }),
  restore: (selections: RestoreSelection[], onEvent: (event: GameSavesEvent) => void) =>
    invoke<GameSavesOperationResult>("game_saves_restore", { selections, onEvent: channel(onEvent) }),
  undoLastRestore: (onEvent: (event: GameSavesEvent) => void) =>
    invoke<GameSavesOperationResult>("game_saves_undo_last_restore", { onEvent: channel(onEvent) }),
  cancel: () => invoke<void>("game_saves_cancel"),
  /** Steam covers as `data:` URLs, by app id: from this PC, and from Steam when `online`. */
  covers: (ids: number[], online: boolean) => invoke<Record<string, string>>("game_saves_covers", { ids, online }),
};
