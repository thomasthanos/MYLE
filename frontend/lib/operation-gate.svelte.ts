/* A small UI-side mutex for workflows that must not overlap. The Rust side is still authoritative; this gate keeps the two pages consistent before an IPC request reaches it.
 *
 * One page can run several workflows (Windows Optimization: the debloat run, Start menu changes, Auto-Logon, a firmware restart, and an operation the backend reports while it runs). Each holds the gate under a key of its own, and the gate is free only once every key is released: one workflow ending (or a page reload seeing nothing of its own running) can no longer free the gate under another that is still running. */
export type OperationOwner = "install-apps" | "spotify-hub" | "windows-optimization";
class OperationGate {
  owner = $state<OperationOwner | null>(null);
  /** The workflows of `owner` holding the gate. */
  keys = $state<string[]>([]);
  begin(owner: OperationOwner, key = "main"): boolean {
    if (this.owner !== null && this.owner !== owner) return false;
    this.owner = owner;
    if (!this.keys.includes(key)) this.keys = [...this.keys, key];
    return true;
  }
  end(owner: OperationOwner, key = "main") {
    if (this.owner !== owner) return;
    this.keys = this.keys.filter((held) => held !== key);
    if (this.keys.length === 0) this.owner = null;
  }
  lockedFor(owner: OperationOwner): boolean {
    return this.owner !== null && this.owner !== owner;
  }
  /** Whether `owner` holds the gate under any key other than `own`. */
  heldBesides(owner: OperationOwner, own: readonly string[]): boolean {
    return this.owner === owner && this.keys.some((key) => !own.includes(key));
  }
}
export const operationGate = new OperationGate();
