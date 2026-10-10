// Typed bridge to the Project Backups commands in backend/src/project_backups/.
// Projects are addressed by id and backups by the id from their list; every
// path is resolved and checked by the backend.
import { Channel, invoke } from "@tauri-apps/api/core";

export type BackupProvider = "googleDrive" | "dropbox";

export interface LastBackup {
  name: string;
  createdAt: number;
  fileCount: number;
  zipSize: number;
}

/** How the last backup attempt of a project ended. */
export interface LastResult {
  at: number;
  ok: boolean;
  cancelled: boolean;
  code: ResultCode | null;
  error: string | null;
}

export interface Project {
  id: string;
  name: string;
  sourcePath: string;
  /** The folder under `Projects Backup` and the start of every backup name. */
  appName: string;
  /** A program (MyApp.exe) to close before backing up. */
  closeApp: string | null;
  extraExclusions: string[];
  keep: string[];
  lastBackup: LastBackup | null;
  /** Set by every backup attempt (also failed and cancelled ones). */
  lastResult?: LastResult | null;
}

export interface ProjectBackupsSettings {
  version: number;
  provider: BackupProvider | null;
  cloudFolder: string | null;
  projects: Project[];
  exclusions: string[];
  smartBuild: boolean;
  followGitignore: boolean;
  imported: boolean;
}

export interface CloudChoice {
  provider: BackupProvider;
  label: string;
  path: string;
  /** The folder is there right now (Google Drive's drive exists only while it runs). */
  available: boolean;
}

export interface ImportReport {
  added: string[];
  alreadyKnown: number;
  from: string | null;
}

export interface PageState {
  settings: ProjectBackupsSettings;
  clouds: CloudChoice[];
  backupRoot: string | null;
  running: boolean;
  defaultExclusions: string[];
  missingSources: string[];
  imported: ImportReport | null;
}

export type Stage =
  | "preparing"
  | "startingCloud"
  | "closingApp"
  | "scanning"
  | "zipping"
  | "verifying"
  | "checkingCompleteness"
  | "copying"
  | "verifyingCopy"
  | "finishing";

export type ProjectBackupsEvent =
  | { event: "project"; data: { index: number; total: number; projectId: string; name: string } }
  | { event: "stage"; data: { stage: Stage } }
  | { event: "progress"; data: { doneBytes: number; totalBytes: number; doneFiles: number; totalFiles: number } }
  | { event: "message"; data: { text: string } };

export interface SkippedItem {
  path: string;
  reason: string;
}

export interface BackupOutcome {
  name: string;
  zipPath: string;
  monthFolder: string;
  version: number;
  fileCount: number;
  totalBytes: number;
  zipSize: number;
  excludedCount: number;
  skipped: SkippedItem[];
  linkedFiles: string[];
  finalCheckOk: boolean;
  createdAt: number;
}

export type ResultCode =
  | "CANCELLED"
  | "SOURCE_MISSING"
  | "NO_PROVIDER"
  | "CLOUD_NOT_INSTALLED"
  | "CLOUD_NOT_READY"
  | "CLOUD_OFFLINE"
  | "DESTINATION_MISSING"
  | "NO_FILES"
  | "NAME_TAKEN";

export interface ProjectResult {
  projectId: string;
  name: string;
  ok: boolean;
  code: ResultCode | null;
  error: string | null;
  outcome: BackupOutcome | null;
}

export interface BackupRun {
  results: ProjectResult[];
  cancelled: boolean;
  settings: ProjectBackupsSettings;
}

export interface BackupEntry {
  /** `<month folder>/<file or folder name>`. */
  id: string;
  name: string;
  monthFolder: string;
  year: number;
  month: number;
  version: number;
  day: number;
  kind: "zip" | "folder";
  size: number | null;
  modified: number | null;
  broken: boolean;
}

/** Why something is left out: a pattern, a detected build/cache folder, .gitignore, or MYLE's own files. */
export type RuleKind = "pattern" | "detected" | "gitignore" | "internal";

export interface ExcludedItem {
  path: string;
  isDir: boolean;
  rule: string;
  kind: RuleKind;
}

export interface RuleCount {
  rule: string;
  kind: RuleKind;
  folders: number;
  files: number;
}

/** One line of the "what will be backed up" tree, in display order. */
export interface PreviewNode {
  path: string;
  name: string;
  depth: number;
  isDir: boolean;
  /** Bytes backed up (folders: all inside); null for what is left out. */
  size: number | null;
  files: number;
  rule: string | null;
  kind: RuleKind | null;
  /** Files of this folder not listed (the tree has a size limit). */
  hiddenFiles: number;
}

export interface Preview {
  fileCount: number;
  totalBytes: number;
  excluded: ExcludedItem[];
  excludedTotal: number;
  byRule: RuleCount[];
  skipped: SkippedItem[];
  envFiles: string[];
  tree: PreviewNode[];
  treeHiddenFiles: number;
}

export interface PreviewRequest {
  sourcePath?: string | null;
  projectId?: string | null;
  patterns?: string[] | null;
  extraExclusions?: string[] | null;
  keep?: string[] | null;
  smartBuild?: boolean | null;
  followGitignore?: boolean | null;
}

export type ChangeStatus = "added" | "modified" | "deleted";

export type FileKind = "text" | "image" | "binary";

export interface Change {
  path: string;
  kind: FileKind;
  status: ChangeStatus;
  oldName: string | null;
  newName: string | null;
  oldSize: number | null;
  newSize: number | null;
  /** A deleted file that is still in the project folder: the newer backup is missing it. */
  stillInSource: boolean;
}

export interface Comparison {
  changes: Change[];
  added: number;
  modified: number;
  deleted: number;
  unchanged: number;
}

export interface CompareResult {
  oldId: string;
  newId: string;
  comparison: Comparison;
}

export type RowKind = "same" | "removed" | "added" | "changed" | "gap";

export interface DiffRow {
  kind: RowKind;
  oldLine: number | null;
  oldText: string | null;
  newLine: number | null;
  newText: string | null;
}

export interface SideInfo {
  size: number;
  /** Hex SHA-256 (files up to 512 MB). */
  sha256: string | null;
}

export type FileDiff =
  | {
      kind: "text";
      rows: DiffRow[];
      identical: boolean;
      lineEndingsDiffer: boolean;
      truncated: boolean;
      oldLines: number;
      newLines: number;
      encoding: string | null;
    }
  | {
      kind: "image";
      mime: string;
      /** data: URLs; null when that side is missing or too large to show. */
      old: string | null;
      new: string | null;
      oldInfo: SideInfo | null;
      newInfo: SideInfo | null;
      /** The source, for SVG. */
      rows: DiffRow[] | null;
    }
  | { kind: "binary"; old: SideInfo | null; new: SideInfo | null; tooLarge: boolean };

export interface CompareProgress {
  done: number;
  total: number;
}

/** A project folder against its newest backup. */
export interface ChangedFile {
  path: string;
  status: "added" | "modified" | "deleted";
  size: number | null;
}

export interface ProjectChanges {
  projectId: string;
  state: "upToDate" | "changed" | "noBackup" | "missing";
  backupId: string | null;
  added: number;
  modified: number;
  deleted: number;
  /** At most 300; the counts are whole. */
  files: ChangedFile[];
  checkedAt: number;
}

export type Job = "backup" | "compare";

/** Stands for the project folder itself in a comparison. */
export const SOURCE_ID = "source";

export type OpenTarget = "root" | "backups" | "source";

function channel(onEvent: (event: ProjectBackupsEvent) => void): Channel<ProjectBackupsEvent> {
  const value = new Channel<ProjectBackupsEvent>();
  value.onmessage = onEvent;
  return value;
}

export const projectBackupsApi = {
  /** On the first start this also imports backup_projects' projects. */
  getState: () => invoke<PageState>("project_backups_get_state"),
  import: () => invoke<[ProjectBackupsSettings, ImportReport]>("project_backups_import"),
  /** A detected `path`, or null to pick the provider's folder; null back when that was cancelled. */
  setProvider: (provider: BackupProvider, path: string | null) =>
    invoke<ProjectBackupsSettings | null>("project_backups_set_provider", { provider, path }),
  pickFolder: (title: string) => invoke<string | null>("project_backups_pick_folder", { title }),
  /** Returns the settings and the project's id (new projects get one). */
  saveProject: (project: Project, requireExistingSource: boolean) =>
    invoke<[ProjectBackupsSettings, string]>("project_backups_save_project", { project, requireExistingSource }),
  removeProject: (projectId: string) =>
    invoke<ProjectBackupsSettings>("project_backups_remove_project", { projectId }),
  setExclusions: (patterns: string[], smartBuild: boolean, followGitignore: boolean) =>
    invoke<ProjectBackupsSettings>("project_backups_set_exclusions", { patterns, smartBuild, followGitignore }),
  /** A newer preview stops the one still running. */
  preview: (request: PreviewRequest) => invoke<Preview>("project_backups_preview", { request }),
  cancelPreview: () => invoke<void>("project_backups_cancel_preview"),
  changes: (projectIds: string[], fresh: boolean) => invoke<ProjectChanges[]>("project_backups_changes", { projectIds, fresh }),
  watch: () => invoke<void>("project_backups_watch"),
  backup: (projectIds: string[], onEvent: (event: ProjectBackupsEvent) => void) =>
    invoke<BackupRun>("project_backups_backup", { projectIds, onEvent: channel(onEvent) }),
  /** Cancels the running `job` (any job when null). */
  cancel: (job: Job | null = null) => invoke<boolean>("project_backups_cancel", { job }),
  /** Starts Google Drive or Dropbox; resolves to a line for the user. */
  startCloud: (provider: BackupProvider) => invoke<string>("project_backups_start_cloud", { provider }),
  list: (projectId: string) => invoke<BackupEntry[]>("project_backups_list", { projectId }),
  /** The backend puts the older side first; `SOURCE_ID` is always the newer one. */
  compare: (projectId: string, firstId: string, secondId: string, onProgress: (progress: CompareProgress) => void) => {
    const progress = new Channel<CompareProgress>();
    progress.onmessage = onProgress;
    return invoke<CompareResult>("project_backups_compare", { projectId, firstId, secondId, onProgress: progress });
  },
  fileDiff: (projectId: string, oldId: string, newId: string, oldName: string | null, newName: string | null) =>
    invoke<FileDiff>("project_backups_file_diff", { projectId, oldId, newId, oldName, newName }),
  open: (target: OpenTarget, projectId: string | null = null, backupId: string | null = null) =>
    invoke<void>("project_backups_open", { target, projectId, backupId }),
};
