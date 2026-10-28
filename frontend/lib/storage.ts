// localStorage wrappers that never throw (storage can be blocked or unavailable). Every write is also announced to one listener: account sync uses it to notice changed preferences without each page having to report them.
type ChangeListener = (key: string) => void;
let listener: ChangeListener | null = null;
/** Registers the (single) listener told about every write. */
export function onStorageChange(fn: ChangeListener): void {
  listener = fn;
}
/** Announces a change kept outside localStorage (e.g. Game Saves settings). */
export function notifyChange(key: string): void {
  try {
    listener?.(key);
  } catch {
    // A listener failure never breaks saving a preference.
  }
}
export function readFlag(key: string, fallback: boolean): boolean {
  try {
    const value = localStorage.getItem(key);
    return value === null ? fallback : value === "1";
  } catch {
    return fallback;
  }
}
export function writeFlag(key: string, value: boolean): void {
  try {
    localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // Not persisted; the in-memory state still applies.
  }
  notifyChange(key);
}
/** Parsed JSON value, or `fallback` if missing, unreadable or rejected by `valid`. */
export function readJson<T>(key: string, fallback: T, valid: (value: unknown) => boolean = () => true): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    const value: unknown = JSON.parse(raw);
    return valid(value) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}
export function writeJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Not persisted; the in-memory state still applies.
  }
  notifyChange(key);
}
