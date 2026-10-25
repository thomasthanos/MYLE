// The treemap's layout, as pure functions (no DOM), so Node can test it.
//
// `squarify` is the squarified treemap of Bruls, Huijsen and van Wijk
// (2000): items go largest first into rows along the shorter side of the
// space left, and a row is closed as soon as adding the next item would make
// its worst aspect ratio worse. `layoutTreemap` nests it: each folder's
// children share the folder's rectangle, less a thin frame and, when it is
// big enough to label, a header strip.

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The worst aspect ratio of a row of `sum` total area, `min` and `max` its
 *  smallest and largest item, laid along a side of length `side`. */
function worst(sum: number, min: number, max: number, side: number): number {
  const s2 = sum * sum;
  const w2 = side * side;
  return Math.max((w2 * max) / s2, s2 / (w2 * min));
}

/** Lays one finished row along the shorter side of `free`; returns what is left. */
function placeRow(row: number[], areas: number[], rowArea: number, free: Rect, out: Rect[]): Rect {
  if (free.w >= free.h) {
    // A column on the left, as tall as the space.
    const width = free.h > 0 ? rowArea / free.h : 0;
    let y = free.y;
    for (const i of row) {
      const h = width > 0 ? areas[i] / width : 0;
      out[i] = { x: free.x, y, w: width, h };
      y += h;
    }
    return { x: free.x + width, y: free.y, w: Math.max(0, free.w - width), h: free.h };
  }
  // A row along the top, as wide as the space.
  const height = free.w > 0 ? rowArea / free.w : 0;
  let x = free.x;
  for (const i of row) {
    const w = height > 0 ? areas[i] / height : 0;
    out[i] = { x, y: free.y, w, h: height };
    x += w;
  }
  return { x: free.x, y: free.y + height, w: free.w, h: Math.max(0, free.h - height) };
}

/** One rectangle per value, in the order given, filling `rect` in
 *  proportion to the values. Zero and negative values get an empty
 *  rectangle at the corner. The same input always gives the same output. */
export function squarify(values: readonly number[], rect: Rect): Rect[] {
  const out: Rect[] = values.map(() => ({ x: rect.x, y: rect.y, w: 0, h: 0 }));
  const total = values.reduce((sum, v) => sum + (v > 0 ? v : 0), 0);
  if (total <= 0 || rect.w <= 0 || rect.h <= 0) return out;
  const scale = (rect.w * rect.h) / total;
  const areas = values.map((v) => (v > 0 ? v * scale : 0));
  // Largest first; equal values keep their order.
  const order = values
    .map((_, i) => i)
    .filter((i) => values[i] > 0)
    .sort((a, b) => values[b] - values[a] || a - b);
  let free = { ...rect };
  let row: number[] = [];
  let rowArea = 0;
  let rowMin = Infinity;
  let rowMax = 0;
  for (let k = 0; k < order.length; ) {
    const i = order[k];
    const area = areas[i];
    const side = Math.min(free.w, free.h);
    if (
      row.length === 0 ||
      worst(rowArea + area, Math.min(rowMin, area), Math.max(rowMax, area), side) <= worst(rowArea, rowMin, rowMax, side)
    ) {
      row.push(i);
      rowArea += area;
      rowMin = Math.min(rowMin, area);
      rowMax = Math.max(rowMax, area);
      k++;
      continue;
    }
    free = placeRow(row, areas, rowArea, free, out);
    row = [];
    rowArea = 0;
    rowMin = Infinity;
    rowMax = 0;
  }
  if (row.length) placeRow(row, areas, rowArea, free, out);
  return out;
}

/** The worst aspect ratio (≥ 1) among rectangles with an area. */
export function worstRatio(rects: readonly Rect[]): number {
  let worstSoFar = 1;
  for (const r of rects) {
    if (r.w <= 0 || r.h <= 0) continue;
    worstSoFar = Math.max(worstSoFar, r.w / r.h, r.h / r.w);
  }
  return worstSoFar;
}

/** What the layout needs of a treemap payload (see api.ts). */
export interface TreeData {
  parent: readonly number[];
  size: readonly number[];
  kind: readonly number[];
}

export const FOLDER = 0;
export const SMALLER = 4;

export interface LayoutOptions {
  /** The frame around a folder's children. */
  frame: number;
  /** A folder's header strip, when the folder is at least `labelW` × `labelH`. */
  header: number;
  labelW: number;
  labelH: number;
  /** Rectangles narrower or lower than this are not drawn. */
  minSide: number;
}

export const DEFAULT_LAYOUT: LayoutOptions = { frame: 2, header: 17, labelW: 64, labelH: 40, minSide: 3 };

export interface Layout {
  /** x, y, w, h for each payload index; w = 0 when it is not drawn. */
  rects: Float32Array;
  /** The header strip of each folder drawn with one (0: none). */
  header: Float32Array;
  /** How deep below the shown folder (its children are 1). */
  depth: Uint8Array;
  /** Drawn indexes, every folder before what is inside it. */
  order: Int32Array;
}

/** Nests the payload into `width` × `height`. Index 0, the folder shown,
 *  fills the whole area without a frame of its own. */
export function layoutTreemap(data: TreeData, width: number, height: number, options: LayoutOptions = DEFAULT_LAYOUT): Layout {
  const count = data.parent.length;
  const rects = new Float32Array(count * 4);
  const header = new Float32Array(count);
  const depth = new Uint8Array(count);
  const order: number[] = [];
  const children: number[][] = Array.from({ length: count }, () => []);
  for (let i = 1; i < count; i++) {
    const p = data.parent[i];
    if (p >= 0 && p < count) children[p].push(i);
  }
  if (count === 0 || width <= 0 || height <= 0) return { rects, header, depth, order: new Int32Array(0) };
  rects.set([0, 0, width, height], 0);
  const queue = [0];
  for (let q = 0; q < queue.length; q++) {
    const i = queue[q];
    const [x, y, w, h] = rects.subarray(i * 4, i * 4 + 4);
    if (i !== 0) order.push(i);
    const kids = children[i];
    if (!kids.length) continue;
    let inner: Rect;
    if (i === 0) {
      inner = { x, y, w, h };
    } else {
      const strip = w >= options.labelW && h >= options.labelH ? options.header : 0;
      header[i] = strip;
      inner = { x: x + options.frame, y: y + options.frame + strip, w: w - options.frame * 2, h: h - options.frame * 2 - strip };
      if (inner.w < options.minSide || inner.h < options.minSide) continue;
    }
    const placed = squarify(
      kids.map((k) => data.size[k]),
      inner,
    );
    kids.forEach((k, n) => {
      const r = placed[n];
      if (r.w < options.minSide || r.h < options.minSide) return;
      rects.set([r.x, r.y, r.w, r.h], k * 4);
      depth[k] = depth[i] + 1;
      queue.push(k);
    });
  }
  return { rects, header, depth, order: Int32Array.from(order) };
}

/** The deepest drawn item under (x, y), or -1. */
export function hitTest(layout: Layout, x: number, y: number): number {
  const { rects, order } = layout;
  for (let n = order.length - 1; n >= 0; n--) {
    const i = order[n];
    const rx = rects[i * 4];
    const ry = rects[i * 4 + 1];
    if (x >= rx && y >= ry && x < rx + rects[i * 4 + 2] && y < ry + rects[i * 4 + 3]) return i;
  }
  return -1;
}
