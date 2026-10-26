import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";

// The same setup as the System Cleaner tests: the real page state, with only
// the native calls and the browser bits Node does not have replaced.
const storage = new Map();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  },
});
Object.defineProperty(globalThis, "isTauri", { configurable: true, value: true });

const hooks = registerHooks({
  resolve(specifier, context, next) {
    if (specifier === "svelte/reactivity") {
      return next(new URL("../../node_modules/svelte/src/reactivity/index-client.js", import.meta.url).href, context);
    }
    if (specifier.startsWith(".") && context.parentURL) {
      const base = new URL(specifier, context.parentURL);
      for (const suffix of [".ts", ".svelte.ts"]) {
        if (existsSync(fileURLToPath(base) + suffix)) return next(base.href + suffix, context);
      }
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (!url.endsWith(".svelte.ts")) return next(url, context);
    const source = transpileModule(readFileSync(fileURLToPath(url), "utf8"), {
      compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext },
    }).outputText;
    return {
      format: "module",
      source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code,
      shortCircuit: true,
    };
  },
});

const { squarify, worstRatio, layoutTreemap, hitTest, DEFAULT_LAYOUT } = await import("../../frontend/app/pages/disk-analyzer/treemap.ts");
const { CATEGORIES, categoryOf, categoryColor, extensionOf } = await import("../../frontend/app/pages/disk-analyzer/palette.ts");
const { flatten, visibleRange } = await import("../../frontend/app/pages/disk-analyzer/rows.ts");
const { diskState: state, recycleQuestion, KEY } = await import("../../frontend/app/pages/disk-analyzer/state.svelte.ts");
const { diskApi } = await import("../../frontend/app/pages/disk-analyzer/api.ts");
const { confirmState } = await import("../../frontend/lib/confirm.svelte.ts");
const { toast } = await import("../../frontend/lib/toast.svelte.ts");
hooks.deregister();

// ---------------------------------------------------------------------------
// The squarified layout

const area = (r) => r.w * r.h;
const EPS = 1e-6;

function overlaps(a, b) {
  return a.x + a.w > b.x + EPS && b.x + b.w > a.x + EPS && a.y + a.h > b.y + EPS && b.y + b.h > a.y + EPS;
}

/** The baseline: every item a full-height strip side by side. */
function sliceAndDice(values, rect) {
  const total = values.reduce((s, v) => s + v, 0);
  let x = rect.x;
  return values.map((v) => {
    const w = (rect.w * v) / total;
    const r = { x, y: rect.y, w, h: rect.h };
    x += w;
    return r;
  });
}

test("squarify fills the rectangle exactly, without overlaps, inside its bounds", () => {
  const rect = { x: 10, y: 20, w: 600, h: 400 };
  const values = [500, 433, 78, 25, 25, 7, 3, 3, 2, 1, 1, 1, 0.5];
  const rects = squarify(values, rect);
  assert.equal(rects.length, values.length);
  const total = values.reduce((s, v) => s + v, 0);
  const sum = rects.reduce((s, r) => s + area(r), 0);
  assert.ok(Math.abs(sum - area(rect)) < 1e-6 * area(rect), `areas add up: ${sum}`);
  rects.forEach((r, i) => {
    assert.ok(Math.abs(area(r) - (values[i] / total) * area(rect)) < 1e-6 * area(rect), `item ${i} has its share`);
    assert.ok(r.x >= rect.x - EPS && r.y >= rect.y - EPS, `item ${i} starts inside`);
    assert.ok(r.x + r.w <= rect.x + rect.w + 1e-6 && r.y + r.h <= rect.y + rect.h + 1e-6, `item ${i} ends inside`);
    for (let j = i + 1; j < rects.length; j++) assert.ok(!overlaps(r, rects[j]), `${i} and ${j} overlap`);
  });
});

test("squarify gives squarer rectangles than slice-and-dice", () => {
  // The paper's example: 6, 6, 4, 3, 2, 2, 1 in a 6 × 4 rectangle.
  const values = [6, 6, 4, 3, 2, 2, 1];
  const rect = { x: 0, y: 0, w: 6, h: 4 };
  const squarified = worstRatio(squarify(values, rect));
  const sliced = worstRatio(sliceAndDice(values, rect));
  assert.ok(squarified < sliced, `${squarified} < ${sliced}`);
  assert.ok(squarified < 3, `close to square: ${squarified}`);
});

test("squarify handles zero sizes, empty space and is deterministic", () => {
  const rect = { x: 0, y: 0, w: 100, h: 50 };
  const rects = squarify([0, 10, 0, 10, -3], rect);
  assert.deepEqual(rects.map(area).map(Math.round), [0, 2500, 0, 2500, 0]);
  assert.deepEqual(squarify([0, 0], rect).map(area), [0, 0], "nothing to show");
  assert.deepEqual(squarify([5, 1], { x: 0, y: 0, w: 0, h: 10 }).map(area), [0, 0], "no room");
  const values = Array.from({ length: 200 }, (_, i) => ((i * 7919) % 97) + 1);
  assert.deepEqual(squarify(values, rect), squarify(values, rect));
  // Equal values keep their order.
  const ties = squarify([3, 3, 3], rect);
  assert.ok(ties[0].x <= ties[1].x && ties[0].y <= ties[1].y);
});

test("the nested layout keeps children inside their folder and the hit test finds the deepest", () => {
  // 0 root: 1 folder (3 file, 4 file), 2 file
  const data = { parent: [-1, 0, 0, 1, 1], size: [100, 60, 40, 30, 30], kind: [0, 0, 1, 1, 1] };
  const layout = layoutTreemap(data, 400, 300);
  const rect = (i) => Array.from(layout.rects.subarray(i * 4, i * 4 + 4));
  const [fx, fy, fw, fh] = rect(1);
  for (const child of [3, 4]) {
    const [x, y, w, h] = rect(child);
    assert.ok(x >= fx && y >= fy && x + w <= fx + fw + EPS && y + h <= fy + fh + EPS, `${child} inside its folder`);
  }
  assert.ok(layout.header[1] > 0, "a big folder gets a header strip");
  assert.equal(layout.depth[3], 2);
  const [cx, cy, cw, ch] = rect(3);
  assert.equal(hitTest(layout, cx + cw / 2, cy + ch / 2), 3);
  assert.equal(hitTest(layout, fx + 1, fy + 1), 1, "the frame belongs to the folder");
  assert.equal(hitTest(layout, -5, -5), -1);
  // Too small to draw: left out.
  const tiny = layoutTreemap({ parent: [-1, 0, 0], size: [1000, 999, 1], kind: [0, 1, 1] }, 100, 100);
  assert.equal(tiny.rects[2 * 4 + 2], 0);
  assert.ok(!Array.from(tiny.order).includes(2));
});

test("tiles keep a gap between them and a frame from the edge", () => {
  const sizes = [100, 30, 25, 20, 15, 10];
  const data = { parent: [-1, 0, 0, 0, 0, 0], size: sizes, kind: [0, 1, 1, 1, 1, 1] };
  const { gap, frame } = DEFAULT_LAYOUT;
  const layout = layoutTreemap(data, 500, 300);
  const rects = [1, 2, 3, 4, 5].map((i) => {
    const [x, y, w, h] = layout.rects.subarray(i * 4, i * 4 + 4);
    return { x, y, w, h };
  });
  for (const r of rects) {
    assert.ok(r.w > 0, "drawn");
    assert.ok(r.x >= frame + gap / 2 - EPS && r.y >= frame + gap / 2 - EPS, "clear of the top left edge");
    assert.ok(r.x + r.w <= 500 - frame - gap / 2 + EPS && r.y + r.h <= 300 - frame - gap / 2 + EPS, "clear of the bottom right edge");
  }
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) {
      const a = rects[i];
      const b = rects[j];
      const apart = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w), b.y - (a.y + a.h), a.y - (b.y + b.h));
      assert.ok(apart >= gap - 1e-3, `${i + 1} and ${j + 1} are ${apart}px apart`);
    }
  }
});

test("each extension has one calm colour per kind, in both themes", () => {
  assert.equal(categoryOf("MP4"), "video");
  assert.equal(categoryOf("png"), "image");
  assert.equal(categoryOf("flac"), "audio");
  assert.equal(categoryOf("vhdx"), "archive");
  assert.equal(categoryOf("pak"), "app");
  assert.equal(categoryOf("pdf"), "document");
  assert.equal(categoryOf("rs"), "code");
  assert.equal(categoryOf("etl"), "system");
  assert.equal(categoryOf("xyz123"), "other");
  assert.equal(categoryOf(""), "other");
  assert.equal(extensionOf("Movie.Part1.MKV"), "mkv");
  assert.equal(extensionOf(".gitignore"), "", "a leading dot is not an extension");
  assert.equal(extensionOf("README"), "");
  for (const navy of [false, true]) {
    const colors = CATEGORIES.map((c) => categoryColor(c.id, navy));
    assert.ok(colors.every((c) => /^#[0-9a-f]{6}$/.test(c)), "hex colours");
    assert.equal(new Set(colors).size, colors.length, "no two kinds share a colour");
  }
});

test("rows follow what is open, with loading and 'smaller items' rows", () => {
  const node = (id, kind = "dir", size = 10) => ({ id, name: `n${id}`, kind, size, alloc: size, files: 0, dirs: 0, hasChildren: kind === "dir" });
  const root = node(0, "dir", 100);
  const pages = new Map([[0, { items: [node(1, "dir", 60), node(2, "file", 40)], total: 3, rest: { count: 1, size: 0, alloc: 0 }, loading: false }]]);
  const rows = flatten(root, pages, new Set([0, 1]), "size");
  assert.deepEqual(rows.map((r) => r.type), ["node", "node", "loading", "node", "more"]);
  assert.equal(rows[1].depth, 1);
  assert.equal(rows[1].parentSize, 100);
  assert.deepEqual(visibleRange(300, 600, 30, 100_000, 8), { first: 2, last: 38 });
});

// ---------------------------------------------------------------------------
// The page state, with the backend replaced

const view = (id, name, kind, size, alloc, more = {}) => ({
  id,
  name,
  kind,
  ext: kind === "file" ? name.split(".").pop() : "",
  size,
  alloc,
  files: kind === "dir" ? 0 : 1,
  dirs: 0,
  modified: 1_700_000_000,
  hasChildren: kind === "dir",
  protected: false,
  reason: null,
  caution: null,
  inaccessible: false,
  ...more,
});

let nodes;
let children;
const calls = [];
const original = { ...diskApi };

function backend() {
  nodes = {
    0: view(0, "D:\\", "dir", 1000, 2000, { files: 3, dirs: 1, protected: true, reason: "This is the folder you scanned." }),
    1: view(1, "Games", "dir", 800, 1600, { files: 1 }),
    2: view(2, "big.iso", "file", 150, 300),
    3: view(3, "a.txt", "file", 50, 100),
    4: view(4, "save.dat", "file", 800, 1600),
  };
  children = { 0: [1, 2, 3], 1: [4] };
  calls.length = 0;
  const log = (name, args) => calls.push({ name, args });
  diskApi.drives = async () => [
    { letter: "C", root: "C:\\", label: "Windows", filesystem: "NTFS", total: 500, free: 100, kind: "fixed", system: true, mftPossible: true },
    { letter: "D", root: "D:\\", label: "Data", filesystem: "NTFS", total: 4000, free: 1000, kind: "fixed", system: false, mftPossible: true },
  ];
  diskApi.cancel = async () => log("cancel", []);
  diskApi.nodes = async (scanId, ids) => {
    log("nodes", [scanId, ids]);
    return ids.map((id) => nodes[id]);
  };
  diskApi.trail = async (scanId, id) => {
    log("trail", [scanId, id]);
    const up = { 0: [0], 1: [0, 1], 2: [0, 2], 3: [0, 3], 4: [0, 1, 4] }[id];
    return up.map((n) => ({ id: n, name: nodes[n].name, size: nodes[n].size, alloc: nodes[n].alloc }));
  };
  diskApi.children = async (scanId, id, offset, limit, sort, sizeMode) => {
    log("children", [scanId, id, offset, limit, sort, sizeMode]);
    const ids = (children[id] ?? []).filter((c) => nodes[c]);
    return { items: ids.map((c) => nodes[c]), total: ids.length, rest: null };
  };
  diskApi.treemap = async (scanId, id, depth, minBytes, sizeMode) => {
    log("treemap", [scanId, id, depth, minBytes, sizeMode]);
    return { scanId, ids: [id], parent: [-1], size: [nodes[id].size], kind: [0], ext: [0], names: [nodes[id].name], exts: [""], ranks: [255], truncated: false };
  };
  diskApi.topFiles = async (scanId, limit, sizeMode, filter) => {
    log("topFiles", [scanId, limit, sizeMode, filter]);
    return [];
  };
  diskApi.extensions = async (scanId, id, sizeMode) => {
    log("extensions", [scanId, id, sizeMode]);
    return [];
  };
}

const done = { scanId: 3, rootId: 0, root: "D:\\", files: 3, dirs: 1, bytes: 1000, alloc: 2000, inaccessible: 0, inaccessiblePaths: [], elapsedMs: 1200, windowsDrive: false, memory: 1000 };
const settle = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));

async function scanned() {
  backend();
  diskApi.scan = async (root, onEvent) => {
    onEvent({ event: "progress", data: { files: 2, dirs: 1, bytes: 900, alloc: 1800, current: "D:\\Games" } });
    return done;
  };
  await state.load();
  state.choose("D:\\");
  state.setArea(800, 500);
  await state.scan();
  await settle();
}

test.beforeEach(() => {
  storage.clear();
  Object.assign(diskApi, original);
  state.phase = "idle";
  state.done = null;
  state.target = null;
  state.sizeMode = "alloc";
  state.view = "folders";
  state.sort = "size";
  state.skipped = [];
});
test.afterEach(() => {
  while (toast.visible.length) toast.dismiss(toast.visible[0].id);
  if (confirmState.current) confirmState.answer(false);
});

test("a scan shows its progress, then the results of the drive picked", async () => {
  backend();
  await state.load();
  assert.equal(state.target, "C:\\", "the Windows drive is picked first");
  state.choose("D:\\");
  const seen = [];
  diskApi.scan = async (root, onEvent) => {
    assert.equal(root, "D:\\", "only the root path is sent");
    assert.equal(state.phase, "scanning");
    onEvent({ event: "progress", data: { files: 2, dirs: 1, bytes: 900, alloc: 1500, current: "D:\\Games" } });
    seen.push({ ...state.progress, share: state.scanShare });
    return done;
  };
  state.setArea(800, 500);
  await state.scan();
  assert.deepEqual(seen, [{ files: 2, dirs: 1, bytes: 900, alloc: 1500, current: "D:\\Games", share: 0.5 }], "1.5 of 3 used");
  assert.equal(state.phase, "ready");
  assert.equal(state.progress, null);
  assert.equal(state.root.name, "D:\\");
  assert.deepEqual(state.trail.map((c) => c.id), [0]);
  assert.deepEqual(state.rows.map((r) => r.node?.name ?? r.type), ["D:\\", "Games", "big.iso", "a.txt"]);
  const map = calls.find((c) => c.name === "treemap");
  assert.deepEqual(map.args.slice(0, 3), [3, 0, 10]);
  assert.equal(map.args[3], Math.max(1, Math.floor((2000 * 9) / (800 * 500))), "small blocks merge below about 9 pixels");
  assert.equal(map.args[4], "alloc", "size on disk is the default");
});

test("cancelling a scan stops it and leaves nothing half-shown", async () => {
  backend();
  let release;
  diskApi.scan = () => new Promise((resolve) => (release = resolve));
  state.choose("D:\\");
  const running = state.scan();
  assert.equal(state.phase, "scanning");
  state.cancel();
  assert.ok(calls.some((c) => c.name === "cancel"));
  release(null);
  await running;
  assert.equal(state.phase, "idle");
  assert.equal(state.done, null);
  assert.equal(toast.visible[0].message, "Scan cancelled.");
});

test("the size toggle reloads the list and the treemap in the other size, and is kept", async () => {
  await scanned();
  calls.length = 0;
  await state.setSizeMode("size");
  assert.equal(storage.get(KEY.sizeMode), JSON.stringify("size"));
  assert.ok(calls.some((c) => c.name === "children" && c.args[5] === "size"));
  assert.ok(calls.some((c) => c.name === "treemap" && c.args[4] === "size"));
  assert.equal(state.rows[0].parentSize, 1000, "shares follow the size picked");
});

test("zooming in and out moves the treemap and its breadcrumbs", async () => {
  await scanned();
  calls.length = 0;
  await state.zoomTo(1);
  assert.equal(state.current, 1);
  assert.deepEqual(state.trail.map((c) => c.name), ["D:\\", "Games"]);
  assert.ok(calls.some((c) => c.name === "treemap" && c.args[1] === 1));
  state.zoomOut();
  await settle();
  assert.equal(state.current, 0);
  assert.deepEqual(state.trail.map((c) => c.id), [0]);
});

test("selecting: one, Ctrl adds, Shift takes the rows in between", async () => {
  await scanned();
  state.select(1);
  assert.deepEqual([...state.selection], [1]);
  state.select(3, "range");
  assert.deepEqual([...state.selection].sort(), [1, 2, 3]);
  state.select(2, "toggle");
  assert.deepEqual([...state.selection].sort(), [1, 3]);
  state.select(2, "toggle");
  assert.deepEqual([...state.selection].sort(), [1, 2, 3]);
  // Ctrl moved the anchor, as in Explorer: Shift now runs from there.
  state.select(3, "range");
  assert.deepEqual([...state.selection].sort(), [2, 3]);
  state.select(4294967295);
  assert.deepEqual([...state.selection].sort(), [2, 3], "a 'smaller items' block is not an item");
});

test("moving to the Recycle Bin asks first, in danger style, then shows the new totals", async () => {
  await scanned();
  state.select(1);
  state.select(2, "toggle");
  state.toggle(1, true);
  await settle();
  let sent = null;
  diskApi.recycle = async (scanId, ids, onEvent) => {
    sent = { scanId, ids };
    onEvent({ event: "progress", data: { done: 0, total: 2, current: "Games" } });
    // What the backend does: Games goes, big.iso is too big for the bin.
    delete nodes[1];
    delete nodes[4];
    nodes[0] = { ...nodes[0], size: 200, alloc: 400, files: 2, dirs: 0 };
    return { recycled: 1, ids: [1], freedBytes: 1600, freedSize: 800, skipped: [{ path: "D:\\big.iso", reason: "Too big for the Recycle Bin, or its drive has none, so it was left alone." }] };
  };
  const running = state.recycle();
  await settle(10);
  const asked = confirmState.current;
  assert.ok(asked, "nothing moves without asking");
  assert.equal(asked.title, "Move to Recycle Bin?");
  assert.equal(asked.confirmLabel, "Move to Recycle Bin");
  assert.equal(asked.danger, true);
  assert.equal(asked.message, "2 items, 1.9 KB. You can restore them from the Recycle Bin.");
  assert.equal(sent, null);
  confirmState.answer(true);
  await running;
  await settle();
  assert.deepEqual(sent, { scanId: 3, ids: [1, 2] });
  assert.equal(state.phase, "ready");
  assert.deepEqual([...state.selection], [2], "what went is no longer selected");
  assert.equal(state.expanded.has(1), false);
  assert.equal(state.root.alloc, 400, "the totals come from the backend, no rescan");
  assert.deepEqual(state.rows.map((r) => r.node?.name ?? r.type), ["D:\\", "big.iso", "a.txt"]);
  assert.equal(state.skipped.length, 1);
  assert.match(toast.visible[0].message, /^Moved 1 item to the Recycle Bin · 1\.6 KB freed\.$/);
});

test("declining keeps everything, and protected items are never offered", async () => {
  await scanned();
  let moved = false;
  diskApi.recycle = async () => {
    moved = true;
    return { recycled: 0, ids: [], freedBytes: 0, freedSize: 0, skipped: [] };
  };
  const running = state.recycle([3]);
  await settle(10);
  confirmState.answer(false);
  await running;
  assert.equal(moved, false);
  // The scanned folder itself: refused before asking.
  await state.recycle([0]);
  assert.equal(confirmState.current, null);
  assert.equal(moved, false);
  assert.match(toast.visible.at(-1).message, /stays where it is/);
});

test("the question says plainly when a lot would move", () => {
  const big = view(9, "Videos", "dir", 12 * 1024 ** 3, 12 * 1024 ** 3, { files: 4321, caution: "Save games of Skyrim. Back them up in Game Saves first." });
  const cloud = view(10, "doc.pdf", "cloud", 10, 0);
  const question = recycleQuestion([big, cloud], 2, "size");
  assert.equal(question.danger, true);
  const lines = question.message.split("\n\n");
  assert.equal(lines[0], "2 items, 12.0 GB. You can restore them from the Recycle Bin.");
  assert.ok(lines.some((l) => l.startsWith("That is more than 10 GB.")));
  assert.ok(lines.includes("That is 4,322 files."));
  assert.ok(lines.includes(["Think twice:", "• Videos: Save games of Skyrim. Back them up in Game Saves first."].join("\n")));
  assert.ok(lines.includes("Cloud files are removed from your cloud storage too."));
  assert.ok(lines.includes("2 protected items stay where they are."));
  const one = recycleQuestion([view(3, "a.txt", "file", 50, 4096)], 0, "alloc");
  assert.equal(one.message, "1 item, 4.0 KB. You can restore it from the Recycle Bin.");
});

test("tabs load their lists when first opened and the view is kept", async () => {
  await scanned();
  state.setView("files");
  state.setView("types");
  await settle();
  assert.equal(storage.get(KEY.view), JSON.stringify("types"));
  assert.ok(calls.some((c) => c.name === "topFiles" && c.args[1] === 1000 && c.args[3] === null));
  assert.ok(calls.some((c) => c.name === "extensions" && c.args[1] === 0));
});
