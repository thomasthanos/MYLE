// The page's pure logic: what its filters are, and how times and reviews read.
export interface Filters {
  onlineCoop: boolean;
  singlePlayer: boolean;
  storyOnly: boolean;
}

export const DEFAULT_FILTERS: Filters = { onlineCoop: true, singlePlayer: false, storyOnly: false };

export function validFilters(value: unknown): boolean {
  if (typeof value !== "object" || value === null) return false;
  const filters = value as Record<string, unknown>;
  return [filters.onlineCoop, filters.singlePlayer, filters.storyOnly].every((flag) => typeof flag === "boolean");
}

/** `5d`, `23h` or `30m` until `endsAt`, in Unix seconds; `null` when it has no end or has passed. */
export function timeLeft(endsAt: number | null, now: number): string | null {
  if (endsAt === null || endsAt < now) return null;
  const left = endsAt - now;
  if (left >= 86_400) return `${Math.floor(left / 86_400)}d`;
  if (left >= 3_600) return `${Math.floor(left / 3_600)}h`;
  return `${Math.max(1, Math.floor(left / 60))}m`;
}

/** Whether a discount has run out. */
export function dealOver(discountPercent: number, dealEndsAt: number | null, now: number): boolean {
  return discountPercent > 0 && dealEndsAt !== null && dealEndsAt < now;
}

/** Whether an offer has not ended yet. */
export function isLive(endsAt: number | null, now: number): boolean {
  return endsAt === null || endsAt >= now;
}

export function reviewLabel(percent: number, count: number): string {
  return count === 0 ? "No reviews yet" : `${percent}% positive`;
}

export function reviewTone(percent: number, count: number): "good" | "mid" | "none" {
  if (count === 0) return "none";
  return percent < 75 ? "mid" : "good";
}

/** `list` and then whatever in `more` it does not have yet. */
export function appendUnique<T extends { appid: number }>(list: T[], more: T[]): T[] {
  const seen = new Set(list.map((item) => item.appid));
  const added = more.filter((item) => {
    if (seen.has(item.appid)) return false;
    seen.add(item.appid);
    return true;
  });
  return [...list, ...added];
}

export function updatedAgo(updatedAt: number | null, now: number): string {
  if (updatedAt === null) return "";
  const seconds = Math.max(0, now - updatedAt);
  if (seconds < 60) return "Updated just now";
  if (seconds < 3_600) return `Updated ${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86_400) return `Updated ${Math.floor(seconds / 3_600)} h ago`;
  return `Updated ${Math.floor(seconds / 86_400)} d ago`;
}

/** Numbers requests, so that a late answer to an older one can be dropped. */
export function createLatest(): { next(): number; isCurrent(id: number): boolean } {
  let current = 0;
  return {
    next: () => ++current,
    isCurrent: (id) => id === current,
  };
}
