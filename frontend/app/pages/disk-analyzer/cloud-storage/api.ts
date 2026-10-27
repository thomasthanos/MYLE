import { Channel, invoke } from "@tauri-apps/api/core";

export type CloudProvider = "googleDrive" | "mega" | "dropbox" | "oneDrive";
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
  current: string | null;
}
export interface CloudOutcome extends Omit<CloudProgress, "current"> {
  cancelled: boolean;
  errors: string[];
}
export const PROVIDERS: { provider: CloudProvider; name: string }[] = [
  { provider: "googleDrive", name: "Google Drive" },
  { provider: "mega", name: "MEGA" },
  { provider: "dropbox", name: "Dropbox" },
  { provider: "oneDrive", name: "OneDrive" },
];
export const cloudStorageApi = {
  status: () => invoke<{ providers: CloudStatus[] }>("cloud_storage_status"),
  onlineOnly: (provider: CloudProvider, expectedFolders: string[], requestId: string, onProgress: (progress: CloudProgress) => void) => {
    const onEvent = new Channel<CloudProgress>();
    onEvent.onmessage = onProgress;
    return invoke<CloudOutcome>("cloud_storage_online_only", { provider, expectedFolders, requestId, onEvent });
  },
  cancel: (requestId: string) => invoke<void>("cloud_storage_cancel", { requestId }),
  openFolder: (provider: CloudProvider, path: string) => invoke<void>("cloud_storage_open_folder", { provider, path }),
  openHelp: (provider: CloudProvider) => invoke<void>("cloud_storage_open_help", { provider }),
};
