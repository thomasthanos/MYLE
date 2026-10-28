import type { Finding, HealthAction } from "./health.svelte";
export const CLEANUP_MODULES = new Set(["leftovers", "broken", "redundant", "installers"]);
export type ListFilter = "all" | "recommended" | "review" | "failed";
export interface BatchItem { id: string; action: HealthAction }
export function filterFindings(rows: Finding[], options: {
  category: string; query: string; filter: ListFilter; sort: "name" | "size"; failures: Record<string, string>;
}): Finding[] {
  const query = options.query.trim().toLocaleLowerCase();
  return rows.filter((f) => {
    const category = options.category === "cleanup" ? CLEANUP_MODULES.has(f.module)
      : options.category === "controls" ? !CLEANUP_MODULES.has(f.module) : f.module === options.category;
    return category && (!query || `${f.title} ${f.path ?? ""}`.toLocaleLowerCase().includes(query))
      && (options.filter === "all" || options.filter === "recommended" && f.recommended
        || options.filter === "review" && !f.recommended || options.filter === "failed" && !!options.failures[f.id]);
  }).sort((a, b) => options.sort === "size" ? (b.size ?? 0) - (a.size ?? 0) || a.title.localeCompare(b.title) : a.title.localeCompare(b.title));
}
export function selectResults(selected: Set<string>, rows: Finding[]): Set<string> {
  const next = new Set(selected);
  for (const f of rows) if (f.actions.length) next.add(f.id);
  return next;
}
export function cleanupPlan(rows: Finding[], selected: Set<string>, overrides: Record<string, HealthAction>): BatchItem[] {
  return rows.filter((f) => selected.has(f.id) && CLEANUP_MODULES.has(f.module) && f.cleanupAction).map((f) => ({
    id: f.id,
    action: overrides[f.id] && f.actions.includes(overrides[f.id]) ? overrides[f.id] : f.cleanupAction!,
  }));
}
export function compatibleActions(rows: Finding[]): { action: HealthAction; count: number }[] {
  const counts = new Map<HealthAction, number>();
  for (const f of rows) for (const action of f.actions) counts.set(action, (counts.get(action) ?? 0) + 1);
  return [...counts].map(([action, count]) => ({ action, count }));
}
