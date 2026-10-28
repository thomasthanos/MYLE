/* A small UI-side mutex for workflows that must not overlap. The Rust side is still authoritative; this gate keeps the two pages consistent before an IPC request reaches it. */
export type OperationOwner = "install-apps" | "spotify-hub" | "windows-optimization";
class OperationGate {
  owner = $state<OperationOwner | null>(null);
  begin(owner: OperationOwner): boolean {
    if (this.owner !== null && this.owner !== owner) return false;
    this.owner = owner;
    return true;
  }
  end(owner: OperationOwner) {
    if (this.owner === owner) this.owner = null;
  }
  lockedFor(owner: OperationOwner): boolean {
    return this.owner !== null && this.owner !== owner;
  }
}
export const operationGate = new OperationGate();
