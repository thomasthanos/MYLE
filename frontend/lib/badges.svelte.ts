import type { PageId } from "../app/pages/registry";
/* Counts shown next to a page in the sidebar ("3 saves changed"). A page's state sets its own count; zero hides the badge. */
class Badges {
  counts = $state<Partial<Record<PageId, number>>>({});
  set(page: PageId, count: number) {
    if ((this.counts[page] ?? 0) !== count) this.counts[page] = count;
  }
  of(page: PageId): number {
    return this.counts[page] ?? 0;
  }
}
export const badges = new Badges();
