// State of the System Cleaner page: fixed categories, measured sizes and selections. System folders are included through the explicit administrator scan.
import { isTauri } from "@tauri-apps/api/core";
import { SvelteSet } from "svelte/reactivity";
import { confirm } from "../../../lib/confirm.svelte";
import { readFlag, readJson, writeFlag, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { cleanerApi, formatSize, type CleanerCategory, type CleanEvent } from "./api";
import { playCleanComplete, prepareCleanAudio, stopCleanAudio } from "./clean-sound";
import { DOWNLOADS, DOWNLOADS_CATEGORY, type Preview } from "./downloads";
import { errorMessage as message } from "../../../lib/errors";
const KEY = { selected: "cleaner.selected", lastCleaned: "cleaner.lastCleaned", sound: "cleaner.sound" };
/** The backend's answer when the UAC prompt is declined. */
const DECLINED = "Administrator approval was declined.";
interface Measured { bytes: number; files: number; locked: boolean }
function addSystemMeasurement(user: Measured | undefined, system: Measured): Measured {
  return {
    bytes: (user?.bytes ?? 0) + system.bytes,
    files: (user?.files ?? 0) + system.files,
    locked: system.locked,
  };
}
/** What a clean pass removed from one category, and what it left. */
interface Outcome {
  freed: number;
  skipped: number;
  skippedBytes: number;
  inUse: number;
  inUseBytes: number;
  adminSkipped: number;
  adminSkippedBytes: number;
}
/** The step the running clean is on. */
interface CleanProgress { step: number; total: number; current: string }
type CategoryResult = Omit<Extract<CleanEvent, { event: "category" }>["data"], "locked">;
interface CleanedCategorySummary {
  id: string;
  title: string;
  freed: number;
  remaining: number;
  inUse: number;
  inUseBytes: number;
  otherSkipped: number;
  systemFoldersSkipped: boolean;
}
/** Result of a completed cleanup. */
export interface CelebrationSummary {
  freed: number;
  remaining: number;
  skipped: number;
  systemNote: string | null;
  items: CleanedCategorySummary[];
}
type Phase = "idle" | "scanning" | "scanning-all" | "scanning-admin" | "cleaning";
const isStringArray = (v: unknown) => Array.isArray(v) && v.every((x) => typeof x === "string");
class CleanerState {
  categories = $state<CleanerCategory[]>([]);
  sizes = $state<Record<string, Measured>>({});
  readonly selected = new SvelteSet<string>(readJson<string[]>(KEY.selected, [], isStringArray).filter((id) => id !== DOWNLOADS));
  /** Emptied since the last scan: unchecked and switched off until then. */
  readonly cleaned = new SvelteSet<string>();
  phase = $state<Phase>("idle");
  /** False until the first scan finishes, so sizes stay blank instead of "0 B". */
  scanned = $state(false);
  /** Whether the current results include an approved administrator scan. */
  adminGranted = $state(false);
  appElevated = $state<boolean | null>(null);
  progress = $state<CleanProgress | null>(null);
  /** Both passes are done and what is left is being measured again. */
  settling = $state(false);
  /** Fraction of completed work, including the final recheck. */
  barPercent = $state(0);
  /** After a scan selected everything by itself, for the toolbar hint. */
  autoSelected = $state(false);
  lastCleaned = $state<number | null>(readJson<number | null>(KEY.lastCleaned, null, (v) => typeof v === "number"));
  celebration = $state<CelebrationSummary | null>(null);
  #presentedResult: CelebrationSummary | null = null;
  soundEnabled = $state(readFlag(KEY.sound, true));
  error = $state<string | null>(null);
  /** The Downloads folder's last preview: what the scan found, and what stays. */
  downloads = $state<Preview | null>(null);
  readonly canScanSystemFolders = $derived(this.appElevated === false && !this.adminGranted && this.categories.some((category) => category.mayNeedAdmin));
  readonly busy = $derived(this.phase !== "idle");

  dismissCelebration() {
    this.celebration = null;
    this.#presentedResult = null;
    stopCleanAudio();
  }
  presentResult(result: CelebrationSummary) {
    if (this.#presentedResult === result) return;
    this.#presentedResult = result;
    if (result.freed && this.soundEnabled) playCleanComplete();
  }
  toggleSound() {
    this.soundEnabled = !this.soundEnabled;
    writeFlag(KEY.sound, this.soundEnabled);
    if (!this.soundEnabled) stopCleanAudio();
    else if (this.phase === "cleaning") prepareCleanAudio();
  }
  readonly scanning = $derived(this.phase === "scanning" || this.phase === "scanning-all" || this.phase === "scanning-admin");
  readonly selectable = $derived(this.categories.filter((category) => this.canSelectCategory(category)));
  readonly bulkSelectable = $derived(this.selectable.filter(({ id }) => id !== DOWNLOADS));
  readonly selection = $derived(this.selectable.filter(({ id }) => this.selected.has(id)));
  readonly total = $derived(Object.values(this.sizes).reduce((sum, measured) => sum + measured.bytes, 0));
  readonly selectedBytes = $derived(this.selection.reduce((sum, { id }) => sum + this.sizes[id].bytes, 0));
  readonly allSelected = $derived(this.bulkSelectable.length > 0 && this.bulkSelectable.every(({ id }) => this.selected.has(id)));
  readonly allTicked = $derived(this.scanned && this.allSelected);
  readonly status = $derived.by(() => {
    if (this.phase === "scanning") return "Scanning…";
    if (this.phase === "scanning-all") return "Scanning user and system folders…";
    if (this.phase === "scanning-admin") return "Scanning system folders…";
    if (this.phase === "cleaning") {
      if (this.settling) return "Checking what is left…";
      const p = this.progress;
      return p?.current ? `Cleaning ${p.current} (${p.step}/${p.total})` : "Cleaning…";
    }
    if (!this.scanned) return "Ready to scan";
    if (!this.adminGranted && this.categories.some((category) => category.mayNeedAdmin)) return "Scan completed · system folders not scanned";
    return this.autoSelected ? "Scan completed · available categories selected" : "Scan completed";
  });
  async load() {
    if (!isTauri()) return;
    try {
      // Downloads is measured and cleaned by its own commands, with the same card, tick and Clean button as the rest.
      const [categories, elevated] = await Promise.all([cleanerApi.categories(), cleanerApi.isElevated()]);
      this.appElevated = elevated;
      this.categories = [...categories, DOWNLOADS_CATEGORY];
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
  /** A fresh scan includes system folders when requested or when MYLE already runs elevated. */
  async scan(includeSystem = this.appElevated === true) {
    if (this.busy) return;
    this.phase = includeSystem ? "scanning-all" : "scanning";
    this.scanned = false;
    this.adminGranted = false;
    this.error = null;
    this.sizes = {};
    this.cleaned.clear();
    this.autoSelected = false;
    const systemPass = includeSystem ? this.#measureSystemFolders() : undefined;
    const downloadsPass = this.#measureDownloads();
    let scanSucceeded = false;
    try {
      await cleanerApi.scan((e) => { this.sizes[e.data.id] = e.data; });
      scanSucceeded = true;
    } catch (err) {
      this.error = message(err);
      toast.error(`Scan failed: ${message(err)}`);
    }
    // Wait for each pass before releasing the scan. Merge system sizes after the user measurements, regardless of completion order.
    const [, admin] = await Promise.all([downloadsPass, systemPass]);
    if (scanSucceeded && admin) this.#addSystemFolders(admin);
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
  async #cleanDownloads(): Promise<CategoryResult> {
    const items = this.downloads?.items ?? [];
    const outcome = await cleanerApi.downloadsDelete(items.map((item) => item.path));
    const skippedBytes = items.reduce((sum, item) => sum + item.size, 0) - outcome.freed;
    return {
      id: DOWNLOADS,
      bytes: outcome.freed,
      files: outcome.deleted,
      skipped: outcome.skipped.length,
      skippedBytes: Math.max(0, skippedBytes),
      adminSkipped: 0,
      adminSkippedBytes: 0,
    };
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
    if (!this.scanned) return this.scan(true);
    this.phase = "scanning-admin";
    const previouslyAvailable = new Set(this.selectable.map(({ id }) => id));
    const added = await this.#measureSystemFolders();
    if (added) {
      this.#addSystemFolders(added);
      for (const { id } of this.bulkSelectable) {
        if (added[id] && !previouslyAvailable.has(id)) this.selected.add(id);
      }
      this.#persistSelection();
    }
    this.phase = "idle";
  }

  async #measureSystemFolders(): Promise<Record<string, Measured> | undefined> {
    const admin: Record<string, Measured> = {};
    try {
      await cleanerApi.scanElevated((e) => { admin[e.data.id] = e.data; });
      return admin;
    } catch (err) {
      const error = message(err);
      if (error === DECLINED) toast.info("System folders were left out: administrator approval was declined.");
      else toast.error(`System folders could not be measured: ${error}`);
    }
  }

  #addSystemFolders(admin: Record<string, Measured>) {
    for (const [id, system] of Object.entries(admin)) {
      if (this.sizes[id]?.locked) this.cleaned.delete(id);
      this.sizes[id] = addSystemMeasurement(this.sizes[id], system);
    }
    this.adminGranted = true;
  }
  async clean() {
    if (this.busy) return;
    const chosen = this.selection;
    if (!chosen.length) return;
    const withDownloads = chosen.some((c) => c.id === DOWNLOADS);
    const userSteps = chosen.filter((c) => c.id !== DOWNLOADS);
    const ids = userSteps.map(({ id }) => id);
    // What the scan measured for this selection: the number the user was shown.
    const selectedBefore = chosen.reduce((sum, c) => sum + (this.sizes[c.id]?.bytes ?? 0), 0);
    // Admin folders are included only when the scan's prompt was approved.
    const systemSteps = chosen.filter((c) => c.mayNeedAdmin);
    const withAdmin = this.adminGranted && systemSteps.length > 0;
    const adminReady = withAdmin && (await cleanerApi.adminReady().catch(() => false)); // the scan's helper still runs: no second prompt
    const adminLine = withAdmin
      ? this.appElevated || adminReady ? "System folders are included." : "Windows will ask for administrator approval."
      : systemSteps.length ? "System folders will be skipped." : "";
    const downloadCount = withDownloads ? (this.downloads?.items.length ?? 0) : 0;
    const downloadLine = downloadCount
      ? `${downloadCount.toLocaleString()} Downloads ${downloadCount === 1 ? "item" : "items"} will be permanently deleted.`
      : "";
    const ok = await confirm({
      title: withDownloads && !ids.length ? "Clean Downloads?" : "Clean selected items?",
      message: [
        `Free about ${formatSize(selectedBefore)}. This cannot be undone.`,
        downloadLine,
        adminLine,
      ].filter(Boolean).join("\n"),
      confirmLabel: "Clean now",
      danger: true,
    });
    if (!ok || this.busy) return;
    const outcomes = new Map<string, Outcome>();
    const downloadSteps = Number(withDownloads);
    const systemStart = downloadSteps + userSteps.length;
    const total = systemStart + (withAdmin ? systemSteps.length : 0) + 1; // The last step checks what remains.
    let freed = 0, skipped = 0, remaining = 0;
    let adminNote: string | null = null;
    const report = (completed: number, current = "") => {
      this.progress = { step: Math.min(completed + 1, total), total, current };
      this.barPercent = completed / total;
    };
    this.dismissCelebration();
    report(0, withDownloads ? "Downloads folder" : userSteps[0]?.title);
    this.phase = "cleaning";
    if (this.soundEnabled) prepareCleanAudio();
    try {
      if (withDownloads) {
        const result = await this.#cleanDownloads();
        this.#applyCleaned(outcomes, result);
        freed += result.bytes;
        skipped += result.skipped;
        report(downloadSteps, userSteps[0]?.title);
      }
      const empty = { freed: 0, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0 };
      let userDone = 0;
      const summary = ids.length
        ? await cleanerApi.clean(ids, (e) => {
          if (e.event === "progress") return;
          this.#applyCleaned(outcomes, e.data);
          userDone++;
          report(downloadSteps + userDone, userSteps[userDone]?.title);
        })
        : empty;
      freed += summary.freed;
      skipped += summary.skipped;
      // What the user pass left in the system folders is tried again below, when there is an administrator pass; without one it stays left.
      remaining += summary.skippedBytes - (withAdmin ? summary.adminSkippedBytes : 0);
      // The first pass already deleted files; a declined or failed UAC step must not hide that, so it is reported on its own.
      if (withAdmin) {
        let systemDone = 0;
        report(systemStart, systemSteps[0]?.title);
        try {
          const elevated = await cleanerApi.cleanElevated(systemSteps.map(({ id }) => id), (e) => {
            if (e.event === "progress") return;
            this.#applyCleaned(outcomes, e.data, true);
            systemDone++;
            report(systemStart + systemDone, systemSteps[systemDone]?.title);
          });
          freed += elevated.freed;
          skipped = skipped - summary.adminSkipped + elevated.skipped; // only what that pass itself left
          remaining += elevated.skippedBytes;
        } catch (err) {
          adminNote = `The system folders were skipped: ${message(err)}`;
          remaining += summary.adminSkippedBytes;
        }
      }
      // What is really left, measured again, rather than the scan minus what was freed; the bytes of the skipped files are a lower bound.
      report(total - 1);
      this.settling = true;
      if (ids.length) await this.#remeasure(ids, withAdmin && adminNote === null);
      if (withDownloads) await this.#measureDownloads();
      for (const { id } of chosen) {
        this.cleaned.add(id);
        this.selected.delete(id);
      }
      this.#persistSelection();
      this.lastCleaned = Date.now();
      writeJson(KEY.lastCleaned, this.lastCleaned);
      this.barPercent = 1;
      const items: CleanedCategorySummary[] = [];
      for (const { id, title, mayNeedAdmin } of chosen) {
        const outcome = outcomes.get(id);
        const freed = outcome?.freed ?? 0;
        const remaining = Math.max(this.sizes[id]?.bytes ?? 0, outcome?.skippedBytes ?? 0);
        const inUse = outcome?.inUse ?? 0;
        const inUseBytes = outcome?.inUseBytes ?? 0;
        const otherSkipped = (outcome?.skipped ?? 0) - inUse - (outcome?.adminSkipped ?? 0);
        const systemFoldersSkipped = mayNeedAdmin && (!withAdmin || adminNote !== null);
        if (freed || remaining || inUse || otherSkipped || systemFoldersSkipped) {
          items.push({ id, title, freed, remaining, inUse, inUseBytes, otherSkipped, systemFoldersSkipped });
        }
      }
      items.sort((a, b) => b.freed - a.freed || b.remaining - a.remaining);
      remaining = Math.max(remaining, items.reduce((sum, item) => sum + item.remaining, 0));

      this.celebration = {
        freed,
        remaining,
        skipped,
        systemNote: adminNote ?? (!withAdmin && systemSteps.length ? "System folders were outside this cleanup." : null),
        items,
      };
    } catch (err) {
      stopCleanAudio();
      toast.error(`Cleaning failed: ${message(err)}`);
    } finally {
      this.phase = "idle";
      this.progress = null;
      this.settling = false;
      this.autoSelected = false;
    }
  }
  /** Updates the card's size and collects the data needed for the result popup. */
  #applyCleaned(
    outcomes: Map<string, Outcome>,
    { id, bytes, files, skipped, skippedBytes, inUse = 0, inUseBytes = 0, adminSkipped, adminSkippedBytes }: CategoryResult,
    retry = false,
  ) {
    const seen = outcomes.get(id);
    // An administrator pass replaces the skips it retried.
    outcomes.set(id, {
      freed: (seen?.freed ?? 0) + bytes,
      skipped: retry ? (seen?.skipped ?? 0) - (seen?.adminSkipped ?? 0) + skipped : (seen?.skipped ?? 0) + skipped,
      skippedBytes: retry ? (seen?.skippedBytes ?? 0) - (seen?.adminSkippedBytes ?? 0) + skippedBytes : (seen?.skippedBytes ?? 0) + skippedBytes,
      inUse: (seen?.inUse ?? 0) + inUse,
      inUseBytes: (seen?.inUseBytes ?? 0) + inUseBytes,
      adminSkipped: retry ? 0 : (seen?.adminSkipped ?? 0) + adminSkipped,
      adminSkippedBytes: retry ? 0 : (seen?.adminSkippedBytes ?? 0) + adminSkippedBytes,
    });
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
  #persistSelection() {
    writeJson(KEY.selected, [...this.selected].filter((id) => id !== DOWNLOADS));
  }
}
export const cleanerState = new CleanerState();
