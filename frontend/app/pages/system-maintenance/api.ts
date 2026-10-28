// Typed bridge to the maintenance commands in backend/src/maintenance/.
import { Channel, invoke } from "@tauri-apps/api/core";
export type SectionId = "network" | "health" | "software";
export interface MaintenanceAction {
  id: string;
  label: string;
  cancellable: boolean;
  /** Shown before starting; null runs straight away. */
  confirm: string | null;
  /** Shown before stopping a run in progress. */
  cancelConfirm: string | null;
}
export interface MaintenanceCard {
  id: string;
  section: SectionId;
  title: string;
  description: string;
  icon: string;
  admin: boolean;
  caution: string | null;
  /** Show the embedded live console under this card. */
  console: boolean;
  actions: MaintenanceAction[];
}
export type TaskEvent =
  /** The UAC prompt is on screen; nothing is running yet. */
  | { event: "waiting" }
  | { event: "started" }
  | { event: "line"; data: { text: string; replace: boolean } };
export type TaskOutcome =
  | { result: "done"; note: string | null }
  | { result: "rebootRequired" }
  | { result: "cancelled" }
  | { result: "needsAdmin" };
function channel<T>(onEvent: (e: T) => void): Channel<T> {
  const ch = new Channel<T>();
  ch.onmessage = onEvent;
  return ch;
}
export const maintenanceApi = {
  cards: () => invoke<MaintenanceCard[]>("maintenance_cards"),
  run: (id: string, elevated: boolean, onEvent: (e: TaskEvent) => void) =>
    invoke<TaskOutcome>("maintenance_run", { id, elevated, onEvent: channel(onEvent) }),
  cancel: (id: string) => invoke<void>("maintenance_cancel", { id }),
  /** The action the backend still has running, if the page was reloaded. */
  running: () => invoke<string | null>("maintenance_running"),
};
