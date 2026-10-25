// Typed bridge to the Disk Analyzer commands in backend/src/disk_analyzer/.
// The page sends one folder to scan, then only ids the backend gave it.
import { Channel, invoke } from "@tauri-apps/api/core";

export { formatSize } from "../system-cleaner/api";

/** "size" is the files' length, "alloc" what they take on the disk. */
export type SizeMode = "size" | "alloc";
export type SortKey = "size" | "name" | "files" | "modified";
export type NodeKind = "dir" | "file" | "link" | "cloud";

/** A treemap block that merges a folder's small entries. */
export const SMALLER_ID = 4294967295;

export interface Drive {
  letter: string;
  /** `C:\` */
  root: string;
  label: string;
  filesystem: string;
  total: number;
  free: number;
  kind: "fixed" | "removable" | "network" | "other";
  /** The drive Windows runs from. */
  system: boolean;
  mftPossible: boolean;
}

export interface Progress {
  files: number;
  dirs: number;
  bytes: number;
  alloc: number;
  current: string;
}

export interface ScanDone {
  scanId: number;
  rootId: number;
  root: string;
  files: number;
  dirs: number;
  bytes: number;
  alloc: number;
  inaccessible: number;
  inaccessiblePaths: string[];
  elapsedMs: number;
  /** Hard links count once per link there. */
  windowsDrive: boolean;
  memory: number;
}

export type ScanEvent = { event: "progress"; data: Progress } | { event: "done"; data: ScanDone } | { event: "cancelled" };

export interface NodeView {
  id: number;
  name: string;
  kind: NodeKind;
  ext: string;
  size: number;
  alloc: number;
  files: number;
  dirs: number;
  /** Seconds since 1970. */
  modified: number;
  hasChildren: boolean;
  /** Never moved to the Recycle Bin; `reason` says why. */
  protected: boolean;
  reason: string | null;
  /** Inside Program Files or ProgramData. */
  caution: boolean;
  inaccessible: boolean;
}

export interface Children {
  items: NodeView[];
  total: number;
  /** What comes after this page. */
  rest: { count: number; size: number; alloc: number } | null;
}

export interface Crumb {
  id: number;
  name: string;
  size: number;
  alloc: number;
}

/** A pruned subtree as flat arrays; index 0 is the folder asked for. */
export interface TreemapPayload {
  scanId: number;
  ids: number[];
  parent: number[];
  size: number[];
  /** 0 folder, 1 file, 2 link, 3 cloud, 4 smaller items. */
  kind: number[];
  ext: number[];
  names: string[];
  exts: string[];
  ranks: number[];
  truncated: boolean;
}

export interface FileView extends NodeView {
  path: string;
  parent: number;
  /** Its extension's colour, as in the treemap. */
  rank: number;
}

export interface ExtStat {
  ext: string;
  other: boolean;
  size: number;
  alloc: number;
  count: number;
  rank: number;
}

export type DeleteEvent = { event: "progress"; data: { done: number; total: number; current: string } };

export interface Skipped {
  path: string;
  reason: string;
}

export interface DeleteOutcome {
  recycled: number;
  ids: number[];
  /** Size on disk given back. */
  freedBytes: number;
  freedSize: number;
  skipped: Skipped[];
}

function channel<T>(onEvent: (e: T) => void): Channel<T> {
  const ch = new Channel<T>();
  ch.onmessage = onEvent;
  return ch;
}

export const diskApi = {
  drives: () => invoke<Drive[]>("disk_analyzer_drives"),
  pickFolder: () => invoke<string | null>("disk_analyzer_pick_folder"),
  scan: (root: string, onEvent: (e: ScanEvent) => void) => invoke<ScanDone | null>("disk_analyzer_scan", { root, onEvent: channel(onEvent) }),
  cancel: () => invoke<void>("disk_analyzer_cancel"),
  clear: () => invoke<void>("disk_analyzer_clear"),
  children: (scanId: number, nodeId: number, offset: number, limit: number, sort: SortKey, sizeMode: SizeMode) =>
    invoke<Children>("disk_analyzer_children", { scanId, nodeId, offset, limit, sort, sizeMode }),
  nodes: (scanId: number, nodeIds: number[]) => invoke<NodeView[]>("disk_analyzer_nodes", { scanId, nodeIds }),
  trail: (scanId: number, nodeId: number) => invoke<Crumb[]>("disk_analyzer_trail", { scanId, nodeId }),
  treemap: (scanId: number, nodeId: number, depth: number, minBytes: number, sizeMode: SizeMode) =>
    invoke<TreemapPayload>("disk_analyzer_treemap", { scanId, nodeId, depth, minBytes, sizeMode }),
  topFiles: (scanId: number, limit: number, sizeMode: SizeMode, filter: string | null) =>
    invoke<FileView[]>("disk_analyzer_top_files", { scanId, limit, sizeMode, filter }),
  extensions: (scanId: number, nodeId: number | null, sizeMode: SizeMode) =>
    invoke<ExtStat[]>("disk_analyzer_extensions", { scanId, nodeId, sizeMode }),
  path: (scanId: number, nodeId: number) => invoke<string>("disk_analyzer_path", { scanId, nodeId }),
  reveal: (scanId: number, nodeId: number) => invoke<void>("disk_analyzer_reveal", { scanId, nodeId }),
  recycle: (scanId: number, nodeIds: number[], onEvent: (e: DeleteEvent) => void) =>
    invoke<DeleteOutcome>("disk_analyzer_delete", { scanId, nodeIds, onEvent: channel(onEvent) }),
};

/** The size a view counts, by the size mode. */
export function sizeOf(item: { size: number; alloc: number }, mode: SizeMode): number {
  return mode === "alloc" ? item.alloc : item.size;
}
