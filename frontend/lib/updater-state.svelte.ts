import { checkForUpdate, formatBytes, installUpdate, type UpdateAsset } from "./updater";
import { toast } from "./toast.svelte";

type UpdateView =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "upToDate"; latest: string }
  | { state: "available"; latest: string; asset: UpdateAsset }
  | { state: "downloading"; latest: string; progress: number | null; detail: string }
  | { state: "verifying" }
  | { state: "installing" }
  | { state: "restarting"; version: string }
  | { state: "error"; message: string };

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

class SettingsUpdater {
  view = $state<UpdateView>({ state: "idle" });
  #busy = false;

  async check() {
    if (this.#busy) return;
    this.#busy = true;
    this.view = { state: "checking" };
    try {
      const result = await checkForUpdate();
      if (result.status === "available") this.view = { state: "available", latest: result.latest, asset: result.asset };
      else if (result.status === "upToDate") this.view = { state: "upToDate", latest: result.latest };
      else if (result.status === "justUpdated") this.view = { state: "upToDate", latest: result.current };
      else this.view = { state: "error", message: "Updates are not configured in this build." };
    } catch (error) {
      this.view = { state: "error", message: message(error) };
    } finally {
      this.#busy = false;
    }
  }

  async install(latest: string, asset: UpdateAsset) {
    if (this.#busy) return;
    this.#busy = true;
    this.view = { state: "downloading", latest, progress: null, detail: "" };
    try {
      await installUpdate(asset, (event) => {
        if (event.event === "started" || event.event === "progress") {
          const total = event.data.total;
          const done = event.event === "progress" ? event.data.downloaded : 0;
          this.view = {
            state: "downloading",
            latest,
            progress: total ? Math.min(done / total, 1) : null,
            detail: total ? `${formatBytes(done)} of ${formatBytes(total)}` : formatBytes(done),
          };
        } else if (event.event === "verifying") {
          this.view = { state: "verifying" };
        } else if (event.event === "installing") {
          this.view = { state: "installing" };
        } else if (event.event === "restarting") {
          this.view = { state: "restarting", version: event.data.version || latest };
        }
      });
    } catch (error) {
      this.view = { state: "error", message: message(error) };
      toast.error(`The update failed: ${message(error)}`);
    } finally {
      this.#busy = false;
    }
  }
}

// The native operation survives navigation away from Settings. Its view must
// survive too, so a remounted page can show progress and avoid starting it twice.
export const settingsUpdater = new SettingsUpdater();
