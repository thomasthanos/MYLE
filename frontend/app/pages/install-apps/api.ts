// Typed bridge to the Rust commands in backend/src/apps/.
import { Channel, invoke } from "@tauri-apps/api/core";
export interface InstalledPackage {
  id: string;
  name: string;
  version: string;
  available: string | null;
}
export interface CustomStatus {
  id: string;
  installed: boolean;
  version: string | null;
  available: string | null;
  activated: boolean;
}
export interface InstalledReport {
  winget: InstalledPackage[];
  wingetError: string | null;
  custom: CustomStatus[];
}
export interface CustomAppInfo {
  id: string;
  name: string;
  site: string | null;
  icon: string | null;
  category: string | null;
  selfUpdating: boolean;
  conflicts: string[];
  activateLabel: string | null;
  activateAfterInstall: boolean;
}
export interface PackageLinks {
  homepage: string | null;
  iconDomain: string | null;
}
export interface SearchHit {
  id: string;
  name: string;
  version: string;
}
export type Stage = "resolving" | "downloading" | "verifying" | "extracting" | "installing";
export type JobEvent =
  | { event: "stage"; data: { stage: Stage } }
  | { event: "progress"; data: { fraction: number; downloaded: number | null; total: number | null } }
  | { event: "file"; data: { name: string; total: number | null } }
  | { event: "note"; data: { text: string } };
export type JobOutcome =
  | { result: "done"; note: string | null }
  | { result: "upToDate" }
  | { result: "hashMismatch" }
  | { result: "cancelled" };
export type Mode = "install" | "upgrade";
function channel(onEvent: (e: JobEvent) => void): Channel<JobEvent> {
  const ch = new Channel<JobEvent>();
  ch.onmessage = onEvent;
  return ch;
}
export const api = {
  installed: () => invoke<InstalledReport>("apps_installed"),
  customCatalog: () => invoke<CustomAppInfo[]>("apps_custom_catalog"),
  search: (query: string) => invoke<SearchHit[]>("apps_search", { query }),
  packageLinks: (id: string) => invoke<PackageLinks>("apps_package_links", { id }),
  installWinget: (id: string, mode: Mode, onEvent: (e: JobEvent) => void) =>
    invoke<JobOutcome>("apps_install_winget", { id, mode, onEvent: channel(onEvent) }),
  installIgnoringHash: (id: string, mode: Mode, onEvent: (e: JobEvent) => void) =>
    invoke<JobOutcome>("apps_install_ignoring_hash", { id, mode, onEvent: channel(onEvent) }),
  installCustom: (id: string, onEvent: (e: JobEvent) => void) =>
    invoke<JobOutcome>("apps_install_custom", { id, onEvent: channel(onEvent) }),
  cancel: (id: string) => invoke<void>("apps_cancel", { id }),
  activate: (id: string) => invoke<void>("apps_activate", { id }),
  exportList: (json: string) => invoke<boolean>("apps_export_list", { json }),
  importList: () => invoke<string | null>("apps_import_list"),
};
