// State of the System Cleaner page: fixed categories, measured sizes and selections. System folders are included through the explicit administrator scan.
import { isTauri } from "@tauri-apps/api/core";
import { SvelteSet } from "svelte/reactivity";
import { confirm } from "../../../lib/confirm.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { cleanerApi, formatSize, type CleanerCategory } from "./api";
import { DOWNLOADS, DOWNLOADS_CATEGORY, type Preview } from "./downloads";
const KEY = { selected: "cleaner.selected", lastCleaned: "cleaner.lastCleaned" };
/** The backend's answer when the UAC prompt is declined. */
const DECLINED = "Administrator approval was declined.";
/* The bar moves for the eye, not for the clock: one step is kept on screen about this long, while the fill eases towards it. A slow step is never made slower, and the full bar is held for a moment at the end. */
const STEP_MS = 210;
const HOLD_MS = 420;
export interface Measured { bytes: number; files: number; locked: boolean }
function addSystemMeasurement(user: Measured | undefined, system: Measured): Measured {
  return {
    bytes: (user?.bytes ?? 0) + system.bytes,
    files: (user?.files ?? 0) + system.files,
    locked: system.locked,
  };
}
/** What a clean pass removed from one category, and what it left. */
export interface Outcome { freed: number; skipped: number; skippedBytes: number; adminSkipped: number; adminSkippedBytes: number }
/** The step the running clean is on. */
export interface CleanProgress { pass: "user" | "administrator"; step: number; total: number; current: string }
type Phase = "idle" | "scanning" | "scanning-admin" | "cleaning";
const isStringArray = (v: unknown) => Array.isArray(v) && v.every((x) => typeof x === "string");
const message = (err: unknown): string => (err instanceof Error ? err.message : String(err));
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
/** "Freed 900 MB · 1.2 GB in use", or just the half that applies. */
export function freedAndInUse(freed: number, inUse: number): string {
  if (inUse <= 0) return `Freed ${formatSize(freed)}`;
  if (freed <= 0) return `${formatSize(inUse)} in use`;
  return `Freed ${formatSize(freed)} · ${formatSize(inUse)} in use`;
}
class CleanerState {
  categories = $state<CleanerCategory[]>([]);
  sizes = $state<Record<string, Measured>>({});
  readonly selected = new SvelteSet<string>(readJson<string[]>(KEY.selected, [], isStringArray).filter((id) => id !== DOWNLOADS));
  /** Emptied since the last scan: unchecked and switched off until then. */
  readonly cleaned = new SvelteSet<string>();
  /** What the last clean removed and left, per category, until the next scan. */
  outcome = $state<Record<string, Outcome>>({});
  phase = $state<Phase>("idle");
  /** False until the first scan finishes, so sizes stay blank instead of "0 B". */
  scanned = $state(false);
  /** Whether the current results include an approved administrator scan. */
  adminGranted = $state(false);
  progress = $state<CleanProgress | null>(null);
  /** Both passes are done and what is left is being measured again. */
  settling = $state(false);
  /** 0…1, eased by the animation loop; held full at the end. */
  barPercent = $state(0);
  barDone = $state(false);
  /** After a scan selected everything by itself, for the toolbar hint. */
  autoSelected = $state(false);
  lastCleaned = $state<number | null>(readJson<number | null>(KEY.lastCleaned, null, (v) => typeof v === "number"));
  error = $state<string | null>(null);
  /** The Downloads folder's last preview: what the scan found, and what stays. */
  downloads = $state<Preview | null>(null);
  #raf = 0;
  #barTarget = 0;
  /** When the running visual step is free to move on; see `step` in `clean`. */
  #paced: number | null = null;
  readonly canScanSystemFolders = $derived(this.scanned && !this.adminGranted && this.categories.some((category) => category.mayNeedAdmin));
  readonly busy = $derived(this.phase !== "idle");
  readonly scanning = $derived(this.phase === "scanning" || this.phase === "scanning-admin");
  readonly selectable = $derived(this.categories.filter((category) => this.canSelectCategory(category)));
  readonly bulkSelectable = $derived(this.selectable.filter(({ id }) => id !== DOWNLOADS));
  readonly selection = $derived(this.selectable.filter(({ id }) => this.selected.has(id)));
  readonly total = $derived(Object.values(this.sizes).reduce((sum, measured) => sum + measured.bytes, 0));
  readonly selectedBytes = $derived(this.selection.reduce((sum, { id }) => sum + this.sizes[id].bytes, 0));
  readonly allSelected = $derived(this.bulkSelectable.length > 0 && this.bulkSelectable.every(({ id }) => this.selected.has(id)));
  readonly allTicked = $derived(this.scanned && this.allSelected);
  readonly status = $derived.by(() => {
    if (this.phase === "scanning") return "Scanning…";
    if (this.phase === "scanning-admin") return "Scanning system folders…";
    if (this.phase === "cleaning") {
      if (this.settling) return "Checking what is left…";
      const p = this.progress;
      return p?.current ? `Cleaning ${p.current} (${p.step}/${p.total})` : "Cleaning…";
    }
    if (!this.scanned) return "Ready to scan";
    if (this.canScanSystemFolders) return "Scan completed · system folders not scanned";
    return this.autoSelected ? "Scan completed · available categories selected" : "Scan completed";
  });
  async load() {
    if (!isTauri()) return;
    try {
      // Downloads is measured and cleaned by its own commands, with the same card, tick and Clean button as the rest.
      this.categories = [...(await cleanerApi.categories()), DOWNLOADS_CATEGORY];
      const known = new Set(this.categories.map((c) => c.id)); // drop ids of an older build
      for (const id of [...this.selected]) if (!known.has(id)) this.selected.delete(id);
    } catch (err) {
      this.error = message(err);
    }
  }
  /** Re-reads the saved selection (after account sync replaced it). */
  reloadSelection() {
    const saved = readJson<string[]>(KEY.selected, [...this.selected], isStringArray);
    const downloadsSelected = this.isTicked(DOWNLOADS);
    this.selected.clear();
    for (const id of saved) {
      if (id !== DOWNLOADS && (!this.scanned || this.selectable.some((category) => category.id === id))) this.selected.add(id);
    }
    if (downloadsSelected) this.selected.add(DOWNLOADS);
  }
  isCleaned = (id: string) => this.cleaned.has(id);
  requiresAdmin(category: CleanerCategory) {
    return category.adminOnly && !this.adminGranted;
  }
  canSelectCategory(category: CleanerCategory) {
    return this.scanned && !!this.sizes[category.id] && !this.isCleaned(category.id) && !this.requiresAdmin(category)
      && (category.id !== DOWNLOADS || !!this.downloads?.items.length);
  }
  isTicked = (id: string) => this.selection.some((category) => category.id === id);
  toggle(id: string) {
    if (this.busy || !this.selectable.some((category) => category.id === id)) return;
    if (this.selected.has(id)) this.selected.delete(id); else this.selected.add(id);
    this.#persistSelection();
  }
  toggleAll() {
    if (this.busy || !this.bulkSelectable.length) return;
    const remove = this.allSelected;
    for (const { id } of this.bulkSelectable) {
      if (remove) this.selected.delete(id); else this.selected.add(id);
    }
    this.#persistSelection();
  }
  /** A regular scan starts fresh with the user folders and skips every system folder. */
  async scan() {
    if (this.busy) return;
    this.phase = "scanning";
    this.scanned = false;
    this.adminGranted = false;
    this.error = null;
    this.sizes = {};
    this.cleaned.clear();
    this.outcome = {};
    this.autoSelected = false;
    const downloadsPass = this.#measureDownloads();
    let scanSucceeded = false;
    try {
      await cleanerApi.scan((e) => { this.sizes[e.data.id] = e.data; });
      scanSucceeded = true;
    } catch (err) {
      this.error = message(err);
      toast.error(`Scan failed: ${message(err)}`);
    }
    // A failed category scan must still own its Downloads request until it finishes.
    await downloadsPass;
    this.scanned = scanSucceeded;
    if (scanSucceeded) this.#selectAllAfterScan();
    this.phase = "idle";
  }
  async #measureDownloads() {
    try {
      this.downloads = await cleanerApi.downloadsPreview();
      this.sizes[DOWNLOADS] = { bytes: this.downloads.total, files: this.downloads.items.length, locked: false };
    } catch (err) {
      this.downloads = null;
      toast.error(`Downloads folder: ${message(err)}`);
    }
  }
  /** Deletes the Downloads items the scan listed (each checked again). */
  async #cleanDownloads(): Promise<{ freed: number; skipped: number }> {
    const items = this.downloads?.items ?? [];
    const outcome = await cleanerApi.downloadsDelete(items.map((item) => item.path));
    const skippedBytes = items.reduce((sum, item) => sum + item.size, 0) - outcome.freed;
    this.#applyCleaned({ id: DOWNLOADS, bytes: outcome.freed, files: outcome.deleted, skipped: outcome.skipped.length, skippedBytes: Math.max(0, skippedBytes), adminSkipped: 0, adminSkippedBytes: 0 });
    return { freed: outcome.freed, skipped: outcome.skipped.length };
  }
  /** Drop saved ticks from outside the measured scope and select the available categories. */
  #selectAllAfterScan() {
    this.autoSelected = !this.allSelected && this.bulkSelectable.length > 0;
    this.selected.clear();
    for (const { id } of this.bulkSelectable) this.selected.add(id);
    this.#persistSelection();
  }
  /** Asks for administrator approval and adds system folders to the existing results. */
  async allowAdmin() {
    if (this.busy || !this.canScanSystemFolders) return;
    this.phase = "scanning-admin";
    const admin: Record<string, Measured> = {};
    try {
      await cleanerApi.scanElevated((e) => { admin[e.data.id] = e.data; });
      const previouslyAvailable = new Set(this.selectable.map(({ id }) => id));
      for (const [id, system] of Object.entries(admin)) {
        if (this.sizes[id]?.locked && this.cleaned.delete(id)) delete this.outcome[id];
        this.sizes[id] = addSystemMeasurement(this.sizes[id], system);
      }
      this.adminGranted = true;
      for (const { id } of this.selectable) {
        if (admin[id] && !previouslyAvailable.has(id)) this.selected.add(id);
      }
      this.#persistSelection();
    } catch (err) {
      const error = message(err);
      if (error === DECLINED) toast.info("System folders were left out: administrator approval was declined.");
      else toast.error(`System folders could not be measured: ${error}`);
    } finally {
      this.phase = "idle";
    }
  }
  async clean() {
    if (this.busy) return;
    const chosen = this.selection;
    if (!chosen.length) return;
    const withDownloads = chosen.some((c) => c.id === DOWNLOADS);
    const ids = chosen.map((c) => c.id).filter((id) => id !== DOWNLOADS);
    // What the scan measured for this selection: the number the user was shown.
    const selectedBefore = chosen.reduce((sum, c) => sum + (this.sizes[c.id]?.bytes ?? 0), 0);
    // Admin folders are included only when the scan's prompt was approved.
    const adminIds = chosen.filter((c) => c.mayNeedAdmin).map((c) => c.id);
    const withAdmin = this.adminGranted === true && adminIds.length > 0;
    const adminReady = withAdmin && (await cleanerApi.adminReady().catch(() => false)); // the scan's helper still runs: no second prompt
    const adminLine = withAdmin
      ? adminReady ? "\n\nThe system folders are emptied with the administrator approval from the scan." : "\n\nWindows will ask for administrator approval again to empty the system folders."
      : adminIds.length ? "\n\nSystem folders are skipped: administrator approval was not given during the scan." : "";
    const ok = await confirm({
      title: "Clean selected items?",
      message:
        `${chosen.map((c) => `• ${c.title}`).join("\n")}\n\nThis frees about ${formatSize(selectedBefore)} and cannot be undone.` + adminLine +
        (withDownloads
          ? `\n\nDownloads folder: ${this.downloads?.items.length ?? 0} item(s) are deleted PERMANENTLY. They do not go to the Recycle Bin and cannot be restored. Documents and anything modified in the last 7 days stay.`
          : ""),
      confirmLabel: withDownloads ? "Clean and delete permanently" : "Clean now",
      danger: true,
    });
    if (!ok || this.busy) return;
    // `passIds`, `passTitles` and `done` follow the pass that is running.
    let passIds = ids;
    let passTitles = chosen.filter((c) => c.id !== DOWNLOADS).map((c) => c.title);
    let done = 0;
    let pass: "user" | "administrator" = "user";
    let freed = 0, skipped = 0, inUse = 0;
    let adminNote: string | null = null;
    const step = async () => {
      done++;
      const nextId = passIds[done];
      this.progress = { pass, step: Math.min(done + 1, passIds.length), total: passIds.length, current: nextId ? (passTitles[done] ?? nextId) : "" };
      this.#setBarTarget(done / Math.max(1, passIds.length)); // one category done is one step of the bar
      const wait = STEP_MS - (Date.now() - (this.#paced ?? Date.now())); // held for the eye
      this.#paced = Date.now() + Math.max(0, wait);
      await sleep(Math.max(0, wait));
    };
    this.phase = "cleaning";
    this.#startBar();
    this.progress = { pass, step: 1, total: passIds.length, current: passTitles[0] ?? "" };
    try {
      if (withDownloads) {
        this.progress = { pass, step: 1, total: passIds.length + 1, current: "Downloads folder" };
        const result = await this.#cleanDownloads();
        freed += result.freed;
        skipped += result.skipped;
      }
      const empty = { freed: 0, files: 0, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: [] };
      const summary = ids.length
        ? await cleanerApi.clean(ids, (e) => { if (e.event === "progress") return; this.#applyCleaned(e.data); void step(); })
        : empty;
      freed += summary.freed;
      skipped += summary.skipped;
      // What the user pass left in the system folders is tried again below, when there is an administrator pass; without one it stays left.
      inUse += summary.skippedBytes - (withAdmin ? summary.adminSkippedBytes : 0);
      // The first pass already deleted files; a declined or failed UAC step must not hide that, so it is reported on its own.
      if (withAdmin) {
        pass = "administrator";
        passIds = adminIds;
        passTitles = chosen.filter((c) => adminIds.includes(c.id)).map((c) => c.title);
        done = 0;
        this.#setBarTarget(1);
        this.progress = { pass, step: 1, total: passIds.length, current: passTitles[0] ?? "" };
        try {
          const elevated = await cleanerApi.cleanElevated(adminIds, (e) => { if (e.event === "progress") return; this.#applyCleaned(e.data, true); void step(); });
          freed += elevated.freed;
          skipped = skipped - summary.adminSkipped + elevated.skipped; // only what that pass itself left
          inUse += elevated.skippedBytes;
        } catch (err) {
          adminNote = `The system folders were skipped: ${message(err)}`;
          inUse += summary.adminSkippedBytes; // not tried again: what the user pass left there is still there
        }
      }
      // What is really left, measured again, rather than the scan minus what was freed; the bytes of the skipped files are a lower bound.
      this.#setBarTarget(1);
      this.settling = true;
      if (ids.length) await this.#remeasure(ids, withAdmin && adminNote === null);
      if (withDownloads) await this.#measureDownloads();
      const all = withDownloads ? [...ids, DOWNLOADS] : ids;
      inUse = Math.max(inUse, this.#stillThere(all));
      for (const id of all) { this.cleaned.add(id); this.selected.delete(id); }
      this.#persistSelection();
      this.lastCleaned = Date.now();
      writeJson(KEY.lastCleaned, this.lastCleaned);
      await this.#holdBar();
      this.#announce(freed, inUse, skipped, selectedBefore);
    } catch (err) {
      toast.error(`Cleaning failed: ${message(err)}`);
    } finally {
      this.#stopBar();
      if (adminNote) toast.info(adminNote);
      this.phase = "idle";
      this.progress = null;
      this.settling = false;
      this.autoSelected = false;
      this.#paced = null;
    }
  }
  /** One toast with the whole outcome: what went and what stayed. */
  #announce(freed: number, inUse: number, skipped: number, selectedBefore: number) {
    if (inUse > 0) {
      const ofSelection = selectedBefore > freed ? ` of ${formatSize(selectedBefore)} selected` : "";
      toast.success(`${freedAndInUse(freed, inUse)}${ofSelection}.`);
      if (skipped > 0) toast.info(`${skipped.toLocaleString()} file${skipped === 1 ? " is" : "s are"} open in another program and were left alone.`);
      return;
    }
    toast.success(`Successfully freed up ${formatSize(freed)}!`);
  }
  /* Subtracts what a pass freed from a measured size, and keeps what it removed and left for the card. The administrator pass retries what the first one left in its folders, so its count replaces that part. */
  #applyCleaned(
    { id, bytes, files, skipped, skippedBytes, adminSkipped, adminSkippedBytes }:
      { id: string; bytes: number; files: number; skipped: number; skippedBytes: number; adminSkipped: number; adminSkippedBytes: number },
    retry = false,
  ) {
    const seen = this.outcome[id];
    this.outcome[id] = {
      freed: (seen?.freed ?? 0) + bytes,
      skipped: retry ? (seen?.skipped ?? 0) - (seen?.adminSkipped ?? 0) + skipped : (seen?.skipped ?? 0) + skipped,
      skippedBytes: retry ? (seen?.skippedBytes ?? 0) - (seen?.adminSkippedBytes ?? 0) + skippedBytes : (seen?.skippedBytes ?? 0) + skippedBytes,
      adminSkipped: retry ? 0 : (seen?.adminSkipped ?? 0) + adminSkipped,
      adminSkippedBytes: retry ? 0 : (seen?.adminSkippedBytes ?? 0) + adminSkippedBytes,
    };
    const before = this.sizes[id];
    if (!before) return;
    this.sizes[id] = { bytes: Math.max(0, before.bytes - bytes), files: Math.max(0, before.files - files), locked: before.locked };
  }
  /* Measures `ids` again after a clean; the system folders only through the helper that is still running, so this never asks for approval. */
  async #remeasure(ids: string[], admin: boolean) {
    const wanted = new Set(ids);
    const next: Record<string, Measured> = {};
    try {
      await cleanerApi.scan((e) => { if (wanted.has(e.data.id)) next[e.data.id] = { bytes: e.data.bytes, files: e.data.files, locked: e.data.locked }; });
      if (admin && (await cleanerApi.adminReady())) {
        await cleanerApi.scanElevated(({ data }) => {
          if (wanted.has(data.id)) next[data.id] = addSystemMeasurement(next[data.id], data);
        });
      }
    } catch {
      return;
    }
    for (const [id, measured] of Object.entries(next)) this.sizes[id] = measured;
  }
  /** What the freshly measured categories still hold, over the whole run. */
  #stillThere = (ids: string[]) => ids.reduce((sum, id) => sum + (this.sizes[id]?.bytes ?? 0), 0);
  // The bar: the work sets a target, a frame loop eases up to it.
  #startBar() {
    this.#stopBar();
    this.barPercent = 0;
    this.barDone = false;
    this.#barTarget = 0;
    this.#tick();
  }
  #setBarTarget(value: number) {
    this.#barTarget = Math.max(this.#barTarget, Math.min(1, value));
  }
  #tick() {
    this.#raf = requestAnimationFrame(() => {
      const gap = this.#barTarget - this.barPercent;
      this.barPercent = Math.abs(gap) < 0.001 ? this.#barTarget : this.barPercent + gap * 0.16;
      this.#tick();
    });
  }
  /** Rides the bar to the end and holds the full one for a moment. */
  async #holdBar() {
    this.#setBarTarget(1);
    const until = Date.now() + 900;
    while (this.barPercent < 0.995 && Date.now() < until) await sleep(16);
    this.barPercent = 1;
    this.barDone = true;
    await sleep(HOLD_MS);
  }
  #stopBar() {
    if (this.#raf) cancelAnimationFrame(this.#raf);
    this.#raf = 0;
  }
  #persistSelection() {
    writeJson(KEY.selected, [...this.selected].filter((id) => id !== DOWNLOADS));
  }
}
export const cleanerState = new CleanerState();
