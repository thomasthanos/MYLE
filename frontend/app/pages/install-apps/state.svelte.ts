// State of the Install Apps page. A module-level singleton, so everything
// survives leaving and returning to the page; the user's choices are also
// persisted to localStorage.
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SvelteSet } from "svelte/reactivity";
import { confirm } from "../../../lib/confirm.svelte";
import { nav } from "../../../lib/nav.svelte";
import { operationGate } from "../../../lib/operation-gate.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { api, type JobEvent, type JobOutcome, type Mode, type PackageLinks, type Stage } from "./api";
import { CATEGORIES, categorize, isCategory, type Category } from "./categories";
import catalog from "./data/apps.json";
export type Source = "winget" | "custom" | "catalog";
export interface AppEntry {
  id: string;
  name: string;
  source: Source;
  category: Category;
  site?: string;
  /** Full icon URL, used instead of the favicon lookup. */
  icon?: string;
  /** Domain for the favicon lookup when `site`'s favicon is the wrong one. */
  iconDomain?: string;
  selfUpdating?: boolean;
  activateLabel?: string;
  /** Install also runs the activation (e.g. Discord mods patch Discord). */
  activateAfterInstall?: boolean;
  /** Apps that cannot be active together with this one. */
  conflicts?: string[];
  /** Latest version, for catalog search results. */
  version?: string;
}
export type Status = "installed" | "update" | "missing" | "unknown";
export type View = "grid" | "list";
export type Filter = "all" | "installed" | "updates" | "missing";
export type Sort = "category" | "az" | "za" | "status";
export interface AppStatus {
  installed: boolean;
  version?: string;
  available?: string;
  activated?: boolean;
}
export interface Job {
  mode: Mode;
  phase: "queued" | Stage;
  progress: number | null;
  note?: string;
}
export interface Pack {
  id: string;
  name: string;
  description: string;
  apps: string[];
}
const KEY = {
  view: "myle.apps.view",
  filter: "myle.apps.filter",
  sort: "myle.apps.sort",
  selected: "myle.apps.selected",
  pinned: "myle.apps.pinned",
  statuses: "myle.apps.statuses",
  links: "myle.apps.links",
};
/** Parallel `winget show` lookups for catalog results' links. */
const LINK_CONCURRENCY = 3;
const SEARCH_DEBOUNCE_MS = 250;
/** A visit to the page re-reads what is installed after this long. */
const RECHECK_AFTER_MS = 2 * 60 * 1000;
/** Coming back to the window re-reads it after this long: apps installed or
 *  removed outside the app show up without a manual refresh. */
const FOCUS_RECHECK_MS = 45 * 1000;
/** Installers that hand off to a second process finish after winget returns. */
const LATE_RECHECK_MS = 20 * 1000;
const STATUS_ORDER: Record<Status, number> = { update: 0, installed: 1, missing: 2, unknown: 3 };
const key = (id: string) => id.toLowerCase();
const oneOf = <T extends string>(...values: T[]) => (v: unknown) => values.includes(v as T);
const isStringArray = (v: unknown) => Array.isArray(v) && v.every((x) => typeof x === "string");
const isEntryArray = (v: unknown) =>
  Array.isArray(v) && v.every((x) => x && typeof x.id === "string" && typeof x.name === "string");
const isRecord = (v: unknown) => typeof v === "object" && v !== null && !Array.isArray(v);
function fromCatalog(a: (typeof catalog.apps)[number]): AppEntry {
  const category = "category" in a && isCategory(a.category) ? a.category : categorize(a.id);
  return {
    id: a.id,
    name: a.name,
    source: "winget",
    category,
    site: a.site,
    icon: "icon" in a ? a.icon : undefined,
    iconDomain: "iconDomain" in a ? a.iconDomain : undefined,
    selfUpdating: "selfUpdating" in a ? a.selfUpdating : undefined,
  };
}
function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
class InstallAppsState {
  readonly packs: Pack[] = catalog.packs;
  readonly curated: AppEntry[] = catalog.apps.map(fromCatalog);
  custom = $state<AppEntry[]>([]);
  // Persisted choices
  view = $state<View>(readJson(KEY.view, "grid", oneOf("grid", "list")));
  filter = $state<Filter>(readJson(KEY.filter, "all", oneOf("all", "installed", "updates", "missing")));
  sort = $state<Sort>(readJson(KEY.sort, "category", oneOf("category", "az", "za", "status")));
  readonly selected = new SvelteSet<string>(readJson<string[]>(KEY.selected, [], isStringArray));
  /** Checked catalog search results: kept visible whatever the search. */
  pinned = $state<AppEntry[]>(readJson(KEY.pinned, [], isEntryArray));
  activePack = $state<string | null>(null);
  // Search
  query = $state("");
  appliedQuery = $state("");
  catalogResults = $state<AppEntry[]>([]);
  catalogLoading = $state(false);
  #searchTimer: ReturnType<typeof setTimeout> | undefined;
  #searchSeq = 0;
  // Installed state (cached, then refreshed)
  statuses = $state<Record<string, AppStatus>>(readJson(KEY.statuses, {}, isRecord));
  checking = $state(false);
  checkedOnce = $state(false);
  wingetError = $state<string | null>(null);
  #lastCheck = 0;
  #initialized = false;
  // Links (homepage + favicon domain) of catalog results, looked up lazily
  #links: Record<string, PackageLinks> = readJson(KEY.links, {}, isRecord);
  #linkPending = new Set<string>();
  #linkQueue: string[] = [];
  #linkActive = 0;
  /** Finished in this visit: shown whatever the status filter, so an app
   *  installed from "Not installed" does not vanish the moment it is done. */
  readonly recent = new SvelteSet<string>();
  #watching = false;
  #lateCheck: ReturnType<typeof setTimeout> | undefined;
  // Jobs
  jobs = $state<Record<string, Job>>({});
  #queue: { entry: AppEntry; mode: Mode }[] = [];
  #current: string | null = null;
  #running = false;
  /** Work that holds the shared installer slot besides the queue itself. */
  #activations = 0;
  #selecting = false;
  // ---------------------------------------------------------------- derived
  readonly all = $derived([...this.curated, ...this.custom]);
  /** Local apps matching the pack and the search (before the status filter). */
  readonly scoped = $derived.by(() => {
    let list = this.all;
    const pack = this.packs.find((p) => p.id === this.activePack);
    if (pack) {
      const ids = new Set(pack.apps.map(key));
      list = list.filter((a) => ids.has(key(a.id)));
    }
    const q = this.appliedQuery.trim().toLowerCase();
    if (q) list = list.filter((a) => a.name.toLowerCase().includes(q) || a.id.toLowerCase().includes(q));
    return list;
  });
  readonly counts = $derived.by(() => {
    const c = { all: this.scoped.length, installed: 0, updates: 0, missing: 0 };
    for (const app of this.scoped) {
      const s = this.statusOf(app);
      if (s === "installed") c.installed++;
      else if (s === "update") {
        c.updates++;
        c.installed++;
      } else if (s === "missing") c.missing++;
    }
    return c;
  });
  readonly visible = $derived.by(() => {
    const list = this.scoped.filter((a) => this.matchesFilter(a));
    const byName = (a: AppEntry, b: AppEntry) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    switch (this.sort) {
      case "za":
        return list.sort((a, b) => byName(b, a));
      case "status":
        return list.sort((a, b) => STATUS_ORDER[this.statusOf(a)] - STATUS_ORDER[this.statusOf(b)] || byName(a, b));
      default:
        return list.sort(byName);
    }
  });
  /** Sections to render: category headings, or one flat list. */
  readonly groups = $derived.by(() => {
    if (this.sort !== "category") return [{ title: null as Category | null, apps: this.visible }];
    return CATEGORIES.map((title) => ({ title, apps: this.visible.filter((a) => a.category === title) })).filter(
      (g) => g.apps.length > 0,
    );
  });
  /** Pinned catalog apps that pass the status filter. */
  readonly visiblePinned = $derived(this.pinned.filter((a) => this.matchesFilter(a)));
  /** The "More from catalog" section shows when a search matches nothing local. */
  readonly showCatalog = $derived(this.appliedQuery.trim().length >= 2 && this.scoped.length === 0 && !this.activePack);
  readonly shownCount = $derived(
    this.visible.length + this.visiblePinned.length + (this.showCatalog ? this.catalogResults.length : 0),
  );
  readonly busy = $derived(Object.keys(this.jobs).length > 0);
  /** A Spotify Hub workflow owns the shared installer slot. */
  readonly externallyLocked = $derived(operationGate.lockedFor("install-apps"));
  // ---------------------------------------------------------------- status
  statusOf(app: AppEntry): Status {
    const s = this.statuses[key(app.id)];
    if (!s) return this.checkedOnce ? "missing" : "unknown";
    if (!s.installed) return "missing";
    if (s.available && !app.selfUpdating) return "update";
    return "installed";
  }
  statusInfo(app: AppEntry): AppStatus | undefined {
    return this.statuses[key(app.id)];
  }
  private matchesFilter(app: AppEntry): boolean {
    if (this.recent.has(key(app.id))) return true;
    const s = this.statusOf(app);
    switch (this.filter) {
      case "installed":
        return s === "installed" || s === "update";
      case "updates":
        return s === "update";
      case "missing":
        return s === "missing";
      default:
        return true;
    }
  }
  /** First visit: load the custom catalog and check what is installed. */
  async init() {
    if (!isTauri()) return;
    if (!this.#initialized) {
      this.#initialized = true;
      try {
        this.custom = (await api.customCatalog()).map((c) => ({
          id: c.id,
          name: c.name,
          source: "custom",
          category: isCategory(c.category) ? c.category : categorize(c.id),
          site: c.site ?? undefined,
          icon: c.icon ?? undefined,
          selfUpdating: c.selfUpdating,
          conflicts: c.conflicts,
          activateLabel: c.activateLabel ?? undefined,
          activateAfterInstall: c.activateAfterInstall,
        }));
      } catch (err) {
        toast.error(`Could not load the custom apps: ${message(err)}`);
      }
    }
    this.#watchFocus();
    if (Date.now() - this.#lastCheck > RECHECK_AFTER_MS) await this.checkInstalled();
  }
  /** Re-reads installed apps when the user comes back to the window. */
  #watchFocus() {
    if (this.#watching) return;
    this.#watching = true;
    const recheck = () => {
      if (document.visibilityState !== "visible" || nav.current !== "install-apps") return;
      if (this.busy || this.checking || Date.now() - this.#lastCheck < FOCUS_RECHECK_MS) return;
      void this.checkInstalled();
    };
    window.addEventListener("focus", recheck);
    document.addEventListener("visibilitychange", recheck);
  }
  /** Leaving the page (or changing what is shown) ends the grace period of
   *  apps that just finished, and unpins checked-out catalog apps with it. */
  forgetRecent() {
    if (!this.recent.size) return;
    this.recent.clear();
    const pinned = this.pinned.filter((p) => this.selected.has(p.id));
    if (pinned.length !== this.pinned.length) {
      this.pinned = pinned;
      this.persistSelection();
    }
  }
  async checkInstalled(announce = false) {
    if (!isTauri() || this.checking) return;
    this.checking = true;
    try {
      const report = await api.installed();
      const next: Record<string, AppStatus> = {};
      for (const p of report.winget) {
        next[key(p.id)] = { installed: true, version: p.version, available: p.available ?? undefined };
      }
      for (const c of report.custom) {
        next[key(c.id)] = {
          installed: c.installed,
          version: c.version ?? undefined,
          available: c.available ?? undefined,
          activated: c.activated,
        };
      }
      this.statuses = next;
      this.checkedOnce = true;
      this.#lastCheck = Date.now();
      writeJson(KEY.statuses, next);
      // Shown on the page itself, not as a toast: without winget (LTSC,
      // Windows Sandbox) an error that stays would greet every start, on
      // whatever page is open.
      this.wingetError = report.wingetError;
      if (announce) toast.info("Installed apps refreshed.");
    } catch (err) {
      toast.error(`Could not check installed apps: ${message(err)}`);
    } finally {
      this.checking = false;
    }
  }
  // ---------------------------------------------------------------- choices
  /** Re-reads the saved choices (after account sync replaced them). */
  reloadChoices() {
    this.view = readJson(KEY.view, this.view, oneOf("grid", "list"));
    this.filter = readJson(KEY.filter, this.filter, oneOf("all", "installed", "updates", "missing"));
    this.sort = readJson(KEY.sort, this.sort, oneOf("category", "az", "za", "status"));
    const selected = readJson<string[]>(KEY.selected, [...this.selected], isStringArray);
    this.selected.clear();
    for (const id of selected) this.selected.add(id);
    this.pinned = readJson(KEY.pinned, this.pinned, isEntryArray);
  }
  setView(view: View) {
    this.view = view;
    writeJson(KEY.view, view);
  }
  setFilter(filter: Filter) {
    this.forgetRecent();
    this.filter = filter;
    writeJson(KEY.filter, filter);
  }
  setSort(sort: Sort) {
    this.sort = sort;
    writeJson(KEY.sort, sort);
  }
  /** Select and reveal one curated app before another page hands off here. */
  prepareInstall(id: string): boolean {
    const app = this.all.find((entry) => key(entry.id) === key(id));
    if (!app) return false;
    clearTimeout(this.#searchTimer);
    // A targeted handoff must not accidentally carry unrelated apps into the
    // next Install click. Filter to the requested app so it is visible at the
    // restored top-of-page scroll position as well as selected.
    this.query = app.name;
    this.appliedQuery = app.name;
    this.catalogResults = [];
    this.activePack = null;
    this.filter = "all";
    this.selected.clear();
    this.pinned = [];
    this.selected.add(app.id);
    writeJson(KEY.filter, this.filter);
    this.persistSelection();
    return true;
  }
  /** Shows only the pack's apps and checks them all. */
  applyPack(id: string | null) {
    this.forgetRecent();
    this.activePack = id;
    const pack = this.packs.find((p) => p.id === id);
    if (!pack) return;
    for (const appId of pack.apps) {
      const app = this.all.find((a) => key(a.id) === key(appId));
      if (app && this.statusOf(app) !== "installed") this.selected.add(app.id);
    }
    this.persistSelection();
  }
  isSelected(app: AppEntry) {
    return this.selected.has(app.id);
  }
  toggle(app: AppEntry) {
    if (this.selected.has(app.id)) {
      this.selected.delete(app.id);
      if (app.source === "catalog") this.pinned = this.pinned.filter((p) => p.id !== app.id);
    } else {
      this.selected.add(app.id);
      if (app.source === "catalog" && !this.pinned.some((p) => p.id === app.id)) this.pinned.push(app);
    }
    this.persistSelection();
  }
  uncheckAll() {
    this.selected.clear();
    this.pinned = [];
    this.persistSelection();
  }
  private persistSelection() {
    writeJson(KEY.selected, [...this.selected]);
    writeJson(KEY.pinned, $state.snapshot(this.pinned));
  }
  // ---------------------------------------------------------------- search
  setQuery(query: string) {
    this.query = query;
    clearTimeout(this.#searchTimer);
    this.#searchTimer = setTimeout(() => {
      this.forgetRecent();
      this.appliedQuery = query;
      void this.searchCatalog(query);
    }, SEARCH_DEBOUNCE_MS);
  }
  /** Searches winget only when nothing local matches. */
  private async searchCatalog(query: string) {
    const seq = ++this.#searchSeq;
    const q = query.trim();
    const hasLocal = this.all.some(
      (a) => a.name.toLowerCase().includes(q.toLowerCase()) || a.id.toLowerCase().includes(q.toLowerCase()),
    );
    if (q.length < 2 || hasLocal || !isTauri()) {
      this.catalogResults = [];
      this.catalogLoading = false;
      return;
    }
    this.catalogLoading = true;
    try {
      const hits = await api.search(q);
      if (seq !== this.#searchSeq) return;
      const known = new Set([...this.all, ...this.pinned].map((a) => key(a.id)));
      this.catalogResults = hits
        .filter((h) => !known.has(key(h.id)))
        .map((h) => ({ id: h.id, name: h.name, source: "catalog", category: categorize(h.id), version: h.version }));
    } catch (err) {
      if (seq === this.#searchSeq) toast.error(`Catalog search failed: ${message(err)}`);
    } finally {
      if (seq === this.#searchSeq) this.catalogLoading = false;
    }
  }
  // ---------------------------------------------------------------- catalog links
  /** Gives a catalog result its website and favicon (called when its card
   *  scrolls into view). Results are cached across sessions. */
  resolveLinks(app: AppEntry) {
    if (app.source !== "catalog" || app.site || !isTauri()) return;
    const k = key(app.id);
    const cached = this.#links[k];
    if (cached) {
      this.applyLinks(k, cached);
      return;
    }
    if (this.#linkPending.has(k)) return;
    this.#linkPending.add(k);
    this.#linkQueue.push(app.id);
    this.drainLinks();
  }
  private drainLinks() {
    while (this.#linkActive < LINK_CONCURRENCY && this.#linkQueue.length) {
      const id = this.#linkQueue.shift()!;
      this.#linkActive++;
      void this.fetchLinks(id).finally(() => {
        this.#linkActive--;
        this.drainLinks();
      });
    }
  }
  private async fetchLinks(id: string): Promise<PackageLinks> {
    const k = key(id);
    let links: PackageLinks = { homepage: null, iconDomain: null };
    try {
      links = await api.packageLinks(id);
      this.#links[k] = links;
      writeJson(KEY.links, this.#links);
    } catch {
      // Leave it uncached so a later view can retry.
    } finally {
      this.#linkPending.delete(k);
    }
    this.applyLinks(k, links);
    return links;
  }
  private applyLinks(k: string, links: PackageLinks) {
    let pinnedChanged = false;
    for (const list of [this.catalogResults, this.pinned]) {
      for (const app of list) {
        if (key(app.id) !== k || app.site) continue;
        app.site = links.homepage?.replace(/^https?:\/\//, "") ?? undefined;
        app.iconDomain = links.iconDomain ?? undefined;
        if (list === this.pinned) pinnedChanged = true;
      }
    }
    if (pinnedChanged) this.persistSelection();
  }
  // ---------------------------------------------------------------- actions
  async openSite(app: AppEntry) {
    try {
      let url = app.site ? `https://${app.site}` : null;
      if (!url && app.source === "catalog") url = (this.#links[key(app.id)] ?? (await this.fetchLinks(app.id))).homepage;
      if (url) await openUrl(url);
      else toast.info(`${app.name} has no website listed.`);
    } catch (err) {
      toast.error(`Could not open the website: ${message(err)}`);
    }
  }
  async activate(app: AppEntry) {
    const k = key(app.id);
    if (this.jobs[k] || !operationGate.begin("install-apps")) return;
    const label = app.activateLabel ?? "Activate";
    this.jobs[k] = { mode: "install", phase: "installing", progress: null, note: `${label}…` };
    this.#activations++;
    try {
      await api.activate(app.id);
      const s = this.statuses[k];
      if (s) s.activated = true;
      toast.success(`${app.name}: ${label} done.`);
    } catch (err) {
      toast.error(`${app.name}: ${message(err)}`);
    } finally {
      delete this.jobs[k];
      this.#activations--;
      this.#releaseGate();
    }
  }
  /** Lets other pages have the installer slot once nothing here needs it:
   *  an activation finishing must not free it under a running install. */
  #releaseGate() {
    if (this.#running || this.#selecting || this.#activations > 0 || this.#queue.length) return;
    operationGate.end("install-apps");
  }
  /** Installs the checked apps; ones with an update are updated instead. */
  async installSelected() {
    if (this.#selecting) return;
    if (!operationGate.begin("install-apps")) {
      toast.info("Finish the Spotify Hub action before installing apps.");
      return;
    }
    // Conflict questions are awaited in the loop: a batch that finishes in
    // the meantime must not free the slot before this one is queued.
    this.#selecting = true;
    try {
      await this.#queueSelected();
    } finally {
      this.#selecting = false;
      this.#releaseGate();
    }
  }
  async #queueSelected() {
    const byId = new Map([...this.all, ...this.pinned].map((a) => [a.id, a]));
    let skipped = 0;
    for (const id of [...this.selected]) {
      const app = byId.get(id);
      if (!app) continue;
      const status = this.statusOf(app);
      if (status === "installed") {
        skipped++;
        continue;
      }
      const mode = status === "update" ? "upgrade" : "install";
      if (mode === "install" && !(await this.confirmConflicts(app))) continue;
      this.enqueue(app, mode);
    }
    if (skipped) toast.info(`${skipped} selected ${skipped === 1 ? "app is" : "apps are"} already installed.`);
  }
  /** Warns before installing an app that cannot coexist with an installed one. */
  private async confirmConflicts(app: AppEntry): Promise<boolean> {
    const active = (app.conflicts ?? [])
      .map((id) => this.all.find((a) => a.id === id))
      .filter((other): other is AppEntry => !!other && ["installed", "update"].includes(this.statusOf(other)));
    if (!active.length) return true;
    const names = active.map((a) => a.name).join(" and ");
    return confirm({
      title: `${app.name} and ${names} don't mix`,
      message:
        `${names} is already set up. ${app.name} and ${names} modify the same files, ` +
        `so having both can break things until one of them is removed.\n\nInstall ${app.name} anyway?`,
      confirmLabel: `Install ${app.name}`,
      danger: true,
    });
  }
  /** Drops everything queued and stops the running job. */
  cancelAll() {
    for (const { entry } of this.#queue) delete this.jobs[key(entry.id)];
    this.#queue = [];
    if (this.#current) void api.cancel(this.#current);
  }
  cancel(app: AppEntry) {
    const k = key(app.id);
    if (this.#current === app.id) {
      void api.cancel(app.id);
    } else {
      this.#queue = this.#queue.filter((q) => key(q.entry.id) !== k);
      delete this.jobs[k];
    }
  }
  private enqueue(entry: AppEntry, mode: Mode) {
    const k = key(entry.id);
    if (this.jobs[k]) return;
    this.jobs[k] = { mode, phase: "queued", progress: null };
    this.#queue.push({ entry, mode });
    void this.pump();
  }
  private async pump() {
    if (this.#running) return;
    this.#running = true;
    let changed = false;
    try {
      while (this.#queue.length) {
        const { entry, mode } = this.#queue.shift()!;
        this.#current = entry.id;
        changed = (await this.run(entry, mode)) || changed;
        this.#current = null;
      }
    } finally {
      this.#running = false;
      this.#releaseGate();
    }
    // Pick up exact versions after a batch, and again a little later for
    // installers that were still finishing when winget returned.
    if (changed) {
      void this.checkInstalled();
      clearTimeout(this.#lateCheck);
      this.#lateCheck = setTimeout(() => {
        if (!this.busy) void this.checkInstalled();
      }, LATE_RECHECK_MS);
    }
  }
  /** Runs one job; returns true if something was installed or updated. */
  private async run(entry: AppEntry, mode: Mode): Promise<boolean> {
    const k = key(entry.id);
    const onEvent = (e: JobEvent) => {
      const job = this.jobs[k];
      if (!job) return;
      if (e.event === "stage") {
        job.phase = e.data.stage;
        job.progress = null;
      } else if (e.event === "progress") {
        job.progress = e.data.fraction;
      } else if (e.event === "note") {
        job.note = e.data.text;
      }
    };
    const verb = mode === "upgrade" ? "updated" : "installed";
    try {
      let outcome: JobOutcome =
        entry.source === "custom" ? await api.installCustom(entry.id, onEvent) : await api.installWinget(entry.id, mode, onEvent);
      if (outcome.result === "hashMismatch") {
        const ok = await confirm({
          title: "Installer checksum mismatch",
          message:
            `The ${entry.name} installer does not match the checksum published in winget. ` +
            `The vendor may have replaced the file, or it may have been tampered with.\n\n` +
            `Install it anyway? Windows will ask for administrator approval once.`,
          confirmLabel: "Install anyway",
          danger: true,
        });
        if (!ok) {
          toast.info(`${entry.name} was not ${verb}.`);
          return false;
        }
        outcome = await api.installIgnoringHash(entry.id, mode, onEvent);
      }
      switch (outcome.result) {
        case "done": {
          const s = this.statuses[k];
          this.statuses[k] = {
            ...s,
            installed: true,
            version: s?.available ?? s?.version,
            available: undefined,
            activated: entry.activateAfterInstall || s?.activated,
          };
          this.selected.delete(entry.id);
          this.recent.add(k);
          this.persistSelection();
          toast.success(`${entry.name} ${verb}.${outcome.note ? ` ${outcome.note}` : ""}`);
          return true;
        }
        case "upToDate": {
          const s = this.statuses[k];
          if (s) s.available = undefined;
          this.recent.add(k);
          toast.info(`${entry.name} is already up to date.`);
          return false;
        }
        case "cancelled":
          toast.info(`${entry.name}: cancelled.`);
          return false;
        default:
          return false;
      }
    } catch (err) {
      toast.error(`${entry.name}: ${message(err)}`);
      return false;
    } finally {
      delete this.jobs[k];
    }
  }
  // ---------------------------------------------------------------- import/export
  async exportList() {
    const byId = new Map([...this.all, ...this.pinned].map((a) => [a.id, a]));
    const apps = [...this.selected].map((id) => byId.get(id)).filter((a): a is AppEntry => !!a);
    if (!apps.length) {
      toast.info("Check some apps first, then export them.");
      return;
    }
    const json = JSON.stringify(
      { app: "Make Your Life Easier", kind: "app-list", version: 1, apps: apps.map(({ id, name, source }) => ({ id, name, source })) },
      null,
      2,
    );
    try {
      if (await api.exportList(json)) toast.success(`Exported ${apps.length} ${apps.length === 1 ? "app" : "apps"}.`);
    } catch (err) {
      toast.error(`Export failed: ${message(err)}`);
    }
  }
  async importList() {
    try {
      const text = await api.importList();
      if (text === null) return;
      const data: unknown = JSON.parse(text);
      const rows = isRecord(data) && Array.isArray((data as { apps?: unknown }).apps) ? (data as { apps: unknown[] }).apps : null;
      if (!rows) throw new Error("This is not an app list exported from this app.");
      const validId = /^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/;
      let count = 0;
      for (const row of rows) {
        const id = isRecord(row) && typeof (row as { id?: unknown }).id === "string" ? (row as { id: string }).id : null;
        if (!id || !validId.test(id)) continue;
        const local = this.all.find((a) => key(a.id) === key(id));
        if (local) {
          this.selected.add(local.id);
        } else if (!id.startsWith("Custom.")) {
          const name = typeof (row as { name?: unknown }).name === "string" ? (row as { name: string }).name : id;
          if (!this.pinned.some((p) => key(p.id) === key(id))) {
            this.pinned.push({ id, name, source: "catalog", category: categorize(id) });
          }
          this.selected.add(id);
        } else {
          continue;
        }
        count++;
      }
      this.persistSelection();
      toast.success(`Imported ${count} ${count === 1 ? "app" : "apps"}.`);
    } catch (err) {
      toast.error(`Import failed: ${message(err)}`);
    }
  }
}
export const appsState = new InstallAppsState();
