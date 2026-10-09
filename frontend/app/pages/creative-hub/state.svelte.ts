// State of the Creative Hub page: the fixed catalog and the running jobs.
// The list ships with the app; nothing here can change it.
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { toast } from "../../../lib/toast.svelte";
import type { JobEvent, JobOutcome, Stage } from "../install-apps/api";

export const CLIP_STUDIO_ID = "creative.clipstudiopaint.5.1.4";
export const WAND_ENHANCER_ID = "creative.wandenhancer";

export interface CreativeApp {
  id: string;
  name: string;
  description: string;
  category: string;
  icon: string | null;
  sizeHint: number | null;
  /** False while the entry has no download link yet. */
  configured: boolean;
  /** "Download & Setup", or "Download & Extract" for plain packages. */
  actionLabel: string;
}

export interface Job {
  phase: Stage;
  progress: number | null;
  note?: string;
  /** Name of the file being downloaded, so the card can show it. */
  file?: string;
  downloaded?: number;
  total?: number;
  /** Bytes per second, smoothed; only while downloading. */
  speed?: number;
}

/** Speed samples at most this often, averaged so the number does not jump. */
const SPEED_SAMPLE_MS = 500;

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

class CreativeState {
  apps = $state<CreativeApp[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  jobs = $state<Record<string, Job>>({});
  /** True when the Clip Studio restore exe is present in Downloads. */
  clipStudioRestoreReady = $state(false);
  /** True when WandEnhancer.exe is present in roaming directory. */
  wandEnhancerReady = $state(false);
  #loaded = false;

  async load() {
    if (!isTauri() || this.loading || this.#loaded) return;
    this.loading = true;
    try {
      this.apps = await invoke<CreativeApp[]>("creative_catalog");
      this.error = null;
      this.#loaded = true;
      // Quick synchronous fs checks — no perceptible delay.
      this.clipStudioRestoreReady = await invoke<boolean>(
        "creative_clip_studio_restore_available",
      );
      this.wandEnhancerReady = await invoke<boolean>(
        "creative_wand_enhancer_available",
      );
    } catch (err) {
      this.error = message(err);
    } finally {
      this.loading = false;
    }
  }

  async install(app: CreativeApp) {
    if (this.jobs[app.id]) return;
    this.jobs[app.id] = { phase: "resolving", progress: null };
    let sample: { at: number; bytes: number } | null = null;
    const onEvent = new Channel<JobEvent>();
    onEvent.onmessage = (e) => {
      const job = this.jobs[app.id];
      if (!job) return;
      if (e.event === "stage") {
        job.phase = e.data.stage;
        job.progress = null;
        job.speed = undefined;
        sample = null;
      } else if (e.event === "progress") {
        // No total: the size is unknown, so the bar has nothing to fill.
        job.progress = e.data.total ? e.data.fraction : null;
        job.downloaded = e.data.downloaded ?? undefined;
        job.total = e.data.total ?? undefined;
        const bytes = e.data.downloaded;
        if (job.phase === "downloading" && bytes !== null) {
          const now = performance.now();
          if (!sample) sample = { at: now, bytes };
          else if (now - sample.at >= SPEED_SAMPLE_MS) {
            const current = ((bytes - sample.bytes) * 1000) / (now - sample.at);
            job.speed = job.speed === undefined ? current : job.speed * 0.7 + current * 0.3;
            sample = { at: now, bytes };
          }
        }
      } else if (e.event === "file") {
        job.file = e.data.name;
        job.total = e.data.total ?? undefined;
      } else {
        job.note = e.data.text;
      }
    };
    try {
      const outcome = await invoke<JobOutcome>("creative_install", { id: app.id, onEvent });
      if (outcome.result === "done") {
        toast.success(`${app.name} is set up.${outcome.note ? ` ${outcome.note}` : ""}`);
      } else if (outcome.result === "cancelled") {
        toast.info(`${app.name}: cancelled.`);
      }
    } catch (err) {
      toast.error(`${app.name}: ${message(err)}`);
    } finally {
      delete this.jobs[app.id];
      // Re-check immediately after any install — no polling, no delay.
      if (app.id === CLIP_STUDIO_ID) {
        this.clipStudioRestoreReady = await invoke<boolean>(
          "creative_clip_studio_restore_available",
        );
      }
      if (app.id === WAND_ENHANCER_ID) {
        this.wandEnhancerReady = await invoke<boolean>(
          "creative_wand_enhancer_available",
        );
      }
    }
  }

  cancel(app: CreativeApp) {
    void invoke("apps_cancel", { id: app.id });
  }

  async swapExe() {
    try {
      const note = await invoke<string>("creative_clip_studio_swap_exe");
      toast.success(note);
    } catch (err) {
      toast.error(`Swap failed: ${message(err)}`);
    }
  }

  async launchWandEnhancer() {
    try {
      const note = await invoke<string>("creative_wand_enhancer_launch");
      toast.success(note);
    } catch (err) {
      toast.error(`Launch failed: ${message(err)}`);
    }
  }
}

export const creativeState = new CreativeState();
