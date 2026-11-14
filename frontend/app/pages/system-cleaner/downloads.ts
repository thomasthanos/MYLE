
export type Preview = { items: { path: string; size: number }[]; total: number };
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
  adminOnly: false,
};
