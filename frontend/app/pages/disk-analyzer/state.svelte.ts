// State of the Disk Analyzer page: the drives, the running scan, and the slices of its result the page shows. The backend keeps the whole result; the page asks for a folder's children, a treemap or the largest files. It sends one path to scan, then only ids the backend gave it, so moving items to the Recycle Bin goes by id and the backend checks every path itself.
import { isTauri } from "@tauri-apps/api/core";
import { SvelteSet } from "svelte/reactivity";
import { confirm, type ConfirmOptions } from "../../../lib/confirm.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import {
  diskApi,
  formatSize,
  sizeOf,
  SMALLER_ID,
  type Crumb,
  type DeleteProgress,
  type Drive,
  type ExtStat,
  type FileView,
  type NodeView,
  type Progress,
  type ScanDone,
  type SizeMode,
  type Skipped,
  type SortKey,
  type TreemapPayload,
} from "./api";
import { flatten, type ChildPage, type Row } from "./rows";
export const KEY = { sizeMode: "myle.diskAnalyzer.sizeMode", view: "myle.diskAnalyzer.view", fast: "myle.diskAnalyzer.fast" };
export type Phase = "idle" | "scanning" | "ready" | "deleting";
export type View = "folders" | "files" | "types";
/** Children asked for at once. */
const PAGE = 500;
const TOP_FILES = 1000;
const FILTER_DELAY_MS = 150;
/** How many folder levels the treemap opens. */
const DEPTH = 10;
/** A folder's entries smaller than about this many pixels share one block. */
const MIN_PIXELS = 9;
const TEN_GB = 10 * 1024 ** 3;
const MANY_FILES = 1000;
/** The most warnings the Recycle Bin question lists one by one. */
const CAUTIONS_SHOWN = 4;
const message = (err: unknown): string => (err instanceof Error ? err.message : String(err));
const isSizeMode = (v: unknown) => v === "size" || v === "alloc";
const isView = (v: unknown) => v === "folders" || v === "files" || v === "types";
const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
/** The question asked before anything goes to the Recycle Bin. */
export function recycleQuestion(items: readonly NodeView[], locked: number, mode: SizeMode, permanent = false): ConfirmOptions {
  const bytes = items.reduce((sum, item) => sum + (permanent ? item.size : sizeOf(item, mode)), 0);
  const files = items.reduce((sum, item) => sum + (item.kind === "dir" ? item.files : 1), 0);
  const lines = [
    permanent
      ? `${plural(items.length, "item")}, ${formatSize(bytes)}. ${items.length === 1 ? "It" : "They"} will bypass the Recycle Bin and cannot be restored. This cannot be undone.`
      : `${plural(items.length, "item")}, ${formatSize(bytes)}. You can restore ${items.length === 1 ? "it" : "them"} from the Recycle Bin.`,
  ];
  if (permanent) {
    const names = items.slice(0, CAUTIONS_SHOWN).map((item) => `• ${item.name}`);
    if (items.length > CAUTIONS_SHOWN) names.push(`• and ${plural(items.length - CAUTIONS_SHOWN, "more item")}`);
    lines.push(names.join("\n"));
  }
  if (bytes > TEN_GB) {
    lines.push(permanent ? "That is more than 10 GB. Check the selected items carefully before continuing." : "That is more than 10 GB. Anything too big for the Recycle Bin stays where it is: nothing is deleted for good.");
  }
  if (files > MANY_FILES) lines.push(`That is ${files.toLocaleString()} files.`);
  const warned = items.filter((item) => item.caution);
  if (warned.length) {
    const shown = warned.slice(0, CAUTIONS_SHOWN).map((item) => `• ${item.name}: ${item.caution}`);
    if (warned.length > CAUTIONS_SHOWN) shown.push(`• and ${plural(warned.length - CAUTIONS_SHOWN, "more item")} like these`);
    lines.push(["Think twice:", ...shown].join("\n"));
  }
  if (items.some((item) => item.kind === "cloud")) lines.push("Cloud files are removed from your cloud storage too.");
  if (locked) lines.push(`${plural(locked, "protected item")} ${locked === 1 ? "stays where it is" : "stay where they are"}.`);
  return { title: permanent ? "Delete permanently?" : "Move to Recycle Bin?", message: lines.join("\n\n"), confirmLabel: permanent ? "Delete permanently" : "Move to Recycle Bin", danger: true };
}
class DiskAnalyzerState {
  drives = $state.raw<Drive[]>([]);
  /** What Scan scans: a drive's root or a chosen folder. */
  target = $state<string | null>(null);
  phase = $state<Phase>("idle");
  progress = $state.raw<Progress | null>(null);
  done = $state.raw<ScanDone | null>(null);
  /** The scanned folder, with its totals as they are now. */
  root = $state.raw<NodeView | null>(null);
  sizeMode = $state<SizeMode>(readJson<SizeMode>(KEY.sizeMode, "alloc", isSizeMode));
  view = $state<View>(readJson<View>(KEY.view, "folders", isView));
  /** Read a whole NTFS drive's file table (one UAC prompt) instead of walking it. */
  fast = $state(readJson<boolean>(KEY.fast, true, (v) => typeof v === "boolean"));
  sort = $state<SortKey>("size");
  /** The folder the treemap shows, and the way down to it. */
  current = $state(0);
  trail = $state.raw<Crumb[]>([]);
  treemap = $state.raw<TreemapPayload | null>(null);
  /** The children loaded so far, by folder. */
  pages = $state.raw<ReadonlyMap<number, ChildPage>>(new Map());
  readonly expanded = new SvelteSet<number>();
  readonly selection = new SvelteSet<number>();
  /** The row with the keyboard focus: the last one clicked or reached. */
  focused = $state<number | null>(null);
  /** Asks the folder list to bring a row into view. */
  scrollTo = $state.raw<{ id: number } | null>(null);
  topFiles = $state.raw<FileView[] | null>(null);
  filter = $state("");
  extensions = $state.raw<ExtStat[] | null>(null);
  deleting = $state.raw<DeleteProgress | null>(null);
  deletingPermanently = $state(false);
  /** What the last move to the Recycle Bin left behind, and why. */
  skipped = $state.raw<Skipped[]>([]);
  error = $state<string | null>(null);
  #anchor: number | null = null;
  #loadPromise: Promise<void> | undefined;
  #area = { width: 0, height: 0 };
  #treemapArea = 0;
  #treemapRequest = 0;
  #navigationRequest = 0;
  #treeRequest = 0;
  #scanRequest = 0;
  #filesRequest = 0;
  #filterTimer: ReturnType<typeof setTimeout> | undefined;
  #typesRequest = 0;
  #childRequests = new Map<number, { scanId: number; generation: number; promise: Promise<void> }>();
  /** Bumped when the size mode or the sort changes: older answers are dropped. */
  #generation = 0;
  readonly scanId = $derived(this.done?.scanId ?? null);
  readonly busy = $derived(this.phase === "scanning" || this.phase === "deleting");
  readonly rows: Row[] = $derived(flatten(this.root, this.pages, this.expanded, this.sizeMode));
  readonly targetDrive = $derived(this.drives.find((d) => d.root.toLowerCase() === this.target?.toLowerCase()) ?? null);
  /** For a whole drive: what was read of what is used, 0…1. Null: unknown, the bar sweeps. */
  readonly scanShare = $derived.by(() => {
    const drive = this.targetDrive;
    const used = drive ? drive.total - drive.free : 0;
    return this.progress && used > 0 ? Math.min(0.99, this.progress.alloc / used) : null;
  });
  /** Scan reads the file table: a whole fixed NTFS drive, with the fast scan on. */
  readonly fastScan = $derived(this.fast && !!this.targetDrive?.mftPossible);
  readonly currentSize = $derived(this.trail.length ? sizeOf(this.trail[this.trail.length - 1], this.sizeMode) : 0);
  async load() {
    if (!isTauri()) return;
    if (this.#loadPromise) return this.#loadPromise;
    this.#loadPromise = (async () => {
      try {
        this.drives = await diskApi.drives();
        if (!this.target) {
          const first = this.drives.find((d) => d.system) ?? this.drives.find((d) => d.kind === "fixed") ?? this.drives[0];
          this.target = first?.root ?? null;
        }
      } catch (err) {
        this.error = message(err);
      }
    })();
    try {
      await this.#loadPromise;
    } finally {
      this.#loadPromise = undefined;
    }
  }
  /** Re-reads the saved view choices (after account sync replaced them). */
  reloadChoices() {
    const mode = readJson<SizeMode>(KEY.sizeMode, this.sizeMode, isSizeMode);
    if (mode !== this.sizeMode) void this.setSizeMode(mode, false);
    this.view = readJson<View>(KEY.view, this.view, isView);
  }
  choose(root: string) {
    if (!this.busy) this.target = root;
  }
  /** Lets the user pick a folder, then scans it. */
  async pickFolder() {
    if (this.busy) return;
    try {
      const folder = await diskApi.pickFolder();
      if (!folder) return;
      this.target = folder;
      await this.scan();
    } catch (err) {
      toast.error(`The folder picker did not open: ${message(err)}`);
    }
  }
  async scan() {
    const root = this.target;
    if (!root || this.busy) return;
    const request = ++this.#scanRequest;
    this.#reset();
    this.phase = "scanning";
    this.progress = { files: 0, dirs: 0, bytes: 0, alloc: 0, current: root };
    try {
      const done = await diskApi.scan(root, this.fastScan, (e) => {
        if (request === this.#scanRequest && e.event === "progress") this.progress = e.data;
      });
      if (request !== this.#scanRequest) return;
      if (!done) {
        this.phase = "idle";
        toast.info("Scan cancelled.");
        return;
      }
      this.done = done;
      if (done.fallback) toast.info(`The fast scan was not possible (${done.fallback.replace(/\.$/, "")}), so the drive was scanned folder by folder.`);
      this.current = done.rootId;
      this.expanded.add(done.rootId);
      this.phase = "ready";
      await this.#afterScan();
    } catch (err) {
      if (request !== this.#scanRequest) return;
      this.phase = "idle";
      this.error = message(err);
      toast.error(`Scan failed: ${message(err)}`);
    } finally {
      if (request === this.#scanRequest) this.progress = null;
    }
  }
  cancel() {
    if (this.phase === "scanning") void diskApi.cancel().catch(() => {});
  }
  setFast(on: boolean) {
    this.fast = on;
    writeJson(KEY.fast, on);
  }
  #reset() {
    this.#childRequests.clear();
    clearTimeout(this.#filterTimer);
    this.#filterTimer = undefined;
    this.#navigationRequest++;
    this.#treeRequest++;
    this.#treemapRequest++;
    this.#filesRequest++;
    this.#typesRequest++;
    this.done = null;
    this.root = null;
    this.current = 0;
    this.trail = [];
    this.treemap = null;
    this.pages = new Map();
    this.expanded.clear();
    this.selection.clear();
    this.focused = null;
    this.scrollTo = null;
    this.#anchor = null;
    this.topFiles = null;
    this.extensions = null;
    this.skipped = [];
    this.error = null;
    this.#treemapArea = 0;
  }
  async #afterScan() {
    const done = this.done;
    if (!done) return;
    try {
      await Promise.all([this.#loadRoot(), this.#loadTrail(done.rootId), this.loadChildren(done.rootId)]);
    } catch (err) {
      if (this.scanId === done.scanId) toast.error(`The results could not be read: ${message(err)}`);
      return;
    }
    if (this.scanId !== done.scanId) return;
    await this.loadTreemap();
    if (this.scanId !== done.scanId) return;
    if (this.view === "files") void this.loadTopFiles();
    if (this.view === "types") void this.loadExtensions();
  }
  async #loadRoot() {
    const done = this.done;
    if (!done) return;
    const [root] = await diskApi.nodes(done.scanId, [done.rootId]);
    if (this.scanId === done.scanId) this.root = root ?? null;
  }
  async #loadTrail(id: number) {
    const scanId = this.scanId;
    if (scanId === null) return;
    const request = this.#navigationRequest;
    const trail = await diskApi.trail(scanId, id);
    if (this.scanId === scanId && request === this.#navigationRequest) this.trail = trail;
  }
  // ------------------------------------------------------------------ The folder list
  /** Loads a folder's children, or its next page (`more`). */
  async loadChildren(id: number, more = false, quiet = false) {
    const scanId = this.scanId;
    if (scanId === null) return;
    const generation = this.#generation;
    const pending = this.#childRequests.get(id);
    if (pending?.scanId === scanId && pending.generation === generation) return pending.promise;
    const before = this.pages.get(id);
    const offset = more && before ? before.items.length : 0;
    this.#setPage(id, { items: [], total: 0, rest: null, ...before, loading: true });
    const promise = this.#readChildren(scanId, generation, id, offset, quiet);
    const request = { scanId, generation, promise };
    this.#childRequests.set(id, request);
    try {
      await promise;
    } finally {
      if (this.#childRequests.get(id) === request) this.#childRequests.delete(id);
    }
  }
  async #readChildren(scanId: number, generation: number, id: number, offset: number, quiet: boolean) {
    try {
      const page = await diskApi.children(scanId, id, offset, PAGE, this.sort, this.sizeMode);
      if (this.scanId !== scanId || generation !== this.#generation) return;
      const items = offset ? [...(this.pages.get(id)?.items ?? []), ...page.items] : page.items;
      this.#setPage(id, { items, total: page.total, rest: page.rest, loading: false });
    } catch (err) {
      if (this.scanId !== scanId || generation !== this.#generation) return;
      this.expanded.delete(id);
      const next = new Map(this.pages);
      next.delete(id);
      this.pages = next;
      if (!quiet) toast.error(`That folder could not be opened: ${message(err)}`);
    }
  }
  #setPage(id: number, page: ChildPage) {
    const next = new Map(this.pages);
    next.set(id, page);
    this.pages = next;
  }
  /** Opens or closes a folder in the list. */
  toggle(id: number, open?: boolean) {
    const isOpen = this.expanded.has(id);
    const want = open ?? !isOpen;
    if (want === isOpen) return;
    if (!want) {
      this.expanded.delete(id);
      return;
    }
    this.expanded.add(id);
    if (!this.pages.has(id)) void this.loadChildren(id);
  }
  /** Picks an item: on its own, added or taken out (Ctrl), or a run of rows (Shift). */
  select(id: number, how: "only" | "toggle" | "range" = "only") {
    if (id === SMALLER_ID) return;
    const rowIds = () => this.rows.flatMap((row) => (row.type === "node" ? [row.node.id] : []));
    if (how === "range" && this.#anchor !== null) {
      const ids = rowIds();
      const [a, b] = [ids.indexOf(this.#anchor), ids.indexOf(id)];
      if (a >= 0 && b >= 0) {
        this.selection.clear();
        for (const one of ids.slice(Math.min(a, b), Math.max(a, b) + 1)) this.selection.add(one);
        this.focused = id;
        return;
      }
    }
    if (how === "toggle") {
      if (this.selection.has(id)) this.selection.delete(id);
      else this.selection.add(id);
    } else {
      this.selection.clear();
      this.selection.add(id);
    }
    this.#anchor = id;
    this.focused = id;
  }
  clearSelection() {
    this.selection.clear();
    this.#anchor = null;
  }
  /** Opens the list down to an item and brings it into view. */
  async showInTree(id: number) {
    const scanId = this.scanId;
    if (scanId === null || id === SMALLER_ID) return;
    const request = ++this.#treeRequest;
    const current = () => request === this.#treeRequest && this.scanId === scanId;
    try {
      const trail = await diskApi.trail(scanId, id);
      if (!current()) return;
      for (const crumb of trail.slice(0, -1)) {
        this.expanded.add(crumb.id);
        if (!this.pages.has(crumb.id) || this.pages.get(crumb.id)?.loading) await this.loadChildren(crumb.id);
        if (!current()) return;
      }
      // Past the first page of its folder: load more until it shows.
      const parent = trail.at(-2)?.id;
      for (let tries = 0; parent !== undefined && tries < 20; tries++) {
        const page = this.pages.get(parent);
        if (!page || !page.rest || page.items.some((item) => item.id === id)) break;
        await this.loadChildren(parent, true);
        if (!current()) return;
      }
      this.focused = id;
      this.scrollTo = { id };
    } catch {
      // A stale id (a new scan started): nothing to show.
    }
  }
  async setSort(sort: SortKey) {
    if (sort === this.sort) return;
    this.sort = sort;
    this.#generation++;
    await this.#reloadPages();
  }
  /** Reloads every open folder, keeping what is shown until the answer is in. */
  async #reloadPages() {
    this.pages = new Map([...this.pages].filter(([id]) => this.expanded.has(id)).map(([id, page]) => [id, { ...page, loading: false }]));
    await Promise.all([...this.expanded].map((id) => this.loadChildren(id, false, true)));
  }
  // ------------------------------------------------------------------ The treemap
  /* The treemap's size on screen. A much bigger or smaller one asks for a new payload; the layout itself follows every change. */
  setArea(width: number, height: number) {
    this.#area = { width, height };
    const before = this.#treemapArea;
    const now = width * height;
    if (!before || now > before * 1.5 || now < before / 1.5) void this.loadTreemap();
  }
  async loadTreemap() {
    const scanId = this.scanId;
    const { width, height } = this.#area;
    if (scanId === null || width < 8 || height < 8) return;
    const request = ++this.#treemapRequest;
    const total = this.currentSize || (this.root ? sizeOf(this.root, this.sizeMode) : 0);
    const minBytes = Math.max(1, Math.floor((total * MIN_PIXELS) / (width * height)));
    try {
      const payload = await diskApi.treemap(scanId, this.current, DEPTH, minBytes, this.sizeMode);
      if (request !== this.#treemapRequest || this.scanId !== scanId) return;
      this.treemap = payload;
      this.#treemapArea = width * height;
    } catch (err) {
      if (request === this.#treemapRequest && this.scanId === scanId) toast.error(`The treemap could not be drawn: ${message(err)}`);
    }
  }
  /** Shows a folder in the treemap. */
  async zoomTo(id: number) {
    const scanId = this.scanId;
    if (scanId === null || id === SMALLER_ID) return;
    // Picking the current folder also cancels a pending move away from it.
    const request = ++this.#navigationRequest;
    if (id === this.current) return;
    this.#treemapRequest++;
    try {
      const trail = await diskApi.trail(scanId, id);
      if (request !== this.#navigationRequest || this.scanId !== scanId) return;
      this.current = id;
      this.trail = trail;
      this.#typesRequest++;
      this.extensions = null;
    } catch (err) {
      if (request !== this.#navigationRequest || this.scanId !== scanId) return;
      toast.error(`That folder could not be shown: ${message(err)}`);
      return;
    }
    await this.loadTreemap();
    if (request !== this.#navigationRequest || this.scanId !== scanId) return;
    if (this.view === "types" || this.extensions) void this.loadExtensions();
  }
  /** One folder up. */
  zoomOut() {
    if (this.trail.length > 1) void this.zoomTo(this.trail[this.trail.length - 2].id);
  }
  // ------------------------------------------------------------------ Size mode, tabs, the other lists
  async setSizeMode(mode: SizeMode, save = true) {
    if (mode === this.sizeMode) return;
    this.sizeMode = mode;
    if (save) writeJson(KEY.sizeMode, mode);
    if (this.scanId === null) return;
    this.#generation++;
    this.#filesRequest++;
    this.#typesRequest++;
    await Promise.all([
      this.#reloadPages(),
      this.loadTreemap(),
      this.topFiles || this.view === "files" ? this.loadTopFiles() : null,
      this.extensions || this.view === "types" ? this.loadExtensions() : null,
    ]);
  }
  setView(view: View) {
    this.view = view;
    writeJson(KEY.view, view);
    if (view === "files" && !this.topFiles) void this.loadTopFiles();
    if (view === "types" && !this.extensions) void this.loadExtensions();
  }
  async loadTopFiles() {
    clearTimeout(this.#filterTimer);
    this.#filterTimer = undefined;
    const scanId = this.scanId;
    if (scanId === null) return;
    const request = ++this.#filesRequest;
    try {
      const files = await diskApi.topFiles(scanId, TOP_FILES, this.sizeMode, this.filter.trim() || null);
      if (request === this.#filesRequest && this.scanId === scanId) this.topFiles = files;
    } catch (err) {
      if (request === this.#filesRequest && this.scanId === scanId) toast.error(`The largest files could not be listed: ${message(err)}`);
    }
  }
  setFilter(text: string) {
    const before = this.filter.trim();
    this.filter = text;
    if (before === text.trim()) return;
    this.#filesRequest++;
    clearTimeout(this.#filterTimer);
    this.#filterTimer = undefined;
    if (this.scanId === null) return;
    this.#filterTimer = setTimeout(() => {
      this.#filterTimer = undefined;
      void this.loadTopFiles();
    }, FILTER_DELAY_MS);
  }
  async loadExtensions() {
    const scanId = this.scanId;
    if (scanId === null) return;
    const request = ++this.#typesRequest;
    const folder = this.current;
    try {
      const stats = await diskApi.extensions(scanId, folder, this.sizeMode);
      if (request === this.#typesRequest && this.scanId === scanId && this.current === folder) this.extensions = stats;
    } catch (err) {
      if (request === this.#typesRequest && this.scanId === scanId && this.current === folder) toast.error(`File types could not be counted: ${message(err)}`);
    }
  }
  // ------------------------------------------------------------------ Explorer, the path, the Recycle Bin
  async reveal(id: number) {
    if (this.scanId === null || id === SMALLER_ID) return;
    try {
      await diskApi.reveal(this.scanId, id);
    } catch (err) {
      toast.error(`Explorer could not open it: ${message(err)}`);
    }
  }
  async copyPath(id: number) {
    if (this.scanId === null || id === SMALLER_ID) return;
    try {
      await navigator.clipboard.writeText(await diskApi.path(this.scanId, id));
      toast.success("Path copied.");
    } catch (err) {
      toast.error(`The path could not be copied: ${message(err)}`);
    }
  }
  /** Asks, then moves the items (the selection by default) to the Recycle Bin. */
  async recycle(ids: number[] = [...this.selection]) {
    return this.#remove(ids, false);
  }
  /** A separate, explicitly confirmed action for items the bin could not take. */
  async deletePermanently(ids: number[]) {
    return this.#remove(ids, true);
  }
  async #remove(ids: number[], permanent: boolean) {
    const scanId = this.scanId;
    const wanted = ids.filter((id) => id !== SMALLER_ID);
    if (scanId === null || this.phase !== "ready" || !wanted.length) return;
    let views: NodeView[];
    try {
      views = await diskApi.nodes(scanId, wanted);
    } catch (err) {
      toast.error(message(err));
      return;
    }
    const allowed = views.filter((view) => !view.protected);
    const locked = views.length - allowed.length;
    if (!allowed.length) {
      const one = views[0];
      toast.info(views.length === 1 && one ? `“${one.name}” stays where it is. ${one.reason ?? ""}`.trim() : "None of these can be moved to the Recycle Bin.");
      return;
    }
    if (!(await confirm(recycleQuestion(allowed, locked, this.sizeMode, permanent)))) return;
    if (this.phase !== "ready" || this.scanId !== scanId) return;
    this.phase = "deleting";
    this.deletingPermanently = permanent;
    this.deleting = { done: 0, total: allowed.length, current: allowed[0].name };
    try {
      const remove = permanent ? diskApi.deletePermanently : diskApi.recycle;
      const outcome = await remove(
        scanId,
        allowed.map((view) => view.id),
        (e) => (this.deleting = e.data),
      );
      this.skipped = permanent ? [...this.skipped.filter((item) => !wanted.includes(item.id)), ...outcome.skipped] : outcome.skipped;
      for (const id of outcome.ids) {
        this.selection.delete(id);
        this.expanded.delete(id);
      }
      if (outcome.recycled) {
        toast.success(`Moved ${plural(outcome.recycled, "item")} to the Recycle Bin · ${formatSize(outcome.freedBytes)} freed.`);
      }
      if (outcome.deleted) {
        toast.success(`${plural(outcome.deleted, "item")} permanently deleted · ${formatSize(outcome.freedBytes)} freed.`);
      }
      if (outcome.skipped.length) {
        const n = outcome.skipped.length;
        toast.info(`${plural(n, "item")} ${n === 1 ? "stays where it is" : "stay where they are"}. The list says why.`);
      }
      await this.#afterDelete(outcome.ids);
    } catch (err) {
      toast.error(`${permanent ? "Permanent deletion" : "Moving to the Recycle Bin"} failed: ${message(err)}`);
    } finally {
      this.phase = "ready";
      this.deleting = null;
      this.deletingPermanently = false;
    }
  }
  dismissSkipped() {
    this.skipped = [];
  }
  /** The totals moved on in the backend: show them, without scanning again. */
  async #afterDelete(gone: number[]) {
    if (!gone.length || this.scanId === null) return;
    // The treemap's folder (or one above it) went: show what is left above.
    const at = this.trail.findIndex((crumb) => gone.includes(crumb.id));
    if (at > 0) this.current = this.trail[at - 1].id;
    if (this.focused !== null && gone.includes(this.focused)) this.focused = null;
    this.#generation++;
    try {
      await Promise.all([this.#loadRoot(), this.#loadTrail(this.current)]);
    } catch {
      // Shown as it was; the next scan puts it right.
    }
    await Promise.all([
      this.#reloadPages(),
      this.loadTreemap(),
      this.topFiles ? this.loadTopFiles() : null,
      this.extensions ? this.loadExtensions() : null,
    ]);
  }
}
export const diskState = new DiskAnalyzerState();
