// State of the System Maintenance page: the fixed cards, the one task that is
// allowed to run, and the output buffers of the embedded consoles.
// A singleton, because ContentArea destroys the page on every navigation.
import { isTauri } from "@tauri-apps/api/core";
import { SvelteSet } from "svelte/reactivity";
import { confirm } from "../../../lib/confirm.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { maintenanceApi, type MaintenanceAction, type MaintenanceCard, type TaskOutcome } from "./api";
const KEY = { view: "myle.maintenance.view" };
/** Kept per console; older lines are dropped so a long run stays responsive. */
const MAX_LINES = 1200;
const TRIM_TO = 1000;
export type View = "grid" | "list";
export type MaintenanceStatusKind =
  | "ready"
  | "running"
  | "completed"
  | "restartRequired"
  | "stopped"
  | "needsAdmin"
  | "error";
export interface MaintenanceCardStatus {
  state: MaintenanceStatusKind;
  label: string;
  actionId?: string;
}
interface ConsoleBuffer {
  lines: string[];
  open: boolean;
  /** How many lines scrolled out of the buffer, so the page can say so. */
  dropped: number;
}
interface PendingLine {
  text: string;
  replace: boolean;
}
const oneOf =
  <T extends string>(...values: T[]) =>
  (v: unknown) =>
    values.includes(v as T);
function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
class MaintenanceState {
  cards = $state<MaintenanceCard[]>([]);
  loading = $state(true);
  view = $state<View>(readJson(KEY.view, "grid", oneOf("grid", "list")));
  /** The action id that is running; everything else is locked while it is set. */
  running = $state<string | null>(null);
  /** "waiting" while the UAC prompt is up, "running" once it started. */
  phase = $state<"waiting" | "running" | null>(null);
  stopping = $state(false);
  consoles = $state<Record<string, ConsoleBuffer>>({});
  /** Actions that turned out to need administrator rights after all. */
  readonly needsAdmin = new SvelteSet<string>();
  error = $state<string | null>(null);
  /** Session-only outcomes. They intentionally do not survive an app restart. */
  cardStatuses = $state<Record<string, MaintenanceCardStatus>>({});
  #pending: Record<string, PendingLine[]> = {};
  #frame = 0;
  #loaded = false;
  /** Locked because of a task we did not start, so nothing will report it done. */
  #adopted = false;
  readonly locked = $derived(this.running !== null);
  isRunning = (actionId: string) => this.running === actionId;
  cardOf(actionId: string): MaintenanceCard | undefined {
    return this.cards.find((card) => card.actions.some((action) => action.id === actionId));
  }
  statusOf(card: MaintenanceCard): MaintenanceCardStatus {
    return this.cardStatuses[card.id] ?? { state: "ready", label: "Ready" };
  }
  #actionName(action: MaintenanceAction): string {
    return action.label.replace(/^Run\s+/i, "").replace(/^Update all$/i, "Updates");
  }
  #setStatus(card: MaintenanceCard, action: MaintenanceAction, state: MaintenanceStatusKind, label: string) {
    this.cardStatuses[card.id] = { state, label, actionId: action.id };
  }
  async load() {
    if (!isTauri()) {
      this.loading = false;
      return;
    }
    if (this.#loaded) {
      this.loading = false;
      return;
    }
    this.loading = true;
    try {
      this.cards = await maintenanceApi.cards();
      this.#loaded = true;
      // A task can outlive a page reload. Keep the buttons locked, but watch
      // for it finishing: its events went to a channel that no longer exists.
      const current = await maintenanceApi.running();
      if (current) {
        this.running = current;
        this.#adopted = true;
        const card = this.cardOf(current);
        const action = card?.actions.find((candidate) => candidate.id === current);
        if (card && action) this.#setStatus(card, action, "running", `Running ${this.#actionName(action)}…`);
        this.#watchAdopted();
      }
    } catch (err) {
      this.error = message(err);
    } finally {
      this.loading = false;
    }
  }
  #watchAdopted() {
    const tick = async () => {
      if (!this.#adopted) return;
      if (await maintenanceApi.running().catch(() => null)) {
        setTimeout(tick, 2000);
        return;
      }
      const adoptedId = this.running;
      const card = adoptedId ? this.cardOf(adoptedId) : undefined;
      if (card) this.cardStatuses[card.id] = { state: "ready", label: "Ready" };
      this.#adopted = false;
      this.running = null;
      this.stopping = false;
    };
    setTimeout(tick, 2000);
  }
  /** Re-reads the saved layout (after account sync replaced it). */
  reloadView() {
    this.view = readJson(KEY.view, this.view, oneOf("grid", "list"));
  }
  setView(view: View) {
    this.view = view;
    writeJson(KEY.view, view);
  }
  toggleConsole(cardId: string) {
    const buffer = (this.consoles[cardId] ??= { lines: [], open: false, dropped: 0 });
    buffer.open = !buffer.open;
  }
  async run(card: MaintenanceCard, action: MaintenanceAction, elevated = false) {
    if (this.locked) return;
    // The retry has already been agreed to; do not ask twice.
    if (action.confirm && !elevated) {
      const ok = await confirm({
        title: `${card.title}?`,
        message: action.confirm,
        confirmLabel: action.label,
        danger: card.caution !== null,
      });
      // Something without a dialog may have started while this one was open.
      if (!ok || this.locked) return;
    }
    this.running = action.id;
    this.phase = "waiting";
    this.#setStatus(
      card,
      action,
      "running",
      card.admin || elevated ? "Waiting for administrator approval…" : `Starting ${this.#actionName(action)}…`,
    );
    this.#adopted = false;
    this.needsAdmin.delete(action.id);
    if (card.console) this.consoles[card.id] = { lines: [], open: true, dropped: 0 };
    try {
      const outcome = await maintenanceApi.run(action.id, elevated, (e) => {
        if (e.event === "waiting") {
          this.phase = "waiting";
          this.#setStatus(card, action, "running", "Waiting for administrator approval…");
        } else if (e.event === "started") {
          this.phase = "running";
          this.#setStatus(card, action, "running", `Running ${this.#actionName(action)}…`);
        }
        else if (card.console) this.#queue(card.id, e.data);
      });
      this.#flush();
      this.#report(card, action, outcome, elevated);
    } catch (err) {
      this.#flush();
      this.#setStatus(card, action, "error", `${this.#actionName(action)} failed`);
      toast.error(`${card.title}: ${message(err)}`);
    } finally {
      // Only release the lock if it is still ours: a rejected second run must
      // not unlock the page while the first one is still going.
      if (this.running === action.id) {
        this.running = null;
        this.phase = null;
        this.stopping = false;
      }
    }
  }
  async cancel(card: MaintenanceCard, action: MaintenanceAction) {
    if (!this.isRunning(action.id) || this.stopping) return;
    if (action.cancelConfirm) {
      const ok = await confirm({
        title: `Stop ${card.title}?`,
        message: action.cancelConfirm,
        confirmLabel: "Stop it",
        cancelLabel: "Keep going",
        danger: true,
      });
      // It may well have finished while the dialog was open.
      if (!ok || !this.isRunning(action.id)) return;
    }
    this.stopping = true;
    this.#setStatus(card, action, "running", `Stopping ${this.#actionName(action)}…`);
    void maintenanceApi.cancel(action.id);
  }
  #report(card: MaintenanceCard, action: MaintenanceAction, outcome: TaskOutcome, elevated: boolean) {
    switch (outcome.result) {
      case "done":
        this.#setStatus(card, action, "completed", `${this.#actionName(action)} completed`);
        toast.success(outcome.note ? `${card.title}: ${outcome.note}` : `${card.title} finished.`);
        break;
      case "rebootRequired":
        this.#setStatus(card, action, "restartRequired", "Restart required");
        toast.info(`${card.title} finished. Restart your PC to complete it.`);
        break;
      case "cancelled":
        this.#setStatus(card, action, "stopped", `${this.#actionName(action)} stopped`);
        toast.info(`${card.title} stopped.`);
        break;
      case "needsAdmin":
        this.#setStatus(card, action, "needsAdmin", "Administrator required");
        if (card.admin || elevated) {
          toast.info("Administrator approval was declined.");
        } else {
          this.needsAdmin.add(action.id);
          toast.info(`${card.title} needs administrator rights on this PC.`);
        }
        break;
    }
  }
  /** Console output arrives in bursts; repaint at most once a frame. */
  #queue(cardId: string, line: PendingLine) {
    (this.#pending[cardId] ??= []).push(line);
    if (this.#frame) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0;
      this.#flush();
    });
  }
  #flush() {
    for (const [cardId, incoming] of Object.entries(this.#pending)) {
      const buffer = this.consoles[cardId];
      if (!buffer) continue;
      for (const line of incoming) {
        // A bare carriage return repaints the line instead of adding one.
        if (line.replace && buffer.lines.length) buffer.lines[buffer.lines.length - 1] = line.text;
        else buffer.lines.push(line.text);
      }
      if (buffer.lines.length > MAX_LINES) {
        buffer.dropped += buffer.lines.length - TRIM_TO;
        buffer.lines.splice(0, buffer.lines.length - TRIM_TO);
      }
    }
    this.#pending = {};
  }
}
export const maintenanceState = new MaintenanceState();
