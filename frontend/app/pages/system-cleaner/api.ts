// Typed bridge to the cleaner commands in backend/src/cleaner/.
import { invoke } from "@tauri-apps/api/core";
import { channel } from "../../../lib/ipc";
import { formatBytes as formatSize } from "../../../lib/format";
export interface CleanerCategory {
  id: string;
  title: string;
  description: string;
  hint: string;
  icon: string;
  /** Includes system folders handled by the administrator scan. */
  mayNeedAdmin: boolean;
  adminOnly: boolean;
}
export type ScanEvent = {
  event: "category";
  data: { id: string; bytes: number; files: number; locked: boolean };
};
export type CleanEvent =
  | { event: "progress"; data: { done: number; total: number; current: string } }
  | {
      event: "category";
      data: {
        id: string;
        bytes: number;
        files: number;
        skipped: number;
        /** What the skipped files hold. */
        skippedBytes: number;
        /** Skipped because Windows reported a file in use. */
        inUse?: number;
        inUseBytes?: number;
        adminSkipped: number;
        adminSkippedBytes: number;
        locked: boolean;
      };
    };
export interface ScanSummary {
  bytes: number;
  /** Categories that need administrator rights to be measured fully. */
  locked: string[];
}
export interface CleanSummary {
  freed: number;
  files: number;
  skipped: number;
  /** Bytes belonging to skipped items. */
  skippedBytes: number;
  /** Of `skipped`, those in administrator folders, which the administrator pass tries again. */
  adminSkipped: number;
  /** Of `skippedBytes`, the part the administrator pass tries again. */
  adminSkippedBytes: number;
  locked: string[];
}
export const cleanerApi = {
  downloadsPreview: () => invoke<import("./downloads").Preview>("cleaner_downloads_preview"),
  downloadsDelete: (paths: string[]) => invoke<import("./downloads").Outcome>("cleaner_downloads_delete", { paths }),
  categories: () => invoke<CleanerCategory[]>("cleaner_categories"),
  isElevated: () => invoke<boolean>("cleaner_is_elevated"),
  scan: (onEvent: (e: ScanEvent) => void) => invoke<ScanSummary>("cleaner_scan", { onEvent: channel(onEvent) }),
  /** The app or its administrator helper can access system folders. */
  adminReady: () => invoke<boolean>("cleaner_admin_ready"),
  /** Reports the system folders only, to be added to the user scan. */
  scanElevated: (onEvent: (e: ScanEvent) => void) =>
    invoke<ScanSummary>("cleaner_scan_elevated", { onEvent: channel(onEvent) }),
  clean: (ids: string[], onEvent: (e: CleanEvent) => void) =>
    invoke<CleanSummary>("cleaner_clean", { ids, onEvent: channel(onEvent) }),
  cleanElevated: (ids: string[], onEvent: (e: CleanEvent) => void) =>
    invoke<CleanSummary>("cleaner_clean_elevated", { ids, onEvent: channel(onEvent) }),
  relaunchElevated: () => invoke<void>("cleaner_relaunch_elevated"),
};
/** "647.3 MB", "7.3 GB", "0 B" */
export { formatSize };
