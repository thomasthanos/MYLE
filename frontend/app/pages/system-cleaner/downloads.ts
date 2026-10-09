import { invoke } from "@tauri-apps/api/core";

export type Item = { path: string; name: string; isDir: boolean; size: number; changed: number };
export type Kept = { name: string; isDir: boolean; reason: "document" | "recent" | "link"; changed: number };
export type Preview = { folder: string; items: Item[]; total: number; kept: Kept[] };
export type Outcome = { deleted: number; freed: number; skipped: string[] };

/** The System Cleaner category id of the Downloads folder. */
export const DOWNLOADS = "downloads";

export const DOWNLOADS_CATEGORY = {
  id: DOWNLOADS,
  title: "Downloads folder",
  description: "Old installers, archives and folders in Downloads. Documents and anything modified in the last 7 days stay.",
  hint: "Deleted permanently, not to the Recycle Bin.",
  icon: "downloads",
  mayNeedAdmin: false,
};

export const previewDownloads = () => invoke<Preview>("cleaner_downloads_preview");
export const deleteDownloads = (paths: string[]) => invoke<Outcome>("cleaner_downloads_delete", { paths });
