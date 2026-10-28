import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
export type Mode = "simple" | "sequence";
export type MouseButton = "left" | "right" | "middle";
export type ClickType = "single" | "double";
export interface Point { x: number; y: number }
export interface Step extends Point { id: string; clicks: number; button: MouseButton; clickType: ClickType; intervalMs: number; delayMs: number }
export interface Config { mode: Mode; button: MouseButton; clickType: ClickType; intervalMs: number; steps: Step[]; loops: number | null }
export interface Profile { id: string; name: string; config: Config }
export interface Profiles { version: number; activeId: string; items: Profile[] }
export type Phase = "ready" | "countdown" | "running" | "stopping" | "picking" | "completed" | "stopped" | "error";
export interface Status {
  revision: number; runId: number; phase: Phase; mode: Mode;
  loop: number; loops: number | null; step: number; steps: number; click: number; clicks: number;
  completed: number; total: number | null; countdown: number;
  hotkey: boolean; hotkeyError: string | null; error: string | null;
}
export const clickerApi = {
  load: () => invoke<Profiles>("autoclicker_load_profiles"),
  save: (profiles: Profiles) => invoke<void>("autoclicker_save_profiles", { profiles }),
  configure: (config: Config | null) => invoke<void>("autoclicker_configure", { config }),
  status: () => invoke<Status>("autoclicker_get_status"),
  start: (config: Config) => invoke<Status>("autoclicker_start", { config }),
  stop: (pickRequestId?: string) => invoke<Status>("autoclicker_stop", { pickRequestId: pickRequestId ?? null }),
  pick: (requestId: string) => invoke<Point | null>("autoclicker_pick_location", { requestId }),
  listen: (onStatus: (status: Status) => void) => listen<Status>("autoclicker-status", ({ payload }) => onStatus(payload)),
};
