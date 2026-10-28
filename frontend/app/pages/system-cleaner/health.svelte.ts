// Software Health: the eight scans in backend/src/cleaner/health/, their findings, and the reversible changes offered for them.
import { Channel, invoke } from "@tauri-apps/api/core";
import { confirm } from "../../../lib/confirm.svelte";
import { toast } from "../../../lib/toast.svelte";
import { formatSize } from "./api";
import type { BatchItem } from "./health-model";
export type HealthModule =
  | "leftovers" | "broken" | "redundant" | "hibernation"
  | "permissions" | "notifications" | "installers" | "suspicious";
export type HealthAction =
  | "recycle" | "forceClean" | "deleteKey" | "disable" | "enable" | "manual"
  | "revoke" | "allow" | "mute" | "unmute" | "block" | "open";
export interface Finding {
  id: string;
  module: HealthModule;
  title: string;
  detail: string;
  path: string | null;
  size: number | null;
  confidence: number | null;
  enabled: boolean | null;
  flags: string[];
  actions: HealthAction[];
  cleanupAction: HealthAction | null;
  recommended: boolean;
  reviewReason: string | null;
}
export interface ModuleReport { module: HealthModule; findings: Finding[]; error: string | null }
export interface RestoreEntry { id: string; at: number; module: string; title: string; undo: { kind: string } }
export interface ApplyResult { id: string; ok: boolean; message: string | null }
export interface BatchProgress { done: number; total: number; current: string }
export type HealthEvent =
  | { event: "started"; data: { module: HealthModule } }
  | { event: "step"; data: { module: HealthModule; fraction: number } }
  | { event: "result"; data: { report: ModuleReport } };
export const MODULES: { id: HealthModule; title: string; hint: string }[] = [
  { id: "leftovers", title: "Uninstallation leftovers", hint: "Folders and registry entries of software that is gone." },
  { id: "broken", title: "Broken uninstalls", hint: "Programs listed in Windows whose uninstaller or folder is missing." },
  { id: "redundant", title: "Redundant files", hint: "Caches, crash dumps and logs of known apps." },
  { id: "hibernation", title: "Software hibernation", hint: "Startup entries, auto services and busy background processes." },
  { id: "permissions", title: "Software permissions", hint: "Apps allowed to use camera, microphone, location or run in background." },
  { id: "notifications", title: "Disturbing notifications", hint: "App and browser site notifications." },
  { id: "installers", title: "Installation files", hint: "Old setup files in Temp and Downloads (7-day rule)." },
  { id: "suspicious", title: "Suspicious software & extensions", hint: "Side-loaded extensions, unsigned startup programs and tasks." },
];
export const ACTION_LABELS: Record<HealthAction, string> = {
  recycle: "Move to Recycle Bin", forceClean: "Force clean", deleteKey: "Remove entry",
  disable: "Disable", enable: "Enable", manual: "Set to Manual", revoke: "Block access", allow: "Allow access",
  mute: "Mute", unmute: "Unmute", block: "Block", open: "Open in browser",
};
/** What each action does, for the preview dialog. */
const ACTION_NOTES: Partial<Record<HealthAction, string>> = {
  recycle: "Files go to the Recycle Bin and can be restored from there.",
  forceClean: "The registry entry is exported to a .reg backup first; its leftover folders go to the Recycle Bin.",
  deleteKey: "The registry entry is exported to a .reg backup first and can be restored.",
  manual: "Windows asks for administrator approval. The service then starts only when needed; restore puts it back.",
  block: "The browser must be closed. Restore puts the permission back.",
};
export const healthApi = {
  scan: async (modules: HealthModule[], onEvent: (event: HealthEvent) => void) => {
    const channel = new Channel<HealthEvent>(); channel.onmessage = onEvent;
    return invoke<ModuleReport[]>("cleaner_health_scan", { modules, onEvent: channel });
  },
  batch: async (items: BatchItem[], onEvent: (event: BatchProgress) => void) => {
    const channel = new Channel<BatchProgress>(); channel.onmessage = onEvent;
    return invoke<ApplyResult[]>("cleaner_health_apply_batch", { items, onEvent: channel });
  },
  restoreList: () => invoke<RestoreEntry[]>("cleaner_health_restore_list"),
  undo: (id: string) => invoke<void>("cleaner_health_restore", { id }),
  reveal: (path: string) => invoke<void>("cleaner_health_reveal", { path }),
};
export class HealthState {
  reports = $state<Record<string, ModuleReport>>({});
  scanning = $state(false);
  current = $state("");
  done = $state(0);
  total = $state(0);
  busy = $state(false);
  selected = $state<Set<string>>(new Set());
  restore = $state<RestoreEntry[]>([]);
  failures = $state<Record<string, string>>({});
  overrides = $state<Record<string, HealthAction>>({});
  operation = $state<BatchProgress | null>(null);
  lastResults = $state<ApplyResult[]>([]);
  #seen = new Set<string>();
  #seenModules = new Map<string, HealthModule>();
  #attempts = new Map<string, BatchItem>();
  get findings() { return Object.values(this.reports).flatMap((r) => r.findings); }
  get picked() { return this.findings.filter((f) => this.selected.has(f.id)); }
  get count() {
    return Object.values(this.reports).reduce((n, r) => n + r.findings.length, 0);
  }
  /** Per module while a scan runs: 0..1, or 1 once its result is in. */
  progress = $state<Record<string, number>>({});
  /** Modules that are scanning right now. */
  running = $state<Set<string>>(new Set());
  /** Overall 0..1: finished modules plus how far the running ones are. */
  get fraction() {
    const ids = Object.keys(this.progress);
    if (!ids.length) return 0;
    return ids.reduce((n, id) => n + this.progress[id], 0) / ids.length;
  }
  async scan(modules: HealthModule[] = []) {
    if (this.scanning || this.busy) return;
    await this.#scan(modules, true);
  }
  async #scan(modules: HealthModule[], recommend: boolean) {
    const ids = modules.length ? modules : MODULES.map((m) => m.id);
    this.scanning = true;
    this.total = ids.length;
    this.done = 0;
    this.progress = Object.fromEntries(ids.map((id) => [id, 0]));
    this.running = new Set();
    const completed = new Set<string>();
    const receive = (e: HealthEvent) => {
      if (e.event === "started") {
        this.running = new Set([...this.running, e.data.module]);
      } else if (e.event === "step") {
        // Never backwards, and never full before the result.
        const now = this.progress[e.data.module] ?? 0;
        this.progress = { ...this.progress, [e.data.module]: Math.max(now, Math.min(0.95, e.data.fraction)) };
      } else if (e.event === "result") {
        const r = e.data.report;
        this.reports = { ...this.reports, [r.module]: r };
        this.progress = { ...this.progress, [r.module]: 1 };
        const running = new Set(this.running);
        running.delete(r.module);
        this.running = running;
        completed.add(r.module);
        this.done = completed.size;
        this.current = [...running][0] ?? "";
      }
    };
    try {
      const reports = await healthApi.scan(modules, receive);
      for (const report of reports) receive({ event: "result", data: { report } });
      const successful = new Set([...completed].filter(id => !this.reports[id]?.error));
      const oldModules = new Map(this.#seenModules);
      const keep = new Set(this.findings.map((f) => f.id));
      this.selected = new Set([...this.selected].filter((id) => keep.has(id) || !successful.has(oldModules.get(id) ?? "")));
      for (const f of this.findings) {
        if (recommend && !this.#seen.has(f.id) && f.recommended && f.cleanupAction) this.selected.add(f.id);
        this.#seen.add(f.id);
        this.#seenModules.set(f.id, f.module);
      }
      this.selected = new Set(this.selected);
    } catch (e) {
      toast.error(`Software Health scan failed: ${e}`);
    } finally {
      this.scanning = false;
      this.current = "";
      this.running = new Set();
    }
    await this.loadRestore();
  }
  toggle(id: string) {
    if (this.busy || this.scanning) return;
    const next = new Set(this.selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.selected = next;
  }
  /** Compatibility entry point; mixed lists use applyBatch. */
  async apply(module: HealthModule, action: HealthAction, items: Finding[]) {
    await this.applyBatch(items.filter((f) => f.module === module && f.actions.includes(action)).map((f) => ({ id: f.id, action })));
  }
  async applyBatch(requests: BatchItem[]) {
    if (!requests.length || this.busy || this.scanning) return;
    const byId = new Map(this.findings.map((f) => [f.id, f]));
    const items = [...new Map(requests.filter((i) => byId.get(i.id)?.actions.includes(i.action)).map((i) => [i.id, i])).values()];
    if (!items.length) return;
    // Lock before preview: a second click, scan or restore cannot race it.
    this.busy = true;
    let attempted = false;
    const modules = [...new Set(items.filter((i) => i.action !== "open").map((i) => byId.get(i.id)!.module))];
    try {
      if (items.some((i) => i.action !== "open")) {
        const groups = new Map<HealthAction, Finding[]>();
        for (const i of items) groups.set(i.action, [...groups.get(i.action) ?? [], byId.get(i.id)!]);
        const bytes = items.reduce((n, i) => n + (byId.get(i.id)?.size ?? 0), 0);
        const message = [...groups].map(([action, rows]) => `${ACTION_LABELS[action]} (${rows.length})\n${ACTION_NOTES[action] ?? "Changes are recorded for restore."}\n${rows.map((f) => `• ${f.title}${f.module === "permissions" && f.flags[0] ? ` (${f.flags[0]})` : ""}${f.path ? ` — ${f.path}` : ""}`).join("\n")}`).join("\n\n");
        if (!await confirm({ title: `Review ${items.length} selected item${items.length === 1 ? "" : "s"}`, message: `${message}${bytes ? `\n\nSelected files: ${formatSize(bytes)}` : ""}`, confirmLabel: "Apply changes", danger: items.some((i) => i.action === "forceClean") })) return;
      }
      attempted = true;
      for (const item of items) this.#attempts.set(item.id, item);
      this.operation = { done: 0, total: items.length, current: "Preparing changes…" };
      const results = await healthApi.batch(items, (progress) => { this.operation = progress; });
      // A missing native result must never be counted as successful.
      const received = new Map(results.map((r) => [r.id, r]));
      this.lastResults = items.map((i) => received.get(i.id) ?? { id: i.id, ok: false, message: "No result received. Scan again before retrying." });
      const failed = this.lastResults.filter((r) => !r.ok);
      const failures = { ...this.failures };
      const selected = new Set(this.selected);
      for (const r of this.lastResults) {
        if (r.ok) { delete failures[r.id]; selected.delete(r.id); }
        else { failures[r.id] = r.message ?? "Failed"; selected.add(r.id); }
      }
      this.failures = failures;
      this.selected = selected;
      const okCount = this.lastResults.length - failed.length;
      if (okCount) toast.success(`${okCount} selected item${okCount === 1 ? "" : "s"} completed.`);
      if (failed.length) toast.error(`${failed.length} could not be changed: ${failed[0].message ?? ""}`);
    } catch (e) {
      const failures = { ...this.failures };
      for (const item of items) failures[item.id] = String(e);
      this.failures = failures;
      this.lastResults = items.map(item => ({ id: item.id, ok: false, message: String(e) }));
      this.selected = new Set([...this.selected, ...items.map(item => item.id)]);
      toast.error(String(e));
    } finally {
      if (attempted && modules.length) await this.#scan(modules, false);
      this.busy = false;
      this.operation = null;
    }
  }
  async retryFailed() { await this.applyBatch([...this.#attempts.values()].filter((i) => !!this.failures[i.id])); }
  async loadRestore() {
    this.restore = await healthApi.restoreList().catch(() => []);
  }
  async undo(entry: RestoreEntry) {
    if (this.busy || this.scanning || entry.undo.kind === "recycled") return;
    this.busy = true;
    try {
      await healthApi.undo(entry.id);
      toast.success(`Restored: ${entry.title}`);
      await this.#scan([entry.module as HealthModule], false);
    } catch (e) {
      toast.error(String(e));
    } finally {
      this.busy = false;
    }
    await this.loadRestore();
  }
}
export const health = new HealthState();
