interface Shortcut {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}
/** Release windows keep editing shortcuts, but expose no browser tools. */
export function blockedWebviewShortcut(event: Shortcut, development: boolean): boolean {
  if (development) return false;
  const key = event.key.toLowerCase();
  const modifier = event.ctrlKey || event.metaKey;
  return key === "f12" || key === "f5" || key === "f7"
    || (modifier && ["r", "p", "f", "g", "u"].includes(key))
    || (modifier && event.shiftKey && ["i", "j", "c", "k"].includes(key));
}
