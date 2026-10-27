// Typed bridge to the Rust updater (backend/src/updater.rs).
import { Channel, invoke } from "@tauri-apps/api/core";

export interface UpdateAsset {
  name: string;
  version?: string;
  url: string;
  size: number;
  digest: string | null;
}

export type UpdateCheck =
  | { status: "upToDate"; current: string; latest: string }
  | { status: "available"; current: string; latest: string; notes: string; asset: UpdateAsset }
  | { status: "notConfigured"; current: string }
  /** Started by the previous version right after it updated this one. */
  | { status: "justUpdated"; current: string };

export type DownloadEvent =
  | { event: "started"; data: { total: number | null } }
  | { event: "progress"; data: { downloaded: number; total: number | null } }
  | { event: "verifying" }
  | { event: "installing" }
  | { event: "restarting"; data: { version: string } };

export function checkForUpdate(): Promise<UpdateCheck> {
  return invoke<UpdateCheck>("check_for_update");
}

/**
 * Downloads, verifies and installs the update, then starts the new version;
 * this app exits once its window is up, so the promise only resolves in demo
 * mode.
 */
export function installUpdate(asset: UpdateAsset, onEvent: (e: DownloadEvent) => void): Promise<void> {
  const channel = new Channel<DownloadEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("install_update", { asset, onEvent: channel });
}

/** The splash is on screen: the main window starts loading behind it. */
export function splashShown(): Promise<void> {
  return invoke<void>("splash_shown");
}

/** The update check is done: the main window opens as soon as it has drawn,
 *  then the splash closes. */
export function finishStartup(): Promise<void> {
  return invoke<void>("finish_startup");
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}
