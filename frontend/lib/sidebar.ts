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

/** Ctrl+1 … Ctrl+9 open the first nine pages of the upper list, in the order
 *  they are shown; Ctrl+, opens Settings. */
export const SETTINGS_SHORTCUT = "Ctrl+,";

export function shortcutOf<T extends NavEntry>(pages: readonly T[], id: string): string | null {
  const bottom = pages.find((page) => page.id === id)?.group === "bottom";
  if (bottom) return id === "settings" ? SETTINGS_SHORTCUT : null;
  const index = sections(pages)
    .flatMap((section) => section.pages)
    .findIndex((page) => page.id === id);
  return index >= 0 && index < 9 ? `Ctrl+${index + 1}` : null;
}

interface Keys {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/** The page a navigation shortcut asks for, if the keys are one. */
export function pageForShortcut<T extends NavEntry>(pages: readonly T[], event: Keys): T | undefined {
  if (!event.ctrlKey || event.shiftKey || event.altKey || event.metaKey) return undefined;
  if (event.key === ",") return pages.find((page) => page.id === "settings");
  if (!/^[1-9]$/.test(event.key)) return undefined;
  return sections(pages).flatMap((section) => section.pages)[Number(event.key) - 1];
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
