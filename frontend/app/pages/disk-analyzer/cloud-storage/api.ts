import { Channel, invoke } from "@tauri-apps/api/core";

/** "googleDrive", "mega", "dropbox", "oneDrive", or "sync:<app>" for any other
 *  app with a Windows sync root (iCloud, Box, Nextcloud…). */
export type CloudProvider = string;
export interface CloudFolder { path: string; label: string }
export interface CloudStatus {
  provider: CloudProvider;
  name: string;
  folders: CloudFolder[];
  mode: "native" | "guided" | "unavailable";
  detail: string;
}
export interface CloudProgress {
  visited: number;
  requested: number;
  alreadyOnline: number;
  pendingUploads: number;
  skipped: number;
  failed: number;
  /** Google Drive folders whose Online only was turned on. */
  foldersRequested: number;
  foldersAlreadyOnline: number;
  current: string | null;
  /** Waiting for Google Drive's storage window, to press Clear cache there. */
  waitingForDrive: boolean;
}
/** Google Drive's cache: cleared (with its size before), already empty, or
 *  not cleared because Drive's storage window wasn't opened. */
export type DriveCache = { state: "cleared"; size: string | null } | { state: "empty" } | { state: "notOpen" };
export interface CloudOutcome extends Omit<CloudProgress, "current" | "waitingForDrive"> {
  cancelled: boolean;
  errors: string[];
  driveCache: DriveCache | null;
}
export const PROVIDERS: { provider: CloudProvider; name: string }[] = [
  { provider: "googleDrive", name: "Google Drive" },
  { provider: "mega", name: "MEGA" },
  { provider: "dropbox", name: "Dropbox" },
  { provider: "oneDrive", name: "OneDrive" },
];
export const cloudStorageApi = {
  status: () => invoke<{ providers: CloudStatus[] }>("cloud_storage_status"),
  onlineOnly: (clouds: CloudProvider[], expectedFolders: string[], requestId: string, onProgress: (progress: CloudProgress) => void) => {
    const onEvent = new Channel<CloudProgress>();
    onEvent.onmessage = onProgress;
    return invoke<CloudOutcome>("cloud_storage_online_only", { clouds, expectedFolders, requestId, onEvent });
  },
  cancel: (requestId: string) => invoke<void>("cloud_storage_cancel", { requestId }),
};
