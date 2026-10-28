// The folder tree as flat rows, for a list that draws only what is on screen. Pure functions, tested in Node (scripts/tests/disk-analyzer.test.mjs).
import { sizeOf, type NodeView, type SizeMode } from "./api";
/** The children of a folder loaded so far. */
export interface ChildPage {
  items: NodeView[];
  total: number;
  rest: { count: number; size: number; alloc: number } | null;
  loading: boolean;
}
export type Row =
  | { type: "node"; key: string; depth: number; node: NodeView; parent: number; parentSize: number; expanded: boolean }
  | { type: "more"; key: string; depth: number; parent: number; parentSize: number; count: number; size: number }
  | { type: "loading"; key: string; depth: number; parent: number };
/** The root, then every expanded folder's loaded children below it. */
export function flatten(root: NodeView | null, pages: ReadonlyMap<number, ChildPage>, expanded: ReadonlySet<number>, mode: SizeMode): Row[] {
  const rows: Row[] = [];
  if (!root) return rows;
  const visit = (node: NodeView, depth: number, parent: number, parentSize: number) => {
    const open = node.kind === "dir" && expanded.has(node.id);
    rows.push({ type: "node", key: `n${node.id}`, depth, node, parent, parentSize, expanded: open });
    if (!open) return;
    const page = pages.get(node.id);
    const size = sizeOf(node, mode);
    if (!page) {
      rows.push({ type: "loading", key: `l${node.id}`, depth: depth + 1, parent: node.id });
      return;
    }
    for (const child of page.items) visit(child, depth + 1, node.id, size);
    if (page.loading) rows.push({ type: "loading", key: `l${node.id}`, depth: depth + 1, parent: node.id });
    else if (page.rest) {
      rows.push({
        type: "more",
        key: `m${node.id}`,
        depth: depth + 1,
        parent: node.id,
        parentSize: size,
        count: page.rest.count,
        size: sizeOf(page.rest, mode),
      });
    }
  };
  visit(root, 0, -1, sizeOf(root, mode));
  return rows;
}
/** The rows to draw for a scroll position, with `buffer` more on each side. */
export function visibleRange(scrollTop: number, viewport: number, rowHeight: number, count: number, buffer = 8) {
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - buffer);
  const last = Math.min(count, Math.ceil((scrollTop + viewport) / rowHeight) + buffer);
  return { first, last };
}
/** A share of the parent, 0…1. */
export function share(size: number, parentSize: number): number {
  return parentSize > 0 ? Math.min(1, Math.max(0, size / parentSize)) : 0;
}
