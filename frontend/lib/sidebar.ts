// The sidebar's order, groups, shortcuts and keyboard moves, kept apart from
// the components so they can be tested on their own (scripts/tests).
/** Where a page sits in the sidebar, top to bottom. */
export type NavGroup = "apps" | "data" | "system" | "owner" | "bottom";
/** The groups of the upper list, in order. "bottom" (Settings) is pinned below. */
export const TOP_GROUPS: readonly NavGroup[] = ["apps", "data", "system", "owner"];
/** Screen-reader names of the groups. */
export const GROUP_LABELS: Record<NavGroup, string> = {
  apps: "Apps",
  data: "Your data",
  system: "System",
  owner: "Owner only",
  bottom: "App",
};
export interface NavEntry {
  id: string;
  group: NavGroup;
}
export interface NavSection<T extends NavEntry> {
  group: NavGroup;
  pages: T[];
}
/** The upper list's groups, each with its pages in registry order; empty
 *  groups (the owner's, for everyone else) are left out. */
export function sections<T extends NavEntry>(pages: readonly T[]): NavSection<T>[] {
  return TOP_GROUPS.map((group) => ({ group, pages: pages.filter((page) => page.group === group) })).filter(
    (section) => section.pages.length > 0,
  );
}
/** Ctrl+1 … Ctrl+9, then Ctrl+0, follow the upper list's order.
 *  Ctrl+. opens Settings; Ctrl+, remains an alias. */
export const SETTINGS_SHORTCUT = "Ctrl+.";
export const SIDEBAR_SHORTCUT = "Ctrl+B";
export function shortcutOf<T extends NavEntry>(pages: readonly T[], id: string): string | null {
  const bottom = pages.find((page) => page.id === id)?.group === "bottom";
  if (bottom) return id === "settings" ? SETTINGS_SHORTCUT : null;
  const index = sections(pages)
    .flatMap((section) => section.pages)
    .findIndex((page) => page.id === id);
  return index >= 0 && index < 10 ? `Ctrl+${(index + 1) % 10}` : null;
}
interface Keys {
  key: string;
  code?: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}
function ctrlOnly(event: Keys): boolean {
  return event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey;
}
/** Match the physical B key too, so Greek β/Β does not break the toggle. */
export function isSidebarToggle(event: Keys): boolean {
  return ctrlOnly(event) && (event.code === "KeyB" || event.key.toLowerCase() === "b");
}
/** The page a navigation shortcut asks for, if the keys are one. */
export function pageForShortcut<T extends NavEntry>(pages: readonly T[], event: Keys): T | undefined {
  if (!ctrlOnly(event)) return undefined;
  if (event.code === "Period" || event.code === "Comma" || event.key === "." || event.key === ",") {
    return pages.find((page) => page.id === "settings");
  }
  const digit = event.code?.match(/^Digit([0-9])$/)?.[1] ?? event.key;
  if (!/^[0-9]$/.test(digit)) return undefined;
  const index = digit === "0" ? 9 : Number(digit) - 1;
  return sections(pages).flatMap((section) => section.pages)[index];
}
/** Leave room for the floating panel's shadow while keeping its arrow on
 *  the button. The arrow stays clear of the rounded panel corners. */
export function tooltipPosition(anchorY: number, height: number, viewportHeight: number) {
  const edge = 32;
  const top = Math.max(edge, Math.min(anchorY - height / 2, viewportHeight - height - edge));
  const arrow = Math.max(18, Math.min(anchorY - top, height - 18));
  return { top, arrow };
}
/** Arrow keys, Home and End inside the sidebar: the index to focus next
 *  (wrapping around), or null for any other key. */
export function nextFocus(current: number, count: number, key: string): number | null {
  if (count <= 0) return null;
  switch (key) {
    case "ArrowDown":
      return current < 0 ? 0 : (current + 1) % count;
    case "ArrowUp":
      return current < 0 ? count - 1 : (current - 1 + count) % count;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}
