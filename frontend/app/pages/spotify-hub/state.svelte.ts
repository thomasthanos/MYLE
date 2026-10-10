import { isTauri } from "@tauri-apps/api/core";
import { compareVersions } from "../../../installer/versions";
import { confirm } from "../../../lib/confirm.svelte";
import { nav } from "../../../lib/nav.svelte";
import { operationGate } from "../../../lib/operation-gate.svelte";
import { toast } from "../../../lib/toast.svelte";
import { appsState } from "../install-apps/state.svelte";
import {
  spotifyHubApi,
  type PurgePreview,
  type SpotifyHubAction,
  type SpotifyHubEvent,
  type SpotifyHubJob,
  type SpotifyHubOutcome,
  type SpotifyHubSnapshot,
  type SpotifyHubStage,
  type SpotifyHubReleases,
} from "./api";

export type HubCardStatus = "Ready" | "Running" | "Completed" | "Partial" | "Error";

export interface ConsoleBuffer {
  lines: string[];
  open: boolean;
  dropped: number;
}

const MAX_LINES = 1200;
const TRIM_TO = 1000;
const POLL_MS = 1500;
/** A revisit within this window reuses the last detection. */
const FRESH_MS = 30_000;

const STAGE_LABELS: Record<SpotifyHubStage, string> = {
  preparing: "Preparing",
  downloading: "Downloading",
  verifying: "Verifying",
  installing: "Installing",
  restoring: "Restoring",
  uninstalling: "Uninstalling",
  cleaning: "Cleaning",
  finalizing: "Finalizing",
};

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

class SpotifyHubState {
  snapshot = $state<SpotifyHubSnapshot | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  releases = $state<SpotifyHubReleases | null>(null);
  checkingUpdates = $state(false);
  updateError = $state<string | null>(null);
  startingAction = $state<SpotifyHubAction | null>(null);
  liveJob = $state<SpotifyHubJob | null>(null);
  stopping = $state(false);
  purgePreview = $state<PurgePreview | null>(null);
  previewingPurge = $state(false);
  consoles = $state<Record<SpotifyHubAction, ConsoleBuffer>>({
    installSpicetify: { lines: [], open: false, dropped: 0 },
    restoreSpotify: { lines: [], open: false, dropped: 0 },
    purgeAll: { lines: [], open: false, dropped: 0 },
  });
  statuses = $state<Record<SpotifyHubAction, HubCardStatus>>({
    installSpicetify: "Ready",
    restoreSpotify: "Ready",
    purgeAll: "Ready",
  });

  #pollTimer: ReturnType<typeof setTimeout> | null = null;
  /** Last backend outcome already reflected in `statuses`. A failed run
   *  leaves the previous outcome in place, and re-applying it would turn
   *  the fresh "Error" back into "Completed". */
  #seenOutcome: string | null = null;
  #loadedAt = 0;
  #updateRequest = 0;

  readonly activeJob = $derived(this.liveJob ?? this.snapshot?.activeJob ?? null);
  readonly activeAction = $derived(this.startingAction ?? this.activeJob?.action ?? null);
  readonly ownBusy = $derived(this.activeAction !== null);
  readonly installAppsBusy = $derived(appsState.busy || operationGate.lockedFor("spotify-hub"));
  readonly locked = $derived(this.ownBusy || this.installAppsBusy);

  statusOf(action: SpotifyHubAction): HubCardStatus {
    return this.activeAction === action ? "Running" : this.statuses[action];
  }

  stageLabel(action: SpotifyHubAction): string | null {
    if (this.activeAction !== action) return null;
    return this.activeJob ? STAGE_LABELS[this.activeJob.stage] : "Preparing";
  }

  progressOf(action: SpotifyHubAction): number | null {
    return this.activeAction === action ? (this.activeJob?.progress ?? null) : null;
  }

  /** `force` after an action; a plain page visit reuses a fresh detection. */
  async load(force = false) {
    if (!isTauri() || this.loading) return;
    if (!force && this.snapshot && !this.error && Date.now() - this.#loadedAt < FRESH_MS) return;
    this.loading = true;
    try {
      const snapshot = await spotifyHubApi.getState();
      this.applySnapshot(snapshot);
      this.#loadedAt = Date.now();
      this.error = null;
    } catch (error) {
      this.error = message(error);
    } finally {
      this.loading = false;
    }
    if (this.snapshot && !this.error && !this.ownBusy) await this.checkUpdates();
  }

  async checkUpdates() {
    if (!isTauri() || this.locked || this.checkingUpdates || !this.snapshot?.spicetify.healthy) return;
    const request = ++this.#updateRequest;
    this.checkingUpdates = true;
    this.updateError = null;
    try {
      const releases = await spotifyHubApi.checkUpdates();
      if (request === this.#updateRequest) this.releases = releases;
    } catch (error) {
      if (request === this.#updateRequest) {
        this.releases = null;
        this.updateError = `Could not check releases: ${message(error)}`;
      }
    } finally {
      if (request === this.#updateRequest) this.checkingUpdates = false;
    }
  }

  private applySnapshot(snapshot: SpotifyHubSnapshot) {
    this.snapshot = snapshot;
    if (snapshot.activeJob) {
      this.liveJob = snapshot.activeJob;
      this.statuses[snapshot.activeJob.action] = "Running";
      this.consoles[snapshot.activeJob.action].open = true;
      operationGate.begin("spotify-hub");
      this.watchActiveJob();
    } else if (!this.startingAction) {
      this.liveJob = null;
      operationGate.end("spotify-hub");
      if (snapshot.lastOutcome) this.adoptOutcome(snapshot.lastOutcome);
    }
  }

  private adoptOutcome(outcome: SpotifyHubOutcome) {
    if (outcome.jobId === this.#seenOutcome) return;
    this.#seenOutcome = outcome.jobId;
    this.applyOutcomeStatus(outcome, outcome.action);
  }

  private watchActiveJob() {
    if (this.#pollTimer) return;
    const poll = async () => {
      this.#pollTimer = null;
      if (!this.activeJob || this.startingAction) return;
      try {
        const snapshot = await spotifyHubApi.getState();
        const previous = this.activeJob?.action ?? null;
        const previousJob = this.activeJob?.jobId ?? null;
        this.snapshot = snapshot;
        this.liveJob = snapshot.activeJob;
        if (snapshot.activeJob) {
          this.#pollTimer = setTimeout(poll, POLL_MS);
        } else {
          if (snapshot.lastOutcome?.jobId === previousJob) this.adoptOutcome(snapshot.lastOutcome);
          else if (previous) this.statuses[previous] = "Error";
          this.stopping = false;
          operationGate.end("spotify-hub");
        }
      } catch {
        this.#pollTimer = setTimeout(poll, POLL_MS);
      }
    };
    this.#pollTimer = setTimeout(poll, POLL_MS);
  }

  installLabel(): string {
    const state = this.snapshot;
    if (!state?.desktop.installed) return "Install Spotify first";
    if (!state.spicetify.installed) return "Install Spicetify";
    if (!state.spicetify.healthy || !state.spicetify.applied || !state.marketplace.installed || this.statuses.installSpicetify === "Error" || this.statuses.installSpicetify === "Partial") return "Repair";
    if (this.checkingUpdates) return "Checking updates…";
    if (!this.releases) return "Check for updates";
    const cli = state.spicetify.version;
    const marketplace = state.marketplace.version;
    if ((cli && compareVersions(cli, this.releases.cli) < 0)
      || (marketplace && compareVersions(marketplace, this.releases.marketplace) < 0)) return "Update";
    return cli && marketplace ? "Up to date" : "Repair";
  }

  prepareSpotifyInstall() {
    if (this.locked) return;
    appsState.prepareInstall("Spotify.Spotify");
    nav.rememberScroll("install-apps", 0);
    nav.go("install-apps");
  }

  async install() {
    if (this.checkingUpdates || this.locked) return;
    if (!this.snapshot?.desktop.installed) {
      this.prepareSpotifyInstall();
      return;
    }
    const label = this.installLabel();
    if (label === "Check for updates") {
      await this.checkUpdates();
      return;
    }
    if (label === "Up to date") return;
    await this.run("installSpicetify", null);
  }

  async repair() {
    if (this.snapshot?.desktop.installed) await this.run("installSpicetify", null, "Repair");
  }

  async restore() {
    if (this.locked) return;
    const ok = await confirm({
      title: "Restore stock Spotify?",
      message:
        "Spicetify will restore Spotify to its vanilla state first. Only after a successful restore will the CLI, Marketplace, config folders and this app's PATH entry be removed. Your Spotify account and cloud library are not affected.",
      confirmLabel: "Restore Spotify",
    });
    if (ok && !this.locked) await this.run("restoreSpotify", null);
  }

  async previewPurge() {
    if (this.locked || this.previewingPurge) return;
    this.previewingPurge = true;
    try {
      this.purgePreview = await spotifyHubApi.previewPurge();
    } catch (error) {
      toast.error(`Could not prepare the uninstall preview: ${message(error)}`);
    } finally {
      this.previewingPurge = false;
    }
  }

  dismissPurgePreview() {
    if (!this.startingAction) this.purgePreview = null;
  }

  async confirmPurge() {
    const preview = this.purgePreview;
    if (!preview || preview.expiresAt <= Date.now()) {
      this.purgePreview = null;
      toast.info("The uninstall preview expired. Review the current state again.");
      return;
    }
    this.purgePreview = null;
    await this.run("purgeAll", preview.token);
  }

  async cancel() {
    const job = this.activeJob;
    if (!job || this.stopping) return;
    if (job.action === "purgeAll") {
      const ok = await confirm({
        title: "Stop full uninstall?",
        message:
          "Stopping is best-effort between stages. Anything already removed cannot be restored, and the result will be marked Partial.",
        confirmLabel: "Stop uninstall",
        cancelLabel: "Keep running",
        danger: true,
      });
      if (!ok || !this.activeJob) return;
    }
    this.stopping = true;
    try {
      await spotifyHubApi.cancel(job.jobId);
    } catch (error) {
      this.stopping = false;
      toast.error(`Could not request cancellation: ${message(error)}`);
    }
  }

  toggleConsole(action: SpotifyHubAction) {
    this.consoles[action].open = !this.consoles[action].open;
  }

  private async run(action: SpotifyHubAction, purgeToken: string | null, requestedLabel?: string) {
    if (this.locked || !operationGate.begin("spotify-hub")) {
      toast.info("Finish the current app task before starting another action.");
      return;
    }

    const requested = requestedLabel ?? this.installLabelFor(action);
    this.startingAction = action;
    this.#updateRequest++;
    this.checkingUpdates = false;
    this.releases = null;
    this.statuses[action] = "Running";
    this.error = null;
    this.stopping = false;
    this.consoles[action] = { lines: [], open: true, dropped: 0 };
    this.append(action, `${requested} requested.`);

    try {
      const outcome = await spotifyHubApi.run(action, purgeToken, (event) => this.onEvent(action, event));
      this.#seenOutcome = outcome.jobId;
      this.applyOutcomeStatus(outcome, action);
      this.reportOutcome(action, outcome);
    } catch (error) {
      const text = message(error);
      this.statuses[action] = "Error";
      this.append(action, `Error: ${text}`);
      toast.error(`${this.titleFor(action)}: ${text}`);
    } finally {
      this.startingAction = null;
      this.liveJob = null;
      this.stopping = false;
      operationGate.end("spotify-hub");
      await this.load(true);
    }
  }

  private onEvent(action: SpotifyHubAction, event: SpotifyHubEvent) {
    if (event.event === "stage") {
      // The backend announces "preparing" twice (on start and inside the
      // action); one line per actual change is enough.
      const changed = this.liveJob?.jobId !== event.data.jobId || this.liveJob.stage !== event.data.stage;
      this.liveJob = { jobId: event.data.jobId, action, stage: event.data.stage, progress: changed ? null : (this.liveJob?.progress ?? null) };
      if (changed) this.append(action, `${STAGE_LABELS[event.data.stage]}…`);
    } else if (event.event === "progress") {
      if (this.liveJob?.jobId === event.data.jobId) this.liveJob.progress = Math.max(0, Math.min(1, event.data.fraction));
    } else {
      this.append(action, event.data.text, event.data.replace);
    }
  }

  private append(action: SpotifyHubAction, text: string, replace = false) {
    const buffer = this.consoles[action];
    const lines = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
    if (replace && buffer.lines.length && lines.length) {
      buffer.lines[buffer.lines.length - 1] = lines.shift()!;
    }
    buffer.lines.push(...lines);
    if (buffer.lines.length > MAX_LINES) {
      const remove = buffer.lines.length - TRIM_TO;
      buffer.lines.splice(0, remove);
      buffer.dropped += remove;
    }
  }

  private applyOutcomeStatus(outcome: SpotifyHubOutcome, action: SpotifyHubAction | null) {
    if (!action) return;
    if (outcome.result === "done") {
      this.statuses[action] = "Completed";
      if (action === "installSpicetify") this.consoles[action].open = false;
    }
    else if (outcome.result === "partial" || outcome.result === "cancelled") this.statuses[action] = "Partial";
    else this.statuses[action] = "Error";
  }

  private reportOutcome(action: SpotifyHubAction, outcome: SpotifyHubOutcome) {
    const title = this.titleFor(action);
    if (outcome.result === "done") toast.success(`${title} completed.${outcome.note ? ` ${outcome.note}` : ""}`);
    else if (outcome.result === "cancelled") toast.info(`${title} stopped before completion.`);
    else if (outcome.result === "partial") toast.info(`${title} completed only partially.${outcome.note ? ` ${outcome.note}` : ""}`);
    else toast.info(`${title} needs administrator approval to finish.${outcome.note ? ` ${outcome.note}` : ""}`);
  }

  private titleFor(action: SpotifyHubAction): string {
    if (action === "installSpicetify") return "Spicetify setup";
    if (action === "restoreSpotify") return "Spotify restore";
    return "Full Spotify uninstall";
  }

  private installLabelFor(action: SpotifyHubAction): string {
    if (action === "installSpicetify") return this.installLabel();
    if (action === "restoreSpotify") return "Restore Spotify";
    return "Full uninstall";
  }
}

export const spotifyHubState = new SpotifyHubState();
