import type { PageId } from "../app/pages/registry";
import { readFlag, readJson, writeFlag, writeJson } from "./storage";
const SIDEBAR_KEY = "myle.sidebar.collapsed";
const SCROLL_KEY = "myle.scroll";
/* App-wide navigation state: the active page, the sidebar mode and the scroll position of every page (restored when you come back to it). */
class Nav {
  current = $state<PageId>("install-apps");
  collapsed = $state(readFlag(SIDEBAR_KEY, false));
  #scroll: Record<string, number> = readJson(SCROLL_KEY, {}, (v) => typeof v === "object" && v !== null);
  #saveTimer: ReturnType<typeof setTimeout> | undefined;
  go(id: PageId) {
    this.current = id;
  }
  /** Re-reads the saved sidebar mode (after account sync replaced it). */
  reload() {
    this.collapsed = readFlag(SIDEBAR_KEY, this.collapsed);
  }
  toggleSidebar = () => {
    this.collapsed = !this.collapsed;
    writeFlag(SIDEBAR_KEY, this.collapsed);
  };
  scrollOf(id: PageId): number {
    return this.#scroll[id] ?? 0;
  }
  rememberScroll(id: PageId, top: number) {
    this.#scroll[id] = top;
    clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => writeJson(SCROLL_KEY, this.#scroll), 300);
  }
}
export const nav = new Nav();
