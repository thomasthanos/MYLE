// Promise-based confirmation dialog: `if (await confirm({...})) { ... }`
export interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Style the confirm button as destructive/risky. */
  danger?: boolean;
}
interface Pending extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}
class ConfirmState {
  current = $state<Pending | null>(null);
  /** Questions asked while another one is on screen, answered in turn. */
  #waiting: Pending[] = [];
  ask(options: ConfirmOptions): Promise<boolean> {
    return new Promise((resolve) => {
      const pending = { ...options, resolve };
      // Never answer an open question for the user: a second one waits.
      if (this.current) this.#waiting.push(pending);
      else this.current = pending;
    });
  }
  answer(ok: boolean) {
    const pending = this.current;
    this.current = this.#waiting.shift() ?? null;
    pending?.resolve(ok);
  }
}
export const confirmState = new ConfirmState();
export function confirm(options: ConfirmOptions): Promise<boolean> {
  return confirmState.ask(options);
}
