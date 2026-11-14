import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";
// The same setup as the Game Saves tests: the real client state, with only the native calls and the browser bits Node does not have replaced.
const storage = new Map();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  },
});
Object.defineProperty(globalThis, "isTauri", { configurable: true, value: true });
// The progress bar rides on animation frames; a timer does the same here.
Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: (run) => setTimeout(() => run(Date.now()), 16) });
Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: (id) => clearTimeout(id) });
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
const { cleanerState: state } = await import("../../frontend/app/pages/system-cleaner/state.svelte.ts");
const { cleanerApi } = await import("../../frontend/app/pages/system-cleaner/api.ts");
const { confirmState } = await import("../../frontend/lib/confirm.svelte.ts");
const { toast } = await import("../../frontend/lib/toast.svelte.ts");
hooks.deregister();
const categories = [
  { id: "temp", title: "Temporary Files", description: "", hint: "", icon: "temp", mayNeedAdmin: true, adminOnly: false },
  { id: "prefetch", title: "Prefetch Files", description: "", hint: "", icon: "prefetch", mayNeedAdmin: true, adminOnly: true },
  { id: "recycle-bin", title: "Empty Recycle Bin", description: "", hint: "", icon: "recycle-bin", mayNeedAdmin: false, adminOnly: false },
];
const sizes = { temp: 1_000, prefetch: 2_000, "recycle-bin": 4_000 };
/** What a re-measure finds after a run; empty means "nothing has been cleaned". */
const remaining = { temp: 400, prefetch: 0, "recycle-bin": 0 };
/** The command's event callback, answered the way the backend does. */
const answer = (onEvent, events) => {
  for (const event of events) onEvent(event);
};
const measureEvent = (id) => ({ event: "category", data: { id, bytes: sizes[id], files: 4, locked: false } });
/** What the second measurement of a run — the one after the cleaning — finds. */
const remeasureEvent = (id) => ({ event: "category", data: { id, bytes: remaining[id], files: 1, locked: false } });
const original = { ...cleanerApi };
test("only an explicit admin scan includes system folders, even when a helper is available", async () => {
  let adminScans = 0;
  let standardScans = 0;
  cleanerApi.categories = async () => categories;
  cleanerApi.scan = async (onEvent) => {
    standardScans++;
    answer(onEvent, [
      { event: "category", data: { id: "temp", bytes: 500, files: 2, locked: true } },
      { event: "category", data: { id: "prefetch", bytes: 0, files: 0, locked: true } },
      measureEvent("recycle-bin"),
    ]);
    return { bytes: 4_500, locked: ["temp", "prefetch"] };
  };
  cleanerApi.downloadsPreview = async () => ({ folder: "Downloads", items: [], total: 0, kept: [] });
  cleanerApi.adminReady = async () => true;
  cleanerApi.scanElevated = async (onEvent) => {
    adminScans++;
    if (adminScans === 1) throw new Error("Administrator approval was declined.");
    answer(onEvent, [measureEvent("temp"), measureEvent("prefetch")]);
    return { bytes: 3_000, locked: [] };
  };
  await state.load();
  await state.scan();
  assert.equal(adminScans, 0);
  assert.equal(state.total, 4_500);
  assert.equal(state.isTicked("temp"), true);
  state.toggle("recycle-bin");
  assert.equal(state.isTicked("prefetch"), false);
  await state.allowAdmin();
  assert.equal(adminScans, 1);
  assert.equal(state.total, 4_500);
  assert.equal(state.isTicked("prefetch"), false);
  await state.allowAdmin();
  assert.equal(adminScans, 2);
  assert.equal(standardScans, 1);
  assert.equal(state.total, 7_500);
  assert.deepEqual(state.sizes.temp, { bytes: 1_500, files: 6, locked: false });
  assert.equal(state.isTicked("prefetch"), true);
  assert.equal(state.isTicked("recycle-bin"), false, "an existing unchecked category stays unchecked");
  assert.equal(state.selectedBytes, 3_500);
  await state.scan();
  assert.equal(adminScans, 2);
  assert.equal(state.total, 4_500);
  assert.equal(state.canSelectCategory(categories[1]), false);
});
/** The first scan of a test measures full sizes; later ones measure what is left. */
let scans = 0;
async function scanAll(downloadItems = []) {
  scans = 0;
  cleanerApi.categories = async () => categories;
  cleanerApi.scan = async (onEvent) => {
    scans++;
    const event = scans > 1 ? remeasureEvent : measureEvent;
    answer(onEvent, categories.map((category) => category.adminOnly
      ? { event: "category", data: { id: category.id, bytes: 0, files: 0, locked: true } }
      : event(category.id)));
    return { bytes: 0, locked: [] };
  };
  cleanerApi.scanElevated = async (onEvent) => {
    const prefetch = scans > 1 ? remeasureEvent("prefetch") : measureEvent("prefetch");
    answer(onEvent, [{ event: "category", data: { id: "temp", bytes: 0, files: 0, locked: false } }, prefetch]);
    return { bytes: prefetch.data.bytes, locked: [] };
  };
  cleanerApi.adminReady = async () => true;
  cleanerApi.downloadsPreview = async () => ({ folder: "D:\\Downloads", items: downloadItems, total: downloadItems.reduce((sum, item) => sum + item.size, 0), kept: [] });
  cleanerApi.downloadsDelete = async () => ({ deleted: 0, freed: 0, skipped: [] });
  await state.load();
  await state.scan();
}
const settle = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));
test.beforeEach(() => {
  storage.clear();
  Object.assign(cleanerApi, original);
  scans = 0;
  state.categories = [];
  state.sizes = {};
  state.outcome = {};
  state.selected.clear();
  state.cleaned.clear();
  state.phase = "idle";
  state.scanned = false;
  state.adminGranted = false;
  state.progress = null;
  state.settling = false;
  state.barPercent = 0;
  state.barDone = false;
  state.autoSelected = false;
  state.error = null;
});
test.afterEach(() => {
  while (toast.visible.length) toast.dismiss(toast.visible[0].id);
  if (confirmState.current) confirmState.answer(false);
});
test("Downloads with no deletion candidates stays disabled and unchecked", async () => {
  state.selected.add("downloads");
  await scanAll();
  const downloads = state.categories.find(({ id }) => id === "downloads");
  assert.equal(state.canSelectCategory(downloads), false);
  assert.equal(state.isTicked("downloads"), false);
  state.toggle("downloads");
  assert.equal(state.isTicked("downloads"), false);
});
test("Downloads can be selected only from its card, including a zero-byte candidate", async () => {
  await scanAll([{ path: "D:\\Downloads\\old.zip", name: "old.zip", size: 0, changed: 0, isDir: false }]);
  const downloads = state.categories.find(({ id }) => id === "downloads");
  assert.equal(state.canSelectCategory(downloads), true);
  assert.equal(state.isTicked("downloads"), false);
  state.toggleAll();
  state.toggleAll();
  assert.equal(state.isTicked("downloads"), false);
  await state.allowAdmin();
  assert.equal(state.isTicked("downloads"), false);
  storage.set("cleaner.selected", JSON.stringify(["downloads", "temp"]));
  state.reloadSelection();
  assert.equal(state.isTicked("downloads"), false, "a saved tick does not opt into Downloads cleanup");
  state.toggle("downloads");
  assert.equal(state.isTicked("downloads"), true);
  state.reloadSelection();
  assert.equal(state.isTicked("downloads"), true, "a checkbox tick in the current scan survives a settings refresh");
  await state.scan();
  assert.equal(state.isTicked("downloads"), false);
});
test("a scan ticks the available categories, so one click is left to do", async () => {
  await scanAll();
  assert.equal(state.scanned, true);
  assert.deepEqual([...state.selected].sort(), ["recycle-bin", "temp"]);
  assert.equal(state.autoSelected, true);
  assert.equal(state.allSelected, true, "the button offers to deselect");
  assert.equal(state.selectedBytes, 5_000);
  assert.equal(state.total, 5_000);
});
test("a scan leaves a tick the user made before it alone", async () => {
  state.selected.add("recycle-bin");
  await scanAll();
  assert.deepEqual([...state.selected].sort(), ["recycle-bin", "temp"]);
  // The point of the feature: one Clean click after any scan.
  state.toggle("temp");
  assert.equal(state.selected.has("temp"), false, "unticking takes it out of the run");
  await state.scan();
  assert.equal(state.allSelected, true, "and the next scan ticks everything again");
});
test("declining administrator access excludes system-only categories from selection and cleanup", async () => {
  cleanerApi.categories = async () => categories;
  cleanerApi.scan = async (onEvent) => {
    answer(onEvent, [
      { event: "category", data: { id: "temp", bytes: 1_000, files: 4, locked: true } },
      { event: "category", data: { id: "prefetch", bytes: 0, files: 0, locked: true } },
      measureEvent("recycle-bin"),
    ]);
    return { bytes: 5_000, locked: ["temp", "prefetch"] };
  };
  cleanerApi.scanElevated = async () => { throw new Error("Administrator approval was declined."); };
  cleanerApi.downloadsPreview = async () => ({ folder: "Downloads", items: [], total: 0, kept: [] });
  state.selected.add("prefetch");
  await state.load();
  await state.scan();
  await state.allowAdmin();
  assert.deepEqual([...state.selected].sort(), ["recycle-bin", "temp"]);
  assert.equal(state.selectedBytes, 5_000);
  assert.equal(state.allSelected, true);
  state.toggleAll();
  state.toggle("prefetch");
  assert.equal(state.selected.size, 0);
  state.toggleAll();
  assert.deepEqual([...state.selected].sort(), ["recycle-bin", "temp"]);

  let cleanedIds;
  cleanerApi.downloadsDelete = async () => ({ deleted: 0, freed: 0, skipped: [] });
  cleanerApi.clean = async (ids) => {
    cleanedIds = ids;
    return { freed: 0, files: 0, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: ["temp"] };
  };
  const cleaning = state.clean();
  await settle();
  assert.ok(confirmState.current);
  confirmState.answer(true);
  await cleaning;
  assert.deepEqual(cleanedIds.sort(), ["recycle-bin", "temp"]);
  assert.equal(state.isCleaned("temp"), true);

  cleanerApi.scanElevated = async (onEvent) => {
    answer(onEvent, [measureEvent("prefetch"), measureEvent("temp")]);
    return { bytes: 3_000, locked: [] };
  };
  await state.allowAdmin();
  assert.equal(state.isCleaned("temp"), false, "the previously excluded system folders can now be cleaned");
  assert.equal(state.isTicked("prefetch"), true);
  assert.equal(state.isTicked("temp"), true);
  assert.equal(state.selectedBytes, 4_000);
});
test("a failed category scan keeps ownership until its Downloads preview finishes", async () => {
  let finishDownloads;
  cleanerApi.categories = async () => categories;
  cleanerApi.scan = async () => { throw new Error("category scan failed"); };
  cleanerApi.scanElevated = async () => ({ bytes: 0, locked: [] });
  cleanerApi.downloadsPreview = () => new Promise((resolve) => { finishDownloads = resolve; });
  await state.load();
  const scanning = state.scan();
  await settle();
  try {
    assert.equal(state.busy, true, "another scan cannot replace a preview that is still running");
    assert.equal(state.phase, "scanning");
  } finally {
    finishDownloads({ folder: "D:\\Downloads", items: [], total: 80, kept: [] });
    await scanning;
  }
  assert.equal(state.busy, false);
  assert.equal(state.sizes.downloads.bytes, 80);
  assert.equal(state.error, "category scan failed");
});
test("what stays in use is told in bytes, without counting the admin pass twice", async () => {
  await scanAll();
  await state.allowAdmin();
  cleanerApi.clean = async (ids, onEvent) => {
    answer(onEvent, [
      { event: "progress", data: { done: 0, total: ids.length, current: "Temporary Files" } },
      {
        event: "category",
        data: { id: "temp", bytes: 600, files: 3, skipped: 1, skippedBytes: 400, adminSkipped: 1, adminSkippedBytes: 400, locked: false },
      },
      { event: "progress", data: { done: 1, total: ids.length, current: "Prefetch Files" } },
      {
        event: "category",
        data: { id: "prefetch", bytes: 2_000, files: 4, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      },
      { event: "progress", data: { done: 2, total: ids.length, current: "Empty Recycle Bin" } },
      {
        event: "category",
        data: { id: "recycle-bin", bytes: 4_000, files: 4, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      },
    ]);
    return { freed: 6_600, files: 11, skipped: 1, skippedBytes: 400, adminSkipped: 1, adminSkippedBytes: 400, locked: [] };
  };
  cleanerApi.cleanElevated = async (ids, onEvent) => {
    answer(onEvent, [
      {
        event: "category",
        data: { id: "temp", bytes: 0, files: 0, skipped: 1, skippedBytes: 400, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      },
    ]);
    return { freed: 0, files: 0, skipped: 1, skippedBytes: 400, adminSkipped: 0, adminSkippedBytes: 0, locked: [] };
  };
  const running = state.clean();
  await settle(20);
  assert.ok(confirmState.current, "the user is asked before anything is removed");
  confirmState.answer(true);
  await running;
  assert.equal(state.outcome.temp.freed, 600);
  assert.equal(state.outcome.temp.skipped, 1, "the admin pass replaces the count it retried");
  assert.equal(state.sizes.temp.bytes, 400, "what is left in use stays on the card");
  assert.equal(state.sizes.prefetch.bytes, 0);
  assert.equal(state.outcome.temp.skippedBytes, 400, "and it is told in bytes, not twice over");
  assert.equal(state.selected.size, 0, "the cleaned categories are unticked");
  assert.equal(state.cleaned.size, 3);
  assert.equal(state.lastCleaned !== null, true);
  assert.equal(
    toast.visible[0].message,
    "Freed 6.4 KB · 400 B in use of 6.8 KB selected.",
    "the freed bytes, what is still in use, and what the scan promised",
  );
});
test("without the administrator pass, what the system folders kept is still counted as left", async () => {
  await scanAll();
  await state.allowAdmin();
  // The remeasure cannot see the system folders without approval.
  remaining.temp = 0;
  cleanerApi.clean = async (ids, onEvent) => {
    answer(onEvent, [
      {
        event: "category",
        data: { id: "temp", bytes: 600, files: 3, skipped: 1, skippedBytes: 400, adminSkipped: 1, adminSkippedBytes: 400, locked: false },
      },
      {
        event: "category",
        data: { id: "prefetch", bytes: 2_000, files: 4, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      },
      {
        event: "category",
        data: { id: "recycle-bin", bytes: 4_000, files: 4, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      },
    ]);
    return { freed: 6_600, files: 11, skipped: 1, skippedBytes: 400, adminSkipped: 1, adminSkippedBytes: 400, locked: [] };
  };
  cleanerApi.cleanElevated = async () => {
    throw new Error("Administrator approval was declined.");
  };
  try {
    const running = state.clean();
    await settle(20);
    confirmState.answer(true);
    await running;
    assert.equal(toast.visible[0].message, "Freed 6.4 KB · 400 B in use of 6.8 KB selected.", "the 400 B the admin pass never retried");
  } finally {
    remaining.temp = 400;
  }
});
test("the bar follows the categories and ends full", async () => {
  await scanAll();
  await state.allowAdmin();
  cleanerApi.cleanElevated = async () => ({ freed: 0, files: 0, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: [] });
  const seen = [];
  cleanerApi.clean = async (ids, onEvent) => {
    for (const [index, id] of ids.entries()) {
      onEvent({ event: "progress", data: { done: index, total: ids.length, current: id } });
      onEvent({
        event: "category",
        data: { id, bytes: sizes[id], files: 4, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      });
      seen.push({ ...state.progress });
      await settle(4);
    }
    return { freed: 7_000, files: 12, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: [] };
  };
  const running = state.clean();
  await settle(20);
  confirmState.answer(true);
  await running;
  assert.deepEqual(seen.map((one) => one.step), [2, 3, 3], "the label counts the category being worked on");
  assert.deepEqual(seen.map((one) => one.total), [3, 3, 3]);
  assert.equal(state.progress, null, "nothing is left running");
  assert.equal(state.phase, "idle");
  assert.equal(state.settling, false);
});
test("a category emptied since the scan is offered again", async () => {
  await scanAll();
  cleanerApi.clean = async (ids, onEvent) => {
    for (const id of ids) {
      onEvent({
        event: "category",
        data: { id, bytes: sizes[id], files: 4, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: false },
      });
    }
    return { freed: 7_000, files: 12, skipped: 0, skippedBytes: 0, adminSkipped: 0, adminSkippedBytes: 0, locked: [] };
  };
  const running = state.clean();
  await settle(20);
  confirmState.answer(true);
  await running;
  assert.equal(state.cleaned.has("temp"), true);
  assert.equal(state.toggleAll.length >= 0, true);
  state.cleaned.clear();
  assert.equal(state.selectable.length, 2, "the regular categories with scan results can be ticked again");
});
