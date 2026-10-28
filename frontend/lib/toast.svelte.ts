// App-wide toast notifications. - at most 2 on screen, the rest wait in a queue - errors stay until dismissed, unless other messages are waiting: then they make room after 12 s; success/info close after 5 s - repeating a visible or queued message bumps its counter (×2, ×3…)
export type ToastKind = "success" | "info" | "error";
/** A button on the toast; clicking it runs `run` and closes the toast. */
export interface ToastAction {
  label: string;
  run: () => void;
}
export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  count: number;
  action?: ToastAction;
}
const MAX_VISIBLE = 2;
const AUTO_CLOSE_MS = 5000;
/** How long an error keeps its place while other toasts wait behind it. */
const ERROR_YIELD_MS = 12000;
class Toasts {
  visible = $state<Toast[]>([]);
  private queue: Toast[] = [];
  private timers = new Map<number, ReturnType<typeof setTimeout>>();
  private nextId = 1;
  success = (message: string, action?: ToastAction) => this.push("success", message, action);
  info = (message: string, action?: ToastAction) => this.push("info", message, action);
  error = (message: string, action?: ToastAction) => this.push("error", message, action);
  dismiss = (id: number) => {
    clearTimeout(this.timers.get(id));
    this.timers.delete(id);
    this.visible = this.visible.filter((t) => t.id !== id);
    this.fill();
  };
  private push(kind: ToastKind, message: string, action?: ToastAction) {
    const same = (t: Toast) => t.kind === kind && t.message === message;
    const shown = this.visible.find(same);
    if (shown) {
      shown.count++;
      this.arm(shown); // restart its timer
      this.yieldErrors();
      return;
    }
    const queued = this.queue.find(same);
    if (queued) {
      queued.count++;
      return;
    }
    this.queue.push({ id: this.nextId++, kind, message, count: 1, action });
    this.fill();
  }
  /** Errors that fill every slot would hold the queue forever. */
  private yieldErrors() {
    if (!this.queue.length) return;
    for (const toast of this.visible) {
      if (toast.kind === "error" && !this.timers.has(toast.id)) {
        this.timers.set(toast.id, setTimeout(() => this.dismiss(toast.id), ERROR_YIELD_MS));
      }
    }
  }
  private fill() {
    while (this.visible.length < MAX_VISIBLE && this.queue.length) {
      const toast = this.queue.shift()!;
      this.visible.push(toast);
      this.arm(this.visible[this.visible.length - 1]);
    }
    this.yieldErrors();
  }
  private arm(toast: Toast) {
    clearTimeout(this.timers.get(toast.id));
    this.timers.delete(toast.id);
    if (toast.kind === "error") return;
    this.timers.set(toast.id, setTimeout(() => this.dismiss(toast.id), AUTO_CLOSE_MS));
  }
}
export const toast = new Toasts();
