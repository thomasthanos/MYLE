// Software Health: the eight scans in backend/src/cleaner/health/, their
// findings, and the reversible changes offered for them.
import { Channel, invoke } from "@tauri-apps/api/core";
import { confirm } from "../../../lib/confirm.svelte";
import { toast } from "../../../lib/toast.svelte";
import { formatSize } from "./api";

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
}

export interface ModuleReport { module: HealthModule; findings: Finding[]; error: string | null }
export interface RestoreEntry { id: string; at: number; module: string; title: string; undo: { kind: string } }
export interface ApplyResult { id: string; ok: boolean; message: string | null }
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
  disable: "Disable", enable: "Enable", manual: "Set to Manual", revoke: "Revoke", allow: "Allow",
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

class HealthState {
  reports = $state<Record<string, ModuleReport>>({});
  scanning = $state(false);
  current = $state("");
  done = $state(0);
  total = $state(0);
  busy = $state(false);
  selected = $state<Set<string>>(new Set());
  restore = $state<RestoreEntry[]>([]);
  failures = $state<Record<string, string>>({});

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
    if (this.scanning) return;
    const ids = modules.length ? modules : MODULES.map((m) => m.id);
    this.scanning = true;
    this.total = ids.length;
    this.done = 0;
    this.progress = Object.fromEntries(ids.map((id) => [id, 0]));
    this.running = new Set();
    const channel = new Channel<HealthEvent>();
    channel.onmessage = (e) => {
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
        this.done += 1;
        this.current = [...running][0] ?? "";
      }
    };
    try {
      await invoke<ModuleReport[]>("cleaner_health_scan", { modules, onEvent: channel });
      const keep = new Set(Object.values(this.reports).flatMap((r) => r.findings.map((f) => f.id)));
      this.selected = new Set([...this.selected].filter((id) => keep.has(id)));
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
    const next = new Set(this.selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.selected = next;
  }

  /** Previews `items`, asks, then applies `action` and rescans the module. */
  async apply(module: HealthModule, action: HealthAction, items: Finding[]) {
    if (!items.length || this.busy) return;
    if (action !== "open") {
      const bytes = items.reduce((n, f) => n + (f.size ?? 0), 0);
      const list = items.slice(0, 12).map((f) => `• ${f.title}${f.path ? ` — ${f.path}` : ""}`).join("\n");
      const more = items.length > 12 ? `\n…and ${items.length - 12} more` : "";
      const ok = await confirm({
        title: `${ACTION_LABELS[action]}: ${items.length} item(s)?`,
        message: `${list}${more}${bytes ? `\n\nAbout ${formatSize(bytes)}.` : ""}\n\n${ACTION_NOTES[action] ?? "Every change is recorded in the restore list and can be undone."}`,
        confirmLabel: ACTION_LABELS[action],
      });
      if (!ok) return;
    }
    this.busy = true;
    try {
      const results = await invoke<ApplyResult[]>("cleaner_health_apply", { ids: items.map((f) => f.id), action });
      const failed = results.filter((r) => !r.ok);
      const failures = { ...this.failures };
      for (const r of failed) failures[r.id] = r.message ?? "Failed";
      this.failures = failures;
      const okCount = results.length - failed.length;
      if (okCount) toast.success(`${ACTION_LABELS[action]}: ${okCount} done.`);
      if (failed.length) toast.error(`${failed.length} could not be changed: ${failed[0].message ?? ""}`);
    } catch (e) {
      toast.error(String(e));
    } finally {
      this.busy = false;
    }
    if (action !== "open") {
      const keep = this.failures;
      await this.scan([module]);
      this.failures = keep;
    }
  }

  async loadRestore() {
    this.restore = await invoke<RestoreEntry[]>("cleaner_health_restore_list").catch(() => []);
  }

  async undo(entry: RestoreEntry) {
    try {
      await invoke("cleaner_health_restore", { id: entry.id });
      toast.success(`Restored: ${entry.title}`);
    } catch (e) {
      toast.error(String(e));
    }
    await this.loadRestore();
  }
}

export const health = new HealthState();
