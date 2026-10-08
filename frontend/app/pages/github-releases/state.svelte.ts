// The GitHub Releases page's state: the projects, their status, and the
// builds and releases running on them (kept while the page is left, so a
// build goes on and its log is still there on the way back).
import { SvelteMap } from "svelte/reactivity";
import { badges } from "../../../lib/badges.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import {
  githubReleasesApi as api,
  messageOf,
  problemOf,
  type Artifact,
  type BuildEvent,
  type BuildKind,
  type BuildOutcome,
  type Diagnostic,
  type EntryStatus,
  type FoundRepo,
  type GitEvent,
  type LogLine,
  type Mode,
  type PageState,
  type Problem,
  type ProviderId,
  type ReleaseEvent,
  type ReleaseOutcome,
  type RemoteInfo,
  type RepoBrief,
  type RepoStatus,
  type Run,
  type StepId,
  type StepState,
  type WorkflowJob,
} from "./api";

export { formatBytes, formatDate, formatRelative } from "../game-saves/state.svelte";

export type Tab = "changes" | "build" | "release" | "history";

const SELECTED_KEY = "myle.githubReleases.selected";
const TAB_KEY = "myle.githubReleases.tab";
const tabs: Tab[] = ["changes", "build", "release", "history"];

/** Lines a build log keeps on screen; the whole log is in its file. */
export const LOG_LIMIT = 20_000;

export const buildKindLabels: Record<BuildKind, string> = {
  tauri: "Tauri",
  electron: "Electron",
  vite: "Vite",
  node: "Node",
  rust: "Rust",
  dotnet: ".NET",
  python: "Python",
  go: "Go",
  flutter: "Flutter",
  extension: "Extension",
};

/** A project in the list: an app of a repository, or the repository itself
 *  while its status loads (or when it can't be read). */
export interface ListItem {
  id: string;
  repo: RepoBrief;
  entry: EntryStatus | null;
  status: RepoStatus | null;
}

/** A build of one project, with its log, problems and the files it made. */
export class BuildSession {
  running = $state(false);
  install = $state(false);
  command = $state("");
  logPath = $state<string | null>(null);
  lines = $state.raw<LogLine[]>([]);
  /** Lines dropped from the top to keep the log at LOG_LIMIT. */
  dropped = $state(0);
  diagnostics = $state.raw<Diagnostic[]>([]);
  stage = $state<string | null>(null);
  percent = $state(0);
  estimated = $state(true);
  outcome = $state<BuildOutcome | null>(null);
  artifacts = $state.raw<Artifact[]>([]);
  selected = $state<string[]>([]);
  startedAt = $state(0);

  reset(command: string, install: boolean) {
    this.running = true;
    this.install = install;
    this.command = command;
    this.logPath = null;
    this.lines = [];
    this.dropped = 0;
    this.diagnostics = [];
    this.stage = null;
    this.percent = 0;
    this.estimated = true;
    this.outcome = null;
    if (!install) {
      this.artifacts = [];
      this.selected = [];
    }
    this.startedAt = Date.now();
  }

  apply(event: BuildEvent) {
    switch (event.event) {
      case "started":
        this.logPath = event.data.logPath;
        break;
      case "lines": {
        const next = this.lines.slice();
        for (const line of event.data.lines) {
          if (line.replace && next.length) next[next.length - 1] = line;
          else next.push(line);
        }
        const extra = next.length - LOG_LIMIT;
        if (extra > 0) {
          next.splice(0, extra);
          this.dropped += extra;
        }
        this.lines = next;
        break;
      }
      case "diagnostic":
        this.diagnostics = [...this.diagnostics, event.data.diagnostic];
        break;
      case "stage":
        this.stage = event.data.text;
        break;
      case "progress":
        this.percent = event.data.percent;
        this.estimated = event.data.estimated;
        break;
    }
  }

  finish(outcome: BuildOutcome) {
    this.running = false;
    this.outcome = outcome;
    // The full list (the live one can miss problems found at the end).
    if (outcome.diagnostics.length >= this.diagnostics.length) this.diagnostics = outcome.diagnostics;
    if (outcome.ok) this.percent = 100;
  }
}

export const stepLabels: Record<StepId, string> = {
  check: "Checks",
  version: "Version files",
  build: "Build",
  commit: "Commit",
  push: "Push",
  tag: "Tag",
  release: "Draft release",
  upload: "Upload files",
  publish: "Publish",
  workflow: "GitHub Actions",
};

export const localSteps: StepId[] = ["check", "version", "build", "commit", "push", "tag", "release", "upload", "publish"];
export const actionsSteps: StepId[] = ["check", "version", "commit", "push", "tag", "workflow"];

export interface Upload {
  name: string;
  sent: number;
  total: number;
  index: number;
  count: number;
}

/** A release of one project: what the form says and how the run goes. */
export class ReleaseSession {
  // The form.
  version = $state("");
  title = $state("");
  notes = $state("");
  mode = $state<Mode>("local");
  build = $state(true);
  includeChanges = $state(false);
  commitMessage = $state("");
  draft = $state(false);
  prerelease = $state(false);
  makeLatest = $state(true);
  /** Write the notes to the file the workflow reads (Actions mode). */
  notesFile = $state<string | null>(null);
  /** Paths of the files to attach; null: the build's own choice. */
  assets = $state<string[] | null>(null);
  touched = $state(false);

  // The run.
  running = $state(false);
  steps = $state<Partial<Record<StepId, { state: StepState; message: string | null }>>>({});
  upload = $state<Upload | null>(null);
  run = $state<Run | null>(null);
  jobs = $state.raw<WorkflowJob[]>([]);
  log = $state.raw<string[]>([]);
  outcome = $state<ReleaseOutcome | null>(null);
  buildLog = new BuildSession();

  start() {
    this.running = true;
    this.steps = {};
    this.upload = null;
    this.run = null;
    this.jobs = [];
    this.log = [];
    this.outcome = null;
  }

  apply(event: ReleaseEvent) {
    switch (event.event) {
      case "step":
        this.steps[event.data.id] = { state: event.data.state, message: event.data.message };
        if (event.data.id === "build" && event.data.state === "running") this.buildLog.reset("", false);
        break;
      case "build":
        this.buildLog.apply(event.data.event);
        break;
      case "upload":
        this.upload = event.data;
        break;
      case "workflow":
        this.run = event.data.run;
        this.jobs = event.data.jobs;
        break;
      case "log":
        this.log = [...this.log.slice(-300), event.data.text];
        break;
    }
  }
}

/** Output of a push or pull, shown under the commit box. */
export interface GitOutput {
  action: string;
  lines: string[];
  running: boolean;
}

function isTab(value: unknown): boolean {
  return typeof value === "string" && (tabs as string[]).includes(value);
}

/** Runs async work a few at a time. */
async function each<T>(items: T[], limit: number, run: (item: T) => Promise<void>) {
  const queue = items.slice();
  const workers = Array.from({ length: Math.min(limit, queue.length) }, async () => {
    while (queue.length) await run(queue.shift()!);
  });
  await Promise.all(workers);
}

class GithubReleasesState {
  page = $state<PageState | null>(null);
  loading = $state(true);
  error = $state<string | null>(null);
  statuses = $state<Record<string, RepoStatus>>({});
  remotes = $state<Record<string, RemoteInfo>>({});
  /** Repositories whose status is being read. */
  refreshing = $state<Record<string, boolean>>({});
  /** What a repository is busy with (a git command), by repo id. */
  busy = $state<Record<string, string>>({});
  gitOutput = $state<Record<string, GitOutput>>({});
  selectedId = $state<string | null>(readJson<string | null>(SELECTED_KEY, null, (v) => typeof v === "string"));
  tab = $state<Tab>(readJson<Tab>(TAB_KEY, "changes", isTab));
  query = $state("");
  settingsOpen = $state<null | "account" | "ai">(null);
  found = $state<FoundRepo[] | null>(null);
  scanning = $state(false);
  /** The list on a narrow page: shown instead of the project. */
  showList = $state(true);
  clock = $state(Date.now());
  /** Commit messages being written, by repository (not reactive). */
  drafts: Record<string, string> = {};

  builds = new SvelteMap<string, BuildSession>();
  releases = new SvelteMap<string, ReleaseSession>();

  #started = false;

  get repos(): RepoBrief[] {
    return this.page?.repos ?? [];
  }

  /** Every project, in the order of the repositories. */
  items = $derived.by<ListItem[]>(() => {
    const out: ListItem[] = [];
    for (const repo of this.repos) {
      const status = this.statuses[repo.id] ?? null;
      if (status?.entries.length) {
        for (const entry of status.entries) out.push({ id: entry.id, repo, entry, status });
      } else {
        out.push({ id: repo.id, repo, entry: null, status });
      }
    }
    return out;
  });

  visible = $derived.by<ListItem[]>(() => {
    const words = this.query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return this.items;
    return this.items.filter((item) => {
      const text = [item.repo.name, item.repo.path, item.entry?.name, item.entry?.sub, item.status?.remote?.owner, item.status?.remote?.repo]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      return words.every((word) => text.includes(word));
    });
  });

  selected = $derived.by<ListItem | null>(() => {
    const id = this.selectedId;
    // Nothing chosen yet (or it was removed): the first project, so the
    // panel never opens empty.
    if (!id) return this.items[0] ?? null;
    return (
      this.items.find((item) => item.id === id) ??
      // The repository's apps appeared after it was chosen: its first app.
      this.items.find((item) => item.repo.id === id.split("/")[0]) ??
      this.items[0] ??
      null
    );
  });

  async init() {
    if (this.#started) {
      void this.refreshSelected(false);
      return;
    }
    this.#started = true;
    this.loading = true;
    try {
      this.page = await api.getState();
      this.error = null;
    } catch (error) {
      this.error = messageOf(error);
      this.#started = false;
      return;
    } finally {
      this.loading = false;
    }
    const selectedRepo = this.selected?.repo.id;
    // The chosen project first, the rest a few at a time.
    if (selectedRepo) await this.refresh(selectedRepo);
    await each(
      this.repos.filter((repo) => repo.id !== selectedRepo),
      4,
      (repo) => this.refresh(repo.id),
    );
    if (selectedRepo) void this.fetchQuietly(selectedRepo);
  }

  setPage(page: PageState) {
    this.page = page;
  }

  select(id: string) {
    this.selectedId = id;
    this.showList = false;
    writeJson(SELECTED_KEY, id);
    const repoId = id.split("/")[0];
    if (!this.remotes[repoId]) void this.loadRemote(repoId);
  }

  setTab(tab: Tab) {
    this.tab = tab;
    writeJson(TAB_KEY, tab);
  }

  build(entryId: string): BuildSession {
    let session = this.builds.get(entryId);
    if (!session) {
      session = new BuildSession();
      this.builds.set(entryId, session);
    }
    return session;
  }

  release(entryId: string): ReleaseSession {
    let session = this.releases.get(entryId);
    if (!session) {
      session = new ReleaseSession();
      this.releases.set(entryId, session);
    }
    return session;
  }

  /** Whether anything runs on a repository (git, a build, a release). */
  running(repoId: string): string | null {
    if (this.busy[repoId]) return this.busy[repoId];
    for (const [id, session] of this.builds) if (session.running && id.split("/")[0] === repoId) return "build";
    for (const [id, session] of this.releases) if (session.running && id.split("/")[0] === repoId) return "release";
    return null;
  }

  async refresh(repoId: string) {
    this.refreshing[repoId] = true;
    try {
      this.statuses[repoId] = await api.status(repoId);
    } catch (error) {
      const repo = this.repos.find((r) => r.id === repoId);
      if (repo) {
        this.statuses[repoId] = {
          repoId,
          path: repo.path,
          exists: repo.exists,
          isRepo: false,
          remote: null,
          branch: { branch: null, head: null, upstream: null, ahead: 0, behind: 0 },
          changes: 0,
          entries: [],
          workflows: [],
          problem: messageOf(error),
        };
      }
    } finally {
      delete this.refreshing[repoId];
      this.updateBadge();
    }
  }

  async refreshSelected(withRemote: boolean) {
    const repoId = this.selected?.repo.id;
    if (!repoId) return;
    await this.refresh(repoId);
    if (withRemote) await this.loadRemote(repoId);
  }

  async loadRemote(repoId: string) {
    try {
      this.remotes[repoId] = await api.remote(repoId);
    } catch (error) {
      this.remotes[repoId] = { releases: [], lastRelease: {}, run: null, problem: problemOf(error) };
    }
  }

  /** Brings what GitHub has (ahead/behind), without bothering the user. */
  async fetchQuietly(repoId: string) {
    if (this.running(repoId)) return;
    try {
      this.statuses[repoId] = await api.fetch(repoId, true);
    } catch {
      // Offline, or signed out: the local status stays.
    }
    void this.loadRemote(repoId);
  }

  async fetch(repoId: string) {
    if (this.running(repoId)) return;
    this.busy[repoId] = "fetch";
    try {
      this.statuses[repoId] = await api.fetch(repoId, false);
      await this.loadRemote(repoId);
    } catch (error) {
      this.showProblem(problemOf(error));
    } finally {
      delete this.busy[repoId];
    }
  }

  updateBadge() {
    // The sidebar shows how many projects can be pulled.
    const behind = Object.values(this.statuses).filter((s) => s.branch.behind > 0).length;
    badges.set("github-releases", behind);
  }

  async addFolder() {
    try {
      const result = await api.addFolder();
      if (!result) return;
      this.page = result.state;
      if (result.added) {
        await this.refresh(result.added);
        this.select(this.items.find((item) => item.repo.id === result.added)?.id ?? result.added);
        void this.fetchQuietly(result.added);
      } else if (result.found.length) {
        this.found = result.found;
      }
    } catch (error) {
      this.showProblem(problemOf(error));
    }
  }

  async scanFolder() {
    this.scanning = true;
    try {
      const found = await api.scanFolder();
      if (found === null) return;
      if (!found.length) toast.info("No git repositories were found in that folder.");
      else this.found = found;
    } catch (error) {
      toast.error(messageOf(error));
    } finally {
      this.scanning = false;
    }
  }

  async addRepos(paths: string[]) {
    const before = new Set(this.repos.map((r) => r.id));
    this.page = await api.addRepos(paths);
    const added = this.repos.filter((r) => !before.has(r.id));
    await each(added, 4, (repo) => this.refresh(repo.id));
    if (added.length) {
      toast.success(added.length === 1 ? `Added ${added[0].name}.` : `Added ${added.length} repositories.`);
      if (!this.selected) this.select(this.items.find((i) => i.repo.id === added[0].id)?.id ?? added[0].id);
    }
  }

  async removeRepo(repoId: string) {
    this.page = await api.removeRepo(repoId);
    delete this.statuses[repoId];
    delete this.remotes[repoId];
    if (this.selected?.repo.id === repoId || this.selectedId?.split("/")[0] === repoId) {
      this.selectedId = null;
      this.showList = true;
    }
    this.updateBadge();
  }

  // ─── Git ───────────────────────────────────────────────────────────────

  #output(repoId: string, action: string): (event: GitEvent) => void {
    this.gitOutput[repoId] = { action, lines: [], running: true };
    return (event) => {
      const output = this.gitOutput[repoId];
      if (!output) return;
      const text = event.data.text;
      if (event.data.replace && output.lines.length) output.lines[output.lines.length - 1] = text;
      else output.lines.push(text);
      if (output.lines.length > 400) output.lines.splice(0, output.lines.length - 400);
    };
  }

  #outputDone(repoId: string) {
    const output = this.gitOutput[repoId];
    if (output) output.running = false;
  }

  async push(repoId: string): Promise<boolean> {
    if (this.running(repoId)) return false;
    this.busy[repoId] = "push";
    try {
      const outcome = await api.push(repoId, this.#output(repoId, "Push"));
      if (outcome.ok) {
        toast.success(outcome.setUpstream ? "Pushed. The branch is on GitHub now." : "Pushed to GitHub.");
        return true;
      }
      if (outcome.problem) this.showProblem(outcome.problem, repoId);
      return false;
    } catch (error) {
      this.showProblem(problemOf(error), repoId);
      return false;
    } finally {
      this.#outputDone(repoId);
      delete this.busy[repoId];
      await this.refresh(repoId);
      void this.loadRemote(repoId);
    }
  }

  async pull(repoId: string): Promise<boolean> {
    if (this.running(repoId)) return false;
    this.busy[repoId] = "pull";
    try {
      const outcome = await api.pull(repoId, this.#output(repoId, "Pull"));
      if (outcome.ok) {
        toast.success("Up to date with GitHub.");
        return true;
      }
      if (outcome.problem) this.showProblem(outcome.problem, repoId);
      return false;
    } catch (error) {
      this.showProblem(problemOf(error), repoId);
      return false;
    } finally {
      this.#outputDone(repoId);
      delete this.busy[repoId];
      await this.refresh(repoId);
    }
  }

  async commit(repoId: string, message: string, push: boolean): Promise<boolean> {
    if (this.running(repoId)) return false;
    this.busy[repoId] = push ? "commit-push" : "commit";
    try {
      const outcome = await api.commit(repoId, message, push, this.#output(repoId, push ? "Commit & Push" : "Commit"));
      if (outcome.problem) {
        this.showProblem(outcome.problem, repoId);
        return false;
      }
      if (outcome.push && !outcome.push.ok) {
        toast.info(`Committed ${outcome.commit?.slice(0, 7) ?? ""}, but the push didn't go through.`);
        if (outcome.push.problem) this.showProblem(outcome.push.problem, repoId);
        return true;
      }
      toast.success(push ? "Committed and pushed." : `Committed ${outcome.commit?.slice(0, 7) ?? ""}.`);
      return true;
    } catch (error) {
      this.showProblem(problemOf(error), repoId);
      return false;
    } finally {
      this.#outputDone(repoId);
      delete this.busy[repoId];
      await this.refresh(repoId);
      if (push) void this.loadRemote(repoId);
    }
  }

  /** A git or GitHub problem as a toast, with what fixes it. */
  showProblem(problem: Problem, repoId?: string) {
    const action = (() => {
      switch (problem.code) {
        case "AUTH":
        case "WORKFLOW_SCOPE":
          return { label: "GitHub account", run: () => (this.settingsOpen = "account") };
        case "NO_AI":
        case "BAD_KEY":
          return { label: "AI settings", run: () => (this.settingsOpen = "ai") };
        case "REJECTED":
          return repoId ? { label: "Pull now", run: () => void this.pull(repoId) } : undefined;
        case "NO_GIT":
          return { label: "Install Git", run: () => void installTool("git") };
        default:
          return undefined;
      }
    })();
    toast.error(problem.message, action);
  }

  // ─── AI ────────────────────────────────────────────────────────────────

  /** Runs an AI request; when a provider fails and another is set up, the
   *  error offers it (nothing is sent to another provider unasked). */
  async ai<T>(options: {
    run: (provider: ProviderId | null) => Promise<T>;
    apply: (value: T) => void;
    setBusy: (busy: boolean) => void;
    provider?: ProviderId | null;
  }): Promise<void> {
    options.setBusy(true);
    try {
      options.apply(await options.run(options.provider ?? null));
    } catch (error) {
      const problem = problemOf(error);
      const failed = problem.data?.provider as ProviderId | undefined;
      if (problem.code === "RATE_LIMITED" && failed) {
        const seconds = typeof problem.data?.retryAfter === "number" ? problem.data.retryAfter : 60;
        this.aiLimits[failed] = Date.now() + seconds * 1000;
      }
      const next = problem.data?.next;
      if (next) {
        const name = problem.data?.nextName ?? next;
        toast.error(problem.message, { label: `Try ${name}`, run: () => void this.ai({ ...options, provider: next }) });
      } else {
        this.showProblem(problem);
      }
    } finally {
      options.setBusy(false);
    }
  }

  /** Providers that hit their limit, until when (ms). */
  aiLimits = $state<Partial<Record<ProviderId, number>>>({});

  get aiReady(): boolean {
    return !!this.page?.ai.providers.some((p) => p.ready);
  }
}

export async function installTool(tool: "gh" | "git") {
  try {
    await api.installTool(tool);
    toast.info(
      tool === "gh"
        ? "The GitHub CLI installs in the window that opened. Come back here when it's done."
        : "Git installs in the window that opened. Restart MYLE when it's done.",
    );
  } catch (error) {
    toast.error(messageOf(error));
  }
}

export const githubReleases = new GithubReleasesState();

/** "42s", "3m 5s". */
export function duration(ms: number): string {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

/** "main ↑2 ↓1" pieces. */
export function syncText(status: RepoStatus | null): string {
  if (!status) return "";
  const { ahead, behind, upstream } = status.branch;
  if (!upstream) return "Not on GitHub yet";
  if (!ahead && !behind) return "Up to date";
  return [ahead ? `${ahead} to push` : "", behind ? `${behind} to pull` : ""].filter(Boolean).join(", ");
}

/** A run's state: running, success, failure, cancelled… */
export function runState(run: { status: string | null; conclusion: string | null } | null): "running" | "ok" | "failed" | "cancelled" | "skipped" | null {
  if (!run) return null;
  if (run.status && run.status !== "completed") return "running";
  switch (run.conclusion) {
    case "success":
      return "ok";
    case "cancelled":
      return "cancelled";
    case "skipped":
    case "neutral":
      return "skipped";
    default:
      return "failed";
  }
}
