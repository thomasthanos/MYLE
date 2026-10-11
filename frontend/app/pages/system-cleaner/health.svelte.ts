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
  | { event: "module"; data: { module: string; done: number; total: number } }
  | { event: "found"; data: { module: string; count: number } };

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

  async scan(modules: HealthModule[] = []) {
    if (this.scanning) return;
    this.scanning = true;
    this.done = 0;
    this.total = modules.length || MODULES.length;
    const channel = new Channel<HealthEvent>();
    channel.onmessage = (e) => {
      if (e.event === "module") {
        this.current = e.data.module;
        this.done = e.data.done;
        this.total = e.data.total;
      }
    };
    try {
      const list = await invoke<ModuleReport[]>("cleaner_health_scan", { modules, onEvent: channel });
      const next = { ...this.reports };
      for (const r of list) next[r.module] = r;
      this.reports = next;
      this.selected = new Set();
      this.failures = {};
    } catch (e) {
      toast.error(`Software Health scan failed: ${e}`);
    } finally {
      this.scanning = false;
      this.current = "";
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
