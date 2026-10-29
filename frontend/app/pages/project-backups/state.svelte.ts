// Page state of Project Backups: the settings, a running backup and its progress, and the open dialogs.
import { badges } from "../../../lib/badges.svelte";
import { toast } from "../../../lib/toast.svelte";
import { confirm } from "../../../lib/confirm.svelte";
import { nav } from "../../../lib/nav.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { projectBackupsApi as api, CHANGES_EVENT, SOURCE_ID, type BackupEntry, type BackupProvider, type CloudChoice, type PageState, type Project, type ProjectBackupsEvent, type ProjectResult, type Stage } from "./api";
import { formatRelative } from "../game-saves/state.svelte";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";
import type { ProjectChanges } from "./api";
/** 30 days old is called old, a backup within 7 days counts as up to date. */
const STALE_DAYS = 30;
const FRESH_DAYS = 7;
/** The full change check and reminder, independently of live folder watching. */
const CHANGE_CHECK_EVERY = 3 * 24 * 60 * 60 * 1000;
const LAST_CHANGE_CHECK_KEY = "myle.projectBackups.lastChangeCheck";
export { formatBytes, formatDate, formatRelative } from "../game-saves/state.svelte";
export const stageLabels: Record<Stage, string> = {
  preparing: "Preparing", startingCloud: "Starting the cloud app", closingApp: "Closing the app", scanning: "Reading the project",
  zipping: "Zipping", verifying: "Checking the zip", checkingCompleteness: "Checking nothing is missing", copying: "Copying to the cloud folder",
  verifyingCopy: "Checking the copy", finishing: "Finishing",
};
/** What each stage does, in plain words (shown under the stage). */
export const stageHints: Record<Stage, string> = {
  preparing: "Getting the backup folder ready.", startingCloud: "The cloud app has to run for its folder to be there.",
  closingApp: "So no file is half-written while it is zipped.", scanning: "Listing the files and leaving out build and cache folders.",
  zipping: "Packing the files into a zip on this PC first.", verifying: "Reading the zip back to be sure every file is whole.",
  checkingCompleteness: "Comparing the zip with the project folder.", copying: "Copying the checked zip into the cloud folder.",
  verifyingCopy: "Reading the copy back from the cloud folder.", finishing: "Naming the backup and saving its details.",
};
/** The order of the stages, for "step 3 of 8" (starting and closing the app happen sometimes). */
const stageOrder: Stage[] = ["preparing", "scanning", "zipping", "verifying", "checkingCompleteness", "copying", "verifyingCopy", "finishing"];
export function stageStep(stage: Stage): { step: number; of: number } | null {
  const index = stageOrder.indexOf(stage);
  return index < 0 ? null : { step: index + 1, of: stageOrder.length };
}
export const providerNames: Record<BackupProvider, string> = { googleDrive: "Google Drive", dropbox: "Dropbox" };
interface OperationView {
  startedAt: number; index: number; total: number; projectName: string; stage: Stage;
  doneBytes: number; totalBytes: number; doneFiles: number; totalFiles: number; note: string | null;
}
export interface EditorView {
  /** null: a new project; `fixMissing`: the folder was not found, save then back up. */
  project: Project | null;
  fixMissing: boolean;
}
export interface CompareView { projectId: string; firstId: string; secondId: string }
/** How a project is doing, for the card's badge and for the groups. */
export type Health = "missing" | "failed" | "never" | "stale" | "due" | "ok" | "cancelled" | "unknown" | "backedUp";
/** What the page summary counts. */
export interface Summary {
  projects: number; ok: number; attention: number; never: number; stale: number; missing: number;
  /** Everything the kept backups hold, when the sizes are known. */
  backupBytes: number; knownSizes: number; newest: number | null; oldest: number | null;
}
/** A group of projects under one heading of the list. */
export interface Group { key: "attention" | "never" | "stale" | "ok" | "missing" | "unknown"; title: string; note: string; projects: Project[] }
const message = (error: unknown): string => (error instanceof Error ? error.message : String(error));
const emptyPage: PageState = {
  settings: { version: 2, provider: null, cloudFolder: null, projects: [], exclusions: [], smartBuild: true, followGitignore: false, imported: true },
  clouds: [], backupRoot: null, running: false, defaultExclusions: [], missingSources: [], imported: null,
};
export function samePath(a: string, b: string) {
  const normal = (path: string) => path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  return normal(a) === normal(b);
}
class ProjectBackupsState {
  page = $state<PageState>(emptyPage);
  loading = $state(true);
  error = $state<string | null>(null);
  operation = $state<OperationView | null>(null);
  cancelling = $state(false);
  /** Problems of the last run, shown until dismissed. */
  failures = $state<ProjectResult[]>([]);
  busy = $state<string | null>(null);
  exclusionsOpen = $state(false);
  editor = $state<EditorView | null>(null);
  compare = $state<CompareView | null>(null);
  /** The project whose backups are listed, and its list. */
  expanded = $state<string | null>(null);
  backups = $state<Record<string, BackupEntry[]>>({});
  /** A failed listing is unknown, not a successful empty history. */
  backupErrors = $state<Record<string, string>>({});
  backupsLoading = $state<string | null>(null);
  /** Backups ticked for a comparison (at most two). */
  picked = $state<string[]>([]);
  /** Filters the project list (name, backup name or folder); `filter` picks a group. */
  query = $state("");
  filter = $state<"all" | Group["key"]>("all");
  /** Ticks every 30 s so "5 minutes ago" stays true. */
  clock = $state(Date.now());
  /* Each project's folder against its newest backup; `checking`: the projects being checked now (once per check asked for). */
  changes = $state<Record<string, ProjectChanges>>({});
  checking = $state<string[]>([]);
  /** Local to this PC; opening MYLE again keeps the same three-day deadline. */
  lastChangeCheck = $state(readJson<number | null>(LAST_CHANGE_CHECK_KEY, null, (value) =>
    typeof value === "number" && Number.isFinite(value) && value > 0,
  ));
  #scheduledChecking = false;
  #scheduledTimer: ReturnType<typeof setInterval> | null = null;
  #detecting = false;
  /** The count the last toast was about (null: none shown yet). */
  #announced: number | null = null;
  private loaded = $state(false);
  #pageSequence = 0;
  #settingsRevision = 0;
  #backupSequence = 0;
  #backupReads = new Map<string, number>();
  /* The projects the answers are about: before the page state is read (at the app's start), every project an answer came for. */
  private get knownIds(): string[] {
    return this.loaded ? this.projects.map((p) => p.id) : Object.keys(this.changes);
  }
  /* Projects with changes not in a backup yet (or no backup at all). Out of reach or not checked is not a change. */
  get changedIds(): string[] {
    return this.knownIds.filter((id) => ["changed", "noBackup"].includes(this.changes[id]?.state ?? ""));
  }
  /** Projects whose folder changed since their newest backup. */
  get changedCount(): number {
    return this.knownIds.filter((id) => this.changes[id]?.state === "changed").length;
  }
  /* Takes answers in: one started earlier than the answer already shown (a check that was still running during a backup) never replaces it. */
  applyChanges(results: ProjectChanges[], announce = true) {
    for (const result of results) {
      const known = this.changes[result.projectId];
      if (!known || known.checkedAt <= result.checkedAt) this.changes[result.projectId] = result;
    }
    this.#updateBadge(announce);
  }
  /* Sidebar count of changed projects, and a toast when it goes up while another page is open. */
  #updateBadge(announce = true) {
    const changed = this.changedCount;
    badges.set("project-backups", changed);
    if (announce && this.#announced !== null && changed > this.#announced && nav.current !== "project-backups") {
      toast.info(`${changed} ${changed === 1 ? "project has" : "projects have"} changes not backed up yet.`, {
        label: "Review",
        run: () => nav.go("project-backups"),
      });
    }
    this.#announced = changed;
  }
  /* Every three days, compare all current projects afresh and remind about pending changes even when their count is unchanged. An overdue check runs on startup; no backups are created by this check. */
  async checkScheduledChanges(now = Date.now()) {
    if (!isTauri() || this.#scheduledChecking || this.locked || this.checking.length) return;
    if (this.lastChangeCheck !== null && this.lastChangeCheck <= now && now - this.lastChangeCheck < CHANGE_CHECK_EVERY) return;
    this.#scheduledChecking = true;
    let ids: string[] = [];
    try {
      // Detection starts without opening this page: use the saved list, including projects added since the page was last looked at.
      const page = await api.getState();
      if (page.running || this.locked || this.checking.length) return;
      ids = page.settings.projects.map((project) => project.id);
      if (!ids.length) return;
      if (!this.loaded) {
        this.page = page;
        this.loaded = true;
        this.loading = false;
      }
      this.checking = [...ids];
      const results = await api.changes(ids, true);
      this.applyChanges(results, false);
      // An incomplete reply is retried, rather than silently waiting three days.
      if (ids.some((id) => !results.some((result) => result.projectId === id))) return;
      this.lastChangeCheck = now;
      writeJson(LAST_CHANGE_CHECK_KEY, now);
      const pending = ids.filter((id) => ["changed", "noBackup"].includes(this.changes[id]?.state ?? "")).length;
      if (pending) {
        toast.info(`3-day check: ${pending} ${pending === 1 ? "project needs" : "projects need"} a backup.`, {
          label: "Review",
          run: () => nav.go("project-backups"),
        });
      }
      const failed = results.filter((result) => ["missing", "unreachable", "error"].includes(result.state) || result.unreadable > 0).length;
      if (failed) {
        toast.error(`3-day check: ${failed} ${failed === 1 ? "project could" : "projects could"} not be fully checked.`, {
          label: "Review",
          run: () => nav.go("project-backups"),
        });
      }
    } catch {
      // Native requests may be refused while access is being restored. Keep the deadline overdue and retry on the next timer tick.
    } finally {
      const done = [...ids];
      this.checking = this.checking.filter((id) => {
        const index = done.indexOf(id);
        if (index < 0) return true;
        done.splice(index, 1);
        return false;
      });
      this.#scheduledChecking = false;
    }
  }
  /* Checks `ids` (all projects by default) now. `fresh`: compare again even where the folder looks as it did. */
  async checkChanges(ids: string[] = this.projects.map((p) => p.id), fresh = false) {
    if (!isTauri() || !ids.length) return;
    this.checking = [...this.checking, ...ids];
    try {
      this.applyChanges(await api.changes(ids, fresh));
    } catch {
      // Quietly: detection checks again by itself.
    } finally {
      const done = [...ids];
      this.checking = this.checking.filter((id) => {
        const index = done.indexOf(id);
        if (index < 0) return true;
        done.splice(index, 1);
        return false;
      });
    }
  }
  /* The app's background detection (started once, for the owner, whichever page is open): it checks a project after its folder changes and every project every few minutes; its answers arrive here. */
  async startDetection() {
    if (!isTauri() || this.#detecting) return;
    this.#detecting = true;
    // The reminder retries independently if the native watcher or the owner-access check is temporarily unavailable during startup.
    this.#scheduledTimer ??= setInterval(() => void this.checkScheduledChanges(), 60_000);
    void this.checkScheduledChanges();
    let stop: (() => void) | undefined;
    try {
      stop = await listen<ProjectChanges[]>(CHANGES_EVENT, (event) => this.applyChanges(event.payload));
      await api.watch();
      this.applyChanges(await api.latestChanges());
    } catch {
      stop?.();
      this.#detecting = false;
      // The page checks by itself when opened.
    }
  }
  /* Opening the page: detection runs, and projects it has no answer for yet (it just started) are checked now. */
  async showChanges() {
    await this.startDetection();
    const unknown = this.projects.filter((p) => !this.changes[p.id]).map((p) => p.id);
    if (unknown.length) await this.checkChanges(unknown);
  }
  /** After the project list changed: the folders are watched again. */
  async startWatching() {
    if (!isTauri()) return;
    if (!this.#detecting) return this.startDetection();
    void api.watch().catch(() => {});
  }
  get locked() { return this.operation !== null || this.busy !== null }
  get projects() { return this.page.settings.projects }
  get provider() { return this.page.settings.provider }
  get cloud(): CloudChoice | null {
    const folder = this.page.settings.cloudFolder;
    if (!folder) return null;
    return this.page.clouds.find((cloud) => samePath(cloud.path, folder)) ?? null;
  }
  /** The projects that match the search box. */
  get filtered() {
    const query = this.query.trim().toLowerCase();
    if (!query) return this.projects;
    return this.projects.filter((project) => [project.name, project.appName, project.sourcePath].some((text) => text.toLowerCase().includes(query)));
  }
  /** "5 minutes ago", kept fresh by `clock`; days since the last good backup. */
  ago(value: number) {
    void this.clock;
    return formatRelative(value);
  }
  daysSinceBackup(project: Project) {
    if (!project.lastBackup) return null;
    return Math.floor((this.clock - project.lastBackup.createdAt) / 86_400_000);
  }
  isMissing = (project: Project) => this.page.missingSources.includes(project.id);
  /** The last attempt that ended badly, for the card's badge. */
  lastFailure(project: Project) {
    const result = project.lastResult;
    if (!result?.at || result.ok) return null;
    return result;
  }
  /** Where the project stands, in one word. */
  health(project: Project): Health {
    if (this.isMissing(project)) return "missing";
    const failure = this.lastFailure(project);
    if (failure) return failure.cancelled ? "cancelled" : "failed";
    if (!project.lastBackup) {
      const checked = this.changes[project.id];
      if (checked?.backupId && (checked.state === "upToDate" || checked.state === "changed")) return "backedUp";
      // Imported configuration has no local history. Only a completed discovery can establish that the backup folder really is empty.
      return checked?.state === "noBackup" ? "never" : "unknown";
    }
    const days = this.daysSinceBackup(project) ?? 0;
    return days >= STALE_DAYS ? "stale" : days >= FRESH_DAYS ? "due" : "ok";
  }
  /** True when the project wants the user to do something. */
  needsAttention(project: Project) {
    const health = this.health(project);
    return health === "backedUp" ? this.changes[project.id]?.state !== "upToDate" : health !== "ok" && health !== "due";
  }
  /** Everything the page says about itself, in one pass. */
  get summary(): Summary {
    void this.clock;
    const summary: Summary = { projects: this.projects.length, ok: 0, attention: 0, never: 0, stale: 0, missing: 0, backupBytes: 0, knownSizes: 0, newest: null, oldest: null };
    for (const project of this.projects) {
      const health = this.health(project);
      if (health === "ok" || health === "due" || (health === "backedUp" && this.changes[project.id]?.state === "upToDate")) summary.ok++;
      if (health === "never") summary.never++;
      if (health === "stale") summary.stale++;
      if (health === "missing") summary.missing++;
      if (this.needsAttention(project)) summary.attention++;
      const backup = project.lastBackup;
      if (!backup) continue;
      summary.backupBytes += backup.zipSize;
      summary.knownSizes++;
      summary.newest = summary.newest === null ? backup.createdAt : Math.max(summary.newest, backup.createdAt);
      summary.oldest = summary.oldest === null ? backup.createdAt : Math.min(summary.oldest, backup.createdAt);
    }
    return summary;
  }
  /** The groups, in the order the user should read them; `filter` narrows to one. */
  get groups(): Group[] {
    const all = this.allGroups;
    return this.filter === "all" ? all : all.filter((group) => group.key === this.filter);
  }
  /** Every group, whatever the current filter is. Counts come from here. */
  get allGroups(): Group[] {
    const of = (...keys: Health[]) => this.filtered.filter((project) => keys.includes(this.health(project)));
    const groups: Group[] = [
      { key: "attention", title: "Did not finish", note: "The last backup failed; the reason and the fix are below.", projects: of("failed") },
      { key: "unknown", title: "Backup status unknown", note: "Check the backups folder to confirm the existing history.", projects: of("unknown") },
      // A cancelled attempt that never produced a backup still has none.
      { key: "never", title: "Not backed up yet", note: "Add them to the cloud with one click.", projects: of("never", "cancelled") },
      { key: "stale", title: "Getting old", note: `The last backup is ${STALE_DAYS} days old or more.`, projects: of("stale") },
      { key: "ok", title: "Up to date", note: "Backed up recently and checked.", projects: of("ok", "due", "backedUp") },
      { key: "missing", title: "Folder not found", note: "Point MYLE at the folder again, or remove the project.", projects: of("missing") },
    ];
    return groups.filter((group) => group.projects.length > 0);
  }
  setFilter(filter: "all" | Group["key"]) {
    this.filter = filter;
  }
  async init() {
    if (this.loaded && !this.loading) {
      void this.refresh(false);
      return;
    }
    await this.refresh(true);
  }
  async refresh(first = false) {
    const sequence = ++this.#pageSequence;
    const revision = this.#settingsRevision;
    try {
      const page = await api.getState();
      if (sequence !== this.#pageSequence || revision !== this.#settingsRevision) return;
      this.page = page;
      this.error = null;
      this.loaded = true;
      if (first && page.imported?.added.length) {
        const count = page.imported.added.length;
        toast.success(`Imported ${count} ${count === 1 ? "project" : "projects"} from Backup Projects.`);
      }
      if (this.expanded && !page.settings.projects.some((project) => project.id === this.expanded)) this.expanded = null;
    } catch (error) {
      if (sequence === this.#pageSequence && revision === this.#settingsRevision) this.error = message(error);
    } finally {
      if (sequence === this.#pageSequence) this.loading = false;
    }
  }
  private async run<T>(busy: string, work: () => Promise<T>): Promise<T | undefined> {
    if (this.locked) return undefined;
    this.busy = busy;
    this.#settingsRevision++;
    try {
      return await work();
    } catch (error) {
      toast.error(message(error));
      return undefined;
    } finally {
      this.#settingsRevision++;
      this.busy = null;
    }
  }
  async useCloud(provider: BackupProvider, path: string | null) {
    const settings = await this.run("provider", () => api.setProvider(provider, path));
    if (!settings) return;
    toast.success(`Backups go to ${providerNames[provider]}.`);
    await this.refresh();
  }
  async importProjects() {
    const result = await this.run("import", () => api.import());
    if (!result) return;
    const [, report] = result;
    if (report.added.length) toast.success(`Imported ${report.added.join(", ")}.`);
    else if (report.from) toast.info("Every project of Backup Projects is already here.");
    else toast.info("Backup Projects' project list was not found on this PC or in the backups folder.");
    await this.refresh();
    if (report.added.length) void this.startWatching();
  }
  openEditor(project: Project | null, fixMissing = false) {
    if (this.operation) return;
    this.editor = { project, fixMissing };
  }
  closeEditor() {
    this.editor = null;
  }
  /** Saves the project from the editor; after a missing folder was fixed the backup continues. */
  async saveProject(project: Project) {
    const fixMissing = this.editor?.fixMissing ?? false;
    const result = await this.run("save", () => api.saveProject(project, fixMissing));
    if (!result) return;
    const [, id] = result;
    this.editor = null;
    await this.refresh();
    if (fixMissing) {
      this.failures = this.failures.filter((failure) => failure.projectId !== id);
      await this.backup([id]);
    } else {
      toast.success(`${project.name.trim()} saved.`);
    }
    void this.startWatching();
  }
  async removeProject(project: Project) {
    const ok = await confirm({ title: `Remove ${project.name}?`, message: "MYLE stops backing up this project. Its backups stay in the cloud folder.", confirmLabel: "Remove", danger: true });
    if (!ok) return;
    const settings = await this.run("remove", () => api.removeProject(project.id));
    if (!settings) return;
    toast.success(`${project.name} removed.`);
    await this.refresh();
    void this.startWatching();
  }
  async setExclusions(patterns: string[], smartBuild: boolean, followGitignore: boolean) {
    const settings = await this.run("exclusions", () => api.setExclusions(patterns, smartBuild, followGitignore));
    if (!settings) return false;
    this.page = { ...this.page, settings };
    toast.success("Exclusions saved.");
    return true;
  }
  async pickFolder(title: string) {
    try {
      return await api.pickFolder(title);
    } catch (error) {
      toast.error(message(error));
      return null;
    }
  }
  /* Backs up `projectIds`; `incremental`: only what changed where an increment fits (the app makes a full backup otherwise). */
  async backup(projectIds: string[], incremental = false) {
    if (this.locked || !projectIds.length) return;
    if (!this.provider) {
      toast.info("Choose Google Drive or Dropbox first.");
      return;
    }
    this.cancelling = false;
    this.#settingsRevision++;
    this.failures = [];
    this.operation = { startedAt: Date.now(), index: 0, total: projectIds.length, projectName: "", stage: "preparing", doneBytes: 0, totalBytes: 0, doneFiles: 0, totalFiles: 0, note: null };
    try {
      const run = await api.backup(projectIds, (event) => this.onEvent(event), incremental);
      this.page = { ...this.page, settings: run.settings };
      const done = run.results.filter((result) => result.ok);
      const failed = run.results.filter((result) => !result.ok && result.code !== "CANCELLED");
      this.failures = failed;
      if (run.cancelled) toast.info("Backup cancelled.");
      // One toast per project for a few; a long run gets one line, not a queue of toasts that takes minutes to go by.
      if (done.length > 3) {
        const files = done.reduce((sum, result) => sum + (result.outcome?.fileCount ?? 0), 0);
        const late = done.filter((result) => !result.outcome?.finalCheckOk).length;
        toast.success(`${done.length} projects backed up (${files.toLocaleString()} files).${late ? ` The cloud app is still catching up with ${late} of them.` : ""}`, { label: "Show", run: () => void api.open("root") });
      } else {
        for (const result of done) {
          const outcome = result.outcome!;
          const late = outcome.finalCheckOk ? "" : " The cloud app is still catching up with it.";
          if (outcome.kind === "incremental") {
            const gone = outcome.deletedCount ? `, ${outcome.deletedCount.toLocaleString()} deleted` : "";
            toast.success(`${result.name}: only the changes were saved (${outcome.fileCount.toLocaleString()} files${gone}, on ${outcome.base ?? "the last full backup"}).${late}`, {
              label: "Show", run: () => void api.open("backups", result.projectId),
            });
            continue;
          }
          toast.success(`${result.name}: ${outcome.name}.zip saved (${outcome.fileCount.toLocaleString()} files).${late}`, {
            label: "Show", run: () => void api.open("backups", result.projectId, `${outcome.monthFolder}/${outcome.name}.zip`),
          });
        }
      }
      const missing = failed.find((result) => result.code === "SOURCE_MISSING");
      if (missing && failed.length === 1 && done.length === 0) {
        const project = this.projects.find((item) => item.id === missing.projectId);
        if (project) this.openEditor(project, true);
      }
      if (this.expanded) void this.loadBackups(this.expanded);
    } catch (error) {
      toast.error(message(error));
    } finally {
      this.#settingsRevision++;
      this.operation = null;
      this.cancelling = false;
      void this.refresh();
      // What was just backed up is shown up to date right away (writes made meanwhile, elsewhere too, are checked by the app after the backup).
      void this.checkChanges(projectIds, true);
    }
  }
  private onEvent(event: ProjectBackupsEvent) {
    const operation = this.operation;
    if (!operation) return;
    switch (event.event) {
      case "project":
        operation.index = event.data.index;
        operation.total = event.data.total;
        operation.projectName = event.data.name;
        operation.stage = "preparing";
        operation.doneBytes = operation.totalBytes = operation.doneFiles = operation.totalFiles = 0;
        operation.note = null;
        break;
      case "stage":
        operation.stage = event.data.stage;
        operation.doneBytes = operation.totalBytes = operation.doneFiles = operation.totalFiles = 0;
        break;
      case "progress":
        operation.doneBytes = event.data.doneBytes;
        operation.totalBytes = event.data.totalBytes;
        operation.doneFiles = event.data.doneFiles;
        operation.totalFiles = event.data.totalFiles;
        break;
      case "message":
        operation.note = event.data.text;
        break;
    }
  }
  async cancel() {
    if (!this.operation || this.cancelling) return;
    this.cancelling = true;
    try {
      await api.cancel("backup");
    } catch (error) {
      this.cancelling = false;
      toast.error(`Could not cancel: ${message(error)}`);
    }
  }
  dismissFailures() {
    this.failures = [];
  }
  /** Backs up the projects of the failure list again. */
  async retryFailures(projectIds: string[] = this.failures.map((failure) => failure.projectId)) {
    await this.backup(projectIds);
  }
  /** Starts Google Drive or Dropbox for a backup that could not reach it. */
  async startCloud(provider: BackupProvider) {
    const text = await this.run("cloud", () => api.startCloud(provider));
    if (text) toast.info(`${text} Try the backup again once it is signed in.`);
    await this.refresh();
  }
  /* Puts the project as last backed up (its full backup and the increments on it) into an empty folder the user picks; every file is checked. */
  async restore(project: Project) {
    const folder = await this.pickFolder(`Restore ${project.name} into an empty folder`);
    if (!folder) return;
    const restored = await this.run("restore", () => api.restore(project.id, folder));
    if (!restored) return;
    const parts = restored.from.length > 1 ? ` from ${restored.from[0]} and ${restored.from.length - 1} ${restored.from.length === 2 ? "increment" : "increments"}` : ` from ${restored.from[0]}`;
    toast.success(`${project.name}: ${restored.files.toLocaleString()} files restored and checked${parts}.`);
  }
  async toggleBackups(projectId: string) {
    if (this.expanded === projectId) {
      this.expanded = null;
      return;
    }
    this.expanded = projectId;
    this.picked = [];
    await this.loadBackups(projectId);
  }
  async loadBackups(projectId: string) {
    const sequence = ++this.#backupSequence;
    this.#backupReads.set(projectId, sequence);
    const provider = this.provider;
    const folder = this.page.settings.cloudFolder;
    const current = () => this.#backupReads.get(projectId) === sequence && this.provider === provider && this.page.settings.cloudFolder === folder;
    this.backupsLoading = projectId;
    try {
      const list = await api.list(projectId);
      if (!current()) return;
      this.backups = { ...this.backups, [projectId]: list };
      delete this.backupErrors[projectId];
    } catch (error) {
      if (!current()) return;
      this.backupErrors[projectId] = message(error);
      toast.error(message(error));
    } finally {
      if (this.#backupReads.get(projectId) === sequence) this.#backupReads.delete(projectId);
      if (this.#backupSequence === sequence) this.backupsLoading = null;
    }
  }
  togglePick(id: string) {
    if (this.picked.includes(id)) this.picked = this.picked.filter((item) => item !== id);
    else this.picked = [...this.picked.slice(-1), id];
  }
  openCompare(projectId: string, firstId: string, secondId: string = SOURCE_ID) {
    if (this.operation) return;
    this.compare = { projectId, firstId, secondId };
  }
  closeCompare() {
    this.compare = null;
  }
  async open(target: "root" | "backups" | "source", projectId: string | null = null, backupId: string | null = null) {
    try {
      await api.open(target, projectId, backupId);
    } catch (error) {
      toast.error(message(error));
    }
  }
}
export const projectBackupsState = new ProjectBackupsState();
/** Patterns typed one per line. */
export function parseLines(text: string): string[] {
  return text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
}
