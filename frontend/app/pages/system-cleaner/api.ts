// Typed bridge to the cleaner commands in backend/src/cleaner/.
import { Channel, invoke } from "@tauri-apps/api/core";

export interface CleanerCategory {
  id: string;
  title: string;
  description: string;
  hint: string;
  icon: string;
  /** Some of its folders can only be read as administrator. */
  mayNeedAdmin: boolean;
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
  /** What the skipped files hold: after the administrator pass, what is in use. */
  skippedBytes: number;
  /** Of `skipped`, those in administrator folders, which the administrator pass tries again. */
  adminSkipped: number;
  /** Of `skippedBytes`, the part the administrator pass tries again. */
  adminSkippedBytes: number;
  locked: string[];
}

function channel<T>(onEvent: (e: T) => void): Channel<T> {
  const ch = new Channel<T>();
  ch.onmessage = onEvent;
  return ch;
}

export const cleanerApi = {
  downloadsPreview: () => invoke<import("./downloads").Preview>("cleaner_downloads_preview"),
  downloadsDelete: (paths: string[]) => invoke<import("./downloads").Outcome>("cleaner_downloads_delete", { paths }),
  categories: () => invoke<CleanerCategory[]>("cleaner_categories"),
  scan: (onEvent: (e: ScanEvent) => void) => invoke<ScanSummary>("cleaner_scan", { onEvent: channel(onEvent) }),
  /** The administrator helper approved during a scan is still running. */
  adminReady: () => invoke<boolean>("cleaner_admin_ready"),
  scanElevated: (onEvent: (e: ScanEvent) => void) =>
    invoke<ScanSummary>("cleaner_scan_elevated", { onEvent: channel(onEvent) }),
  clean: (ids: string[], onEvent: (e: CleanEvent) => void) =>
    invoke<CleanSummary>("cleaner_clean", { ids, onEvent: channel(onEvent) }),
  cleanElevated: (ids: string[], onEvent: (e: CleanEvent) => void) =>
    invoke<CleanSummary>("cleaner_clean_elevated", { ids, onEvent: channel(onEvent) }),
};

/** "647.3 MB", "7.3 GB", "0 B" */
export function formatSize(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const decimals = unit === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(decimals)} ${units[unit]}`;
}
