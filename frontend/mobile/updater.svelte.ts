// A newer MYLE Passwords on Android (backend/mobile/src/update.rs): found in
// the feed, downloaded into the app's own folder with its progress shown,
// checked, and handed to Android's installer, all without leaving the app.
import { openUrl } from "@tauri-apps/plugin-opener";
import { passwordsApi as api, type MobileUpdate } from "../app/pages/password-manager/api";

export type UpdateStep =
  | { kind: "available" }
  | { kind: "downloading"; percent: number | null }
  /** Android's installer was shown; Install shows it again. */
  | { kind: "installing" }
  /** Android first needs the user to allow this app to install updates. */
  | { kind: "allow" }
  | { kind: "error"; message: string };

function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

class Updater {
  update = $state<MobileUpdate | null>(null);
  step = $state<UpdateStep>({ kind: "available" });
  /** "Later": not shown again until the app starts again. */
  dismissed = $state(false);
  #path: string | null = null;

  readonly visible = $derived(this.update !== null && !this.dismissed);

  async check() {
    if (this.step.kind === "downloading") return;
    try {
      const found = await api.updateCheck();
      if (!found) {
        this.update = null;
        return;
      }
      if (found.version !== this.update?.version) {
        this.#path = null;
        this.step = { kind: "available" };
      }
      this.update = found;
    } catch {
      // Offline: asked again when the app comes back.
    }
  }

  /** Downloads (or reuses) the update, then shows Android's installer. */
  async start() {
    const update = this.update;
    if (!update || this.step.kind === "downloading") return;
    if (!this.#path) {
      this.step = { kind: "downloading", percent: update.size ? 0 : null };
      const unlisten = await api
        .onUpdateProgress(({ downloaded, total }) => {
          const size = total || update.size || 0;
          if (this.step.kind === "downloading" && size > 0) {
            this.step = { kind: "downloading", percent: Math.min(100, Math.round((downloaded / size) * 100)) };
          }
        })
        .catch(() => null);
      try {
        this.#path = await api.updateDownload(update);
      } catch (error) {
        this.step = { kind: "error", message: message(error) };
        return;
      } finally {
        unlisten?.();
      }
    }
    await this.install();
  }

  async install() {
    if (!this.#path) return this.start();
    try {
      await api.updateInstall(this.#path);
      this.step = { kind: "installing" };
    } catch (error) {
      const text = message(error);
      if (text.includes("allow-installs")) {
        this.step = { kind: "allow" };
      } else if (/not downloaded/i.test(text)) {
        this.#path = null;
        this.step = { kind: "available" };
      } else {
        this.step = { kind: "error", message: text };
      }
    }
  }

  /** Back in the app: right after allowing installs, the installer opens. */
  resumed() {
    if (this.step.kind === "allow") void this.install();
    else void this.check();
  }

  /** The last way out: the browser downloads it. */
  inBrowser() {
    if (this.update) void openUrl(this.update.url);
  }
}

export const updater = new Updater();
