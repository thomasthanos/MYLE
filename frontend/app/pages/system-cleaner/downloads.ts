import { invoke, isTauri } from "@tauri-apps/api/core";
import { readFlag, readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { formatSize } from "./api";

export type Item = { path: string; name: string; isDir: boolean; size: number; changed: number };
export type Kept = { name: string; isDir: boolean; reason: "document" | "recent" | "link"; changed: number };
export type Preview = { folder: string; items: Item[]; total: number; kept: Kept[] };
export type Outcome = { deleted: number; freed: number; skipped: string[] };

/** The card in System Cleaner: on unless turned off. */
export const ENABLED_KEY = "myle.cleaner.downloads.on";
/** Clean on start without asking: off unless turned on. */
export const AUTO_KEY = "myle.cleaner.downloads.auto";
const LAST_AUTO_KEY = "myle.cleaner.downloads.lastAuto";
const DAY_MS = 24 * 60 * 60 * 1000;

export const previewDownloads = () => invoke<Preview>("cleaner_downloads_preview");
export const deleteDownloads = (paths: string[]) => invoke<Outcome>("cleaner_downloads_delete", { paths });

/** At start, at most once a day, when "Clean Downloads automatically" is on:
 *  the same rules as the manual clean, no question asked. */
export async function autoCleanDownloads(): Promise<void> {
  if (!isTauri() || !readFlag(AUTO_KEY, false)) return;
  const last = readJson<number>(LAST_AUTO_KEY, 0);
  if (Date.now() - last < DAY_MS) return;
  writeJson(LAST_AUTO_KEY, Date.now());
  try {
    const preview = await previewDownloads();
    if (!preview.items.length) return;
    const outcome = await deleteDownloads(preview.items.map((item) => item.path));
    if (outcome.deleted)
      toast.success(`Downloads cleaned: ${outcome.deleted} old item${outcome.deleted === 1 ? "" : "s"}, ${formatSize(outcome.freed)} freed.`);
  } catch {
    // Quietly: it tries again tomorrow.
  }
}
