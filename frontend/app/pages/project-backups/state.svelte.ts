// Page state of Project Backups: the settings, a running backup and its
// progress, the open dialogs.
import { toast } from "../../../lib/toast.svelte";
import { confirm } from "../../../lib/confirm.svelte";
import {
  projectBackupsApi as api,
  SOURCE_ID,
  type BackupEntry,
  type BackupProvider,
  type CloudChoice,
  type PageState,
  type Project,
  type ProjectBackupsEvent,
  type ProjectResult,
  type Stage,
} from "./api";

export { formatBytes, formatDate, formatRelative } from "../game-saves/state.svelte";

export const stageLabels: Record<Stage, string> = {
  preparing: "Preparing",
  startingCloud: "Starting the cloud app",
  closingApp: "Closing the app",
  scanning: "Reading the project",
  zipping: "Zipping",
  verifying: "Checking the zip",
  checkingCompleteness: "Checking nothing is missing",
  copying: "Copying to the cloud folder",
  verifyingCopy: "Checking the copy",
  finishing: "Finishing",
};

export const providerNames: Record<BackupProvider, string> = {
  googleDrive: "Google Drive",
  dropbox: "Dropbox",
};

interface OperationView {
  startedAt: number;
  index: number;
  total: number;
  projectName: string;
  stage: Stage;
  doneBytes: number;
  totalBytes: number;
  note: string | null;
}

export interface EditorView {
  /** null: a new project. */
  project: Project | null;
  /** Opened because the folder was not found: save, then back up. */
  fixMissing: boolean;
}

export interface CompareView {
  projectId: string;
  firstId: string;
  secondId: string;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const emptyPage: PageState = {
  settings: {
    version: 1,
    provider: null,
    cloudFolder: null,
    projects: [],
    exclusions: [],
    smartBuild: true,
    followGitignore: false,
    imported: true,
  },
  clouds: [],
  backupRoot: null,
  running: false,
  defaultExclusions: [],
  missingSources: [],
  imported: null,
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
  backupsLoading = $state<string | null>(null);
  /** Backups ticked for a comparison (at most two). */
  picked = $state<string[]>([]);
  private loaded = false;

  get locked() {
    return this.operation !== null || this.busy !== null;
  }

  get projects() {
    return this.page.settings.projects;
  }

  get provider() {
    return this.page.settings.provider;
  }

  get cloud(): CloudChoice | null {
    const folder = this.page.settings.cloudFolder;
    if (!folder) return null;
    return this.page.clouds.find((cloud) => samePath(cloud.path, folder)) ?? null;
  }

  isMissing(project: Project) {
    return this.page.missingSources.includes(project.id);
  }

  async init() {
    if (this.loaded && !this.loading) {
      void this.refresh(false);
      return;
    }
    await this.refresh(true);
  }

  async refresh(first = false) {
    try {
      const page = await api.getState();
      this.page = page;
      this.error = null;
      this.loaded = true;
      if (first && page.imported?.added.length) {
        const count = page.imported.added.length;
        toast.success(`Imported ${count} ${count === 1 ? "project" : "projects"} from Backup Projects.`);
      }
      if (this.expanded && !page.settings.projects.some((project) => project.id === this.expanded)) {
        this.expanded = null;
      }
    } catch (error) {
      this.error = message(error);
    } finally {
      this.loading = false;
    }
  }

  private async run<T>(busy: string, work: () => Promise<T>): Promise<T | undefined> {
    if (this.locked) return undefined;
    this.busy = busy;
    try {
      return await work();
    } catch (error) {
      toast.error(message(error));
      return undefined;
    } finally {
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
    if (report.added.length) {
      toast.success(`Imported ${report.added.join(", ")}.`);
    } else if (report.from) {
      toast.info("Every project of Backup Projects is already here.");
    } else {
      toast.info("Backup Projects' project list was not found on this PC or in the backups folder.");
    }
    await this.refresh();
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
  }

  async removeProject(project: Project) {
    const ok = await confirm({
      title: `Remove ${project.name}?`,
      message: "MYLE stops backing up this project. Its backups stay in the cloud folder.",
      confirmLabel: "Remove",
      danger: true,
    });
    if (!ok) return;
    const settings = await this.run("remove", () => api.removeProject(project.id));
    if (!settings) return;
    toast.success(`${project.name} removed.`);
    await this.refresh();
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

  async backup(projectIds: string[]) {
    if (this.locked || !projectIds.length) return;
    if (!this.provider) {
      toast.info("Choose Google Drive or Dropbox first.");
      return;
    }
    this.cancelling = false;
    this.failures = [];
    this.operation = {
      startedAt: Date.now(),
      index: 0,
      total: projectIds.length,
      projectName: "",
      stage: "preparing",
      doneBytes: 0,
      totalBytes: 0,
      note: null,
    };
    try {
      const run = await api.backup(projectIds, (event) => this.onEvent(event));
      this.page = { ...this.page, settings: run.settings };
      const done = run.results.filter((result) => result.ok);
      const failed = run.results.filter((result) => !result.ok && result.code !== "CANCELLED");
      this.failures = failed;
      if (run.cancelled) toast.info("Backup cancelled.");
      for (const result of done) {
        const outcome = result.outcome!;
        const late = outcome.finalCheckOk ? "" : " The cloud app is still catching up with it.";
        toast.success(`${result.name}: ${outcome.name}.zip saved (${outcome.fileCount.toLocaleString()} files).${late}`, {
          label: "Show",
          run: () => void api.open("backups", result.projectId, `${outcome.monthFolder}/${outcome.name}.zip`),
        });
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
      this.operation = null;
      this.cancelling = false;
      void this.refresh();
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
        operation.doneBytes = 0;
        operation.totalBytes = 0;
        operation.note = null;
        break;
      case "stage":
        operation.stage = event.data.stage;
        operation.doneBytes = 0;
        operation.totalBytes = 0;
        break;
      case "progress":
        operation.doneBytes = event.data.doneBytes;
        operation.totalBytes = event.data.totalBytes;
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
      await api.cancel();
    } catch (error) {
      this.cancelling = false;
      toast.error(`Could not cancel: ${message(error)}`);
    }
  }

  dismissFailures() {
    this.failures = [];
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
    this.backupsLoading = projectId;
    try {
      const list = await api.list(projectId);
      this.backups = { ...this.backups, [projectId]: list };
    } catch (error) {
      toast.error(message(error));
    } finally {
      if (this.backupsLoading === projectId) this.backupsLoading = null;
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
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
}
