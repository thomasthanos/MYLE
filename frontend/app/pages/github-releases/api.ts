// Typed bridge to the GitHub Releases commands in backend/src/github_releases/.
// Projects are addressed by id (`<repo id>` or `<repo id>/<app folder>`),
// files by their path in the repository's status; the backend checks every
// path. The GitHub token and AI keys never come back here.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { FileDiff } from "../project-backups/api";

export type { FileDiff };

// ─── Shared ────────────────────────────────────────────────────────────────

/** A failure the page can act on: `code` says what, `message` is for people. */
export interface Problem {
  code: string;
  message: string;
  details: string | null;
  /** AI failures: the provider that failed and the one to offer next. */
  data?: { provider?: ProviderId; next?: ProviderId | null; nextName?: string | null; retryAfter?: number | null } & Record<string, unknown>;
}

/** The backend's errors are plain text or a Problem as JSON. */
export function problemOf(error: unknown): Problem {
  const text = error instanceof Error ? error.message : String(error);
  try {
    const value = JSON.parse(text) as Partial<Problem>;
    if (value && typeof value.code === "string" && typeof value.message === "string") {
      return { code: value.code, message: value.message, details: value.details ?? null, data: value.data };
    }
  } catch {
    // Plain text.
  }
  return { code: "FAILED", message: text, details: null };
}

export const messageOf = (error: unknown): string => problemOf(error).message;

// ─── Page ──────────────────────────────────────────────────────────────────

export type ProviderId = "groq" | "gemini" | "openrouter" | "deepseek" | "ollama";

export interface ProviderView {
  id: ProviderId;
  name: string;
  baseUrl: string;
  defaultModel: string;
  keyUrl: string;
  needsKey: boolean;
  free: string;
  privacy: string;
  model: string;
  ready: boolean;
  keyHint: string | null;
}

export interface AiView {
  providers: ProviderView[];
  order: ProviderId[];
  ollamaUrl: string | null;
  ollamaEnabled: boolean;
}

export interface Account {
  login: string;
  name: string | null;
  avatarUrl: string | null;
  /** "gh" (the GitHub CLI's sign-in) or "token" (a pasted token). */
  source: string;
  scopes: string[];
}

export interface GbrImport {
  at: number;
  project: string | null;
  deepseekKey: boolean;
  plaintextRemoved: boolean;
  configPath: string | null;
  error: string | null;
  dismissed: boolean;
}

export interface RepoBrief {
  id: string;
  path: string;
  name: string;
  exists: boolean;
}

export interface PageState {
  repos: RepoBrief[];
  account: Account | null;
  git: string | null;
  gh: string | null;
  ai: AiView;
  gbrImport: GbrImport | null;
}

// ─── Status ────────────────────────────────────────────────────────────────

export type FileKind = "packageJson" | "packageLock" | "cargoToml" | "tauriConf" | "pyproject" | "csproj" | "extensionManifest";

export interface VersionFile {
  path: string;
  kind: FileKind;
  version: string;
  skipped: boolean;
}

export interface Versions {
  files: VersionFile[];
  current: string | null;
  mismatched: string[];
}

export type BuildKind = "tauri" | "electron" | "vite" | "node" | "rust" | "dotnet" | "python" | "go" | "flutter" | "extension";

export interface LastTag {
  name: string;
  /** Unix seconds. */
  date: number;
  /** A shared `v…` tag from before the app had its own prefix. */
  legacy: boolean;
}

export interface Workflow {
  file: string;
  name: string | null;
  tags: string[];
}

export interface Remote {
  name: string;
  url: string;
  host: string | null;
  owner: string | null;
  repo: string | null;
  ssh: boolean;
}

export interface BranchInfo {
  branch: string | null;
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
}

export interface EntryStatus {
  id: string;
  repoId: string;
  name: string;
  sub: string | null;
  dir: string;
  monorepo: boolean;
  tagPrefix: string;
  versions: Versions;
  buildKinds: BuildKind[];
  lastTag: LastTag | null;
  changes: number;
  releaseWorkflow: string | null;
  releaseMode: "local" | "actions";
  skipVersionFiles: string[];
}

export interface RepoStatus {
  repoId: string;
  path: string;
  exists: boolean;
  isRepo: boolean;
  remote: Remote | null;
  branch: BranchInfo;
  changes: number;
  entries: EntryStatus[];
  workflows: Workflow[];
  problem: string | null;
}

export interface Asset {
  id: number;
  name: string;
  size: number;
  downloadCount: number;
  browserDownloadUrl: string;
}

export interface Release {
  id: number;
  tagName: string;
  name: string | null;
  body: string | null;
  draft: boolean;
  prerelease: boolean;
  createdAt: string | null;
  publishedAt: string | null;
  htmlUrl: string;
  uploadUrl: string;
  targetCommitish: string;
  assets: Asset[];
}

export interface Run {
  id: number;
  name: string | null;
  displayTitle: string | null;
  status: string | null;
  conclusion: string | null;
  htmlUrl: string;
  headBranch: string | null;
  headSha: string;
  event: string;
  createdAt: string | null;
  updatedAt: string | null;
}

export interface Step {
  name: string;
  status: string | null;
  conclusion: string | null;
  number: number;
}

export interface WorkflowJob {
  id: number;
  name: string;
  status: string | null;
  conclusion: string | null;
  htmlUrl: string | null;
  steps: Step[];
}

export interface RemoteInfo {
  releases: Release[];
  lastRelease: Record<string, Release>;
  run: Run | null;
  problem: Problem | null;
}

export interface FoundRepo {
  path: string;
  name: string;
  known: boolean;
  apps: string[];
}

export interface AddResult {
  state: PageState;
  added: string | null;
  found: FoundRepo[];
}

export interface EntrySettings {
  skipVersionFiles?: string[];
  releaseMode?: "local" | "actions" | "";
  buildCommand?: string | null;
  buildChoice?: string | null;
}

// ─── Changes ───────────────────────────────────────────────────────────────

export type ChangeKind = "added" | "modified" | "deleted" | "renamed" | "copied" | "typeChanged" | "untracked" | "conflicted";
export type Staged = "all" | "partial" | "none";

export interface FileChange {
  path: string;
  origPath: string | null;
  kind: ChangeKind;
  staged: Staged;
}

export interface GitStatus {
  branch: BranchInfo;
  changes: FileChange[];
}

export type GitEvent = { event: "line"; data: { text: string; replace: boolean } };

export interface NetOutcome {
  ok: boolean;
  problem: Problem | null;
  setUpstream: boolean;
  usedToken: boolean;
  output: string;
}

export interface CommitOutcome {
  commit: string | null;
  problem: Problem | null;
  push: NetOutcome | null;
  status: GitStatus;
}

export interface Answer {
  text: string;
  provider: ProviderId;
  model: string;
}

// ─── Build ─────────────────────────────────────────────────────────────────

export interface BuildOption {
  id: string;
  command: string;
  label: string;
  detail: string | null;
}

export interface BuildPlan {
  options: BuildOption[];
  packageManager: string | null;
  needsInstall: boolean;
  installCommand: string | null;
}

export interface BuildStats {
  command: string;
  durationMs: number;
  lines: number;
  at: number;
}

export interface BuildInfo {
  plan: BuildPlan;
  command: string | null;
  customCommand: string | null;
  choice: string | null;
  lastBuild: BuildStats | null;
  running: boolean;
}

export type Severity = "error" | "warning";

export interface Diagnostic {
  severity: Severity;
  message: string;
  file: string | null;
  line: number | null;
  column: number | null;
  code: string | null;
  tool: string;
  logLine: number;
}

export interface LogLine {
  index: number;
  text: string;
  err: boolean;
  /** Replaces the line before it (a progress bar redrawn in place). */
  replace: boolean;
  severity: Severity | null;
}

export type BuildEvent =
  | { event: "started"; data: { command: string; dir: string; logPath: string | null } }
  | { event: "lines"; data: { lines: LogLine[] } }
  | { event: "diagnostic"; data: { diagnostic: Diagnostic } }
  | { event: "stage"; data: { text: string } }
  | { event: "progress"; data: { percent: number; estimated: boolean } };

export interface BuildOutcome {
  ok: boolean;
  cancelled: boolean;
  exitCode: number | null;
  durationMs: number;
  lines: number;
  errors: number;
  warnings: number;
  diagnostics: Diagnostic[];
  logPath: string | null;
  failure: string | null;
}

export interface Artifact {
  path: string;
  rel: string;
  name: string;
  size: number;
  /** Unix ms. */
  modified: number;
  kind: "installer" | "program" | "package" | "archive" | "update" | "signature" | "file";
}

export interface BuildResult {
  outcome: BuildOutcome;
  artifacts: Artifact[];
  selected: string[];
  startedAt: number;
}

// ─── Release ───────────────────────────────────────────────────────────────

export type Mode = "local" | "actions";

export interface ReleaseInfo {
  versions: Versions;
  next: Partial<Record<"patch" | "minor" | "major", string>>;
  tagPrefix: string;
  lastTag: LastTag | null;
  commits: { sha: string; subject: string; author: string; date: number }[];
  workflows: Workflow[];
  mode: Mode;
  notesDir: string | null;
  branch: string | null;
  buildCommand: string | null;
}

export interface Commit {
  sha: string;
  subject: string;
  author: string;
  /** Unix seconds. */
  date: number;
}

/** The newest commits, and how many came after the last release. */
export interface RecentCommits {
  commits: Commit[];
  unreleased: number;
  lastTag: string | null;
}

export type CheckState = "ok" | "warn" | "fail";

export interface Check {
  id: string;
  state: CheckState;
  message: string;
  /** What fixes it: "pull", "commit", "connect", "build". */
  fix: string | null;
}

export interface Notes {
  title: string | null;
  notes: string;
  provider: ProviderId;
  model: string;
}

export interface ReleaseRequest {
  entryId: string;
  version: string;
  mode: Mode;
  build: boolean;
  command: string | null;
  includeChanges: boolean;
  commitMessage: string | null;
  title: string;
  notes: string;
  assets: string[] | null;
  draft: boolean;
  prerelease: boolean;
  makeLatest: boolean;
  notesFile: string | null;
}

export type StepId = "check" | "version" | "build" | "commit" | "push" | "tag" | "release" | "upload" | "publish" | "workflow";
export type StepState = "running" | "done" | "failed" | "skipped";

export type ReleaseEvent =
  | { event: "step"; data: { id: StepId; state: StepState; message: string | null } }
  | { event: "build"; data: { event: BuildEvent } }
  | { event: "upload"; data: { name: string; sent: number; total: number; index: number; count: number } }
  | { event: "workflow"; data: { run: Run | null; jobs: WorkflowJob[] } }
  | { event: "log"; data: { text: string } };

export interface Resume {
  releaseId: number | null;
  pendingAssets: string[];
  publish: boolean;
  makeLatest: boolean;
  tag: string | null;
  commit: string | null;
}

export interface ReleaseOutcome {
  ok: boolean;
  cancelled: boolean;
  version: string;
  tag: string;
  commit: string | null;
  release: Release | null;
  run: Run | null;
  problem: Problem | null;
  rolledBack: boolean;
  resume: Resume | null;
  build: BuildOutcome | null;
  artifacts: Artifact[];
}

export interface Deleted {
  releaseId: number;
  tag: string;
  ok: boolean;
  tagDeleted: boolean;
  error: string | null;
}

export interface GhStatus {
  installed: boolean;
  signedIn: boolean;
}

export interface AiChanges {
  order?: ProviderId[];
  models?: Partial<Record<ProviderId, string>>;
  ollamaUrl?: string | null;
  ollamaEnabled?: boolean;
}

// ─── Commands ──────────────────────────────────────────────────────────────

function channel<T>(on: (event: T) => void): Channel<T> {
  const c = new Channel<T>();
  c.onmessage = on;
  return c;
}

export const githubReleasesApi = {
  getState: () => invoke<PageState>("github_releases_get_state"),
  dismissImport: () => invoke<void>("github_releases_dismiss_import"),

  status: (repoId: string) => invoke<RepoStatus>("github_releases_status", { repoId }),
  fetch: (repoId: string, quiet: boolean) => invoke<RepoStatus>("github_releases_fetch", { repoId, quiet }),
  remote: (repoId: string) => invoke<RemoteInfo>("github_releases_remote", { repoId }),
  addFolder: () => invoke<AddResult | null>("github_releases_add_folder"),
  scanFolder: () => invoke<FoundRepo[] | null>("github_releases_scan_folder"),
  cancelScan: () => invoke<boolean>("github_releases_cancel_scan"),
  addRepos: (paths: string[]) => invoke<PageState>("github_releases_add_repos", { paths }),
  removeRepo: (repoId: string) => invoke<PageState>("github_releases_remove_repo", { repoId }),
  setEntry: (entryId: string, changes: EntrySettings) => invoke<void>("github_releases_set_entry", { entryId, changes }),

  changes: (repoId: string) => invoke<GitStatus>("github_releases_changes", { repoId }),
  fileDiff: (repoId: string, path: string) => invoke<FileDiff>("github_releases_file_diff", { repoId, path }),
  stage: (repoId: string, paths: string[], stage: boolean) => invoke<GitStatus>("github_releases_stage", { repoId, paths, stage }),
  commit: (repoId: string, message: string, push: boolean, on: (e: GitEvent) => void) =>
    invoke<CommitOutcome>("github_releases_commit", { repoId, message, push, onEvent: channel(on) }),
  push: (repoId: string, on: (e: GitEvent) => void) => invoke<NetOutcome>("github_releases_push", { repoId, onEvent: channel(on) }),
  pull: (repoId: string, on: (e: GitEvent) => void) => invoke<NetOutcome>("github_releases_pull", { repoId, onEvent: channel(on) }),
  cancel: (repoId: string) => invoke<boolean>("github_releases_cancel", { repoId }),
  aiCommitMessage: (repoId: string, provider: ProviderId | null) =>
    invoke<Answer>("github_releases_ai_commit_message", { repoId, provider }),

  buildInfo: (entryId: string) => invoke<BuildInfo>("github_releases_build_info", { entryId }),
  build: (entryId: string, command: string, install: boolean, on: (e: BuildEvent) => void) =>
    invoke<BuildResult>("github_releases_build", { entryId, command, install, onEvent: channel(on) }),
  artifacts: (entryId: string, since: number) => invoke<Artifact[]>("github_releases_artifacts", { entryId, since }),
  checksum: (entryId: string, path: string) => invoke<string>("github_releases_checksum", { entryId, path }),
  openInEditor: (entryId: string, file: string, line: number | null, column: number | null) =>
    invoke<void>("github_releases_open_in_editor", { entryId, file, line, column }),
  reveal: (entryId: string, path: string | null) => invoke<void>("github_releases_reveal", { entryId, path }),

  releaseInfo: (entryId: string) => invoke<ReleaseInfo>("github_releases_release_info", { entryId }),
  recentCommits: (entryId: string, max: number) => invoke<RecentCommits>("github_releases_recent_commits", { entryId, max }),
  preflight: (entryId: string, version: string, mode: Mode, includeChanges: boolean, build: boolean) =>
    invoke<Check[]>("github_releases_preflight", { entryId, version, mode, includeChanges, build }),
  aiNotes: (entryId: string, version: string, provider: ProviderId | null, polish: string | null) =>
    invoke<Notes>("github_releases_ai_notes", { entryId, version, provider, polish }),
  release: (request: ReleaseRequest, on: (e: ReleaseEvent) => void) =>
    invoke<ReleaseOutcome>("github_releases_release", { request, onEvent: channel(on) }),
  resume: (entryId: string, resume: Resume, on: (e: ReleaseEvent) => void) =>
    invoke<ReleaseOutcome>("github_releases_resume", { entryId, resume, onEvent: channel(on) }),
  watch: (entryId: string, commit: string, tag: string | null, on: (e: ReleaseEvent) => void) =>
    invoke<Run>("github_releases_watch", { entryId, commit, tag, onEvent: channel(on) }),

  listReleases: (repoId: string) => invoke<Release[]>("github_releases_list_releases", { repoId }),
  updateRelease: (repoId: string, releaseId: number, title: string, notes: string, prerelease: boolean) =>
    invoke<Release>("github_releases_update_release", { repoId, releaseId, title, notes, prerelease }),
  deleteReleases: (repoId: string, releaseIds: number[], deleteTags: boolean) =>
    invoke<Deleted[]>("github_releases_delete_releases", { repoId, releaseIds, deleteTags }),
  aiCombine: (repoId: string, releaseIds: number[], provider: ProviderId | null) =>
    invoke<Notes>("github_releases_ai_combine", { repoId, releaseIds, provider }),

  ghStatus: () => invoke<GhStatus>("github_releases_gh_status"),
  connectGh: () => invoke<PageState>("github_releases_connect_gh"),
  connectToken: (token: string) => invoke<PageState>("github_releases_connect_token", { token }),
  disconnect: () => invoke<PageState>("github_releases_disconnect"),
  ghLogin: () => invoke<void>("github_releases_gh_login"),
  installTool: (tool: "gh" | "git") => invoke<void>("github_releases_install_tool", { tool }),

  aiSetKey: (provider: ProviderId, key: string) => invoke<AiView>("github_releases_ai_set_key", { provider, key }),
  aiRemoveKey: (provider: ProviderId) => invoke<AiView>("github_releases_ai_remove_key", { provider }),
  aiSetSettings: (changes: AiChanges) => invoke<AiView>("github_releases_ai_set_settings", { changes }),
  aiOllamaModels: () => invoke<string[]>("github_releases_ai_ollama_models"),
  aiTest: (provider: ProviderId) => invoke<Answer>("github_releases_ai_test", { provider }),
};
