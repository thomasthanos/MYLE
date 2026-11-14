import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";
import { compileModule } from "svelte/compiler";
import { autoUpdateCards } from "../../frontend/app/pages/install-apps/updates.ts";
const source = readFileSync(new URL("../../frontend/app/pages/install-apps/state.svelte.ts", import.meta.url), "utf8");
const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, {
  compilerOptions: { target: ScriptTarget.ESNext, module },
}).outputText;
// The page state, with the vendored modules stubbed: the real check/selection logic runs.
const code = transpile(compileModule(transpile(source.replace(/^import[\s\S]*?;\r?\n/gm, "")) + "\nglobalThis.appsState = appsState;", {
  filename: "install-apps-test.svelte.js",
  generate: "client",
}).js.code, ModuleKind.CommonJS);
const require = createRequire(import.meta.url);
const APPS = [
  { id: "Spotify.Spotify", name: "Spotify", category: "Media", selfUpdating: true },
  { id: "Discord.Discord", name: "Discord", category: "Communication" },
];
const catalog = { packs: [], apps: APPS };
/** A check report with the same shape `apps_installed` returns. */
function report(updateCount = 20) {
  const updates = [
    { id: "Spotify.Spotify", name: "Spotify", version: "Unknown", available: "1.2.80.4" },
    { id: "Discord.Discord", name: "Discord", version: "1.0.9258", available: "1.0.9259" },
    ...Array.from({ length: updateCount - 2 }, (_, index) => ({
      id: `Vendor.App${index}`,
      name: `Vendor App ${index}`,
      version: "1.0",
      available: "2.0",
    })),
  ];
  return {
    updates,
    winget: updates.map((u) => ({ id: u.id, name: u.name, version: u.version, available: u.available })),
    wingetError: null,
    custom: [],
  };
}
function harness(initial = report()) {
  const current = { report: initial };
  const installs = [];
  const context = vm.createContext({
    require, exports: {}, console,
    isTauri: () => true,
    catalog,
    categorize: () => "Others",
    CATEGORIES: [],
    isCategory: () => false,
    autoUpdateCards,
    readJson: (_key, fallback) => fallback,
    writeJson: () => {},
    toast: { info() {}, error() {}, success() {} },
    confirm: async () => true,
    nav: { current: "install-apps" },
    operationGate: { lockedFor: () => false, begin: () => true, end: () => {} },
    whenShown: async () => {},
    SvelteSet: Set,
    setTimeout: () => 0,
    clearTimeout: () => {},
    api: {
      installed: async () => current.report,
      customCatalog: async () => [],
      search: async () => [],
      packageLinks: async () => ({ homepage: null, iconDomain: null }),
      installWinget: async (id, mode) => {
        installs.push([id, mode]);
        const update = current.report.updates.find((u) => u.id.toLowerCase() === id.toLowerCase());
        if (!update) return { result: "done", note: null };
        current.report.updates = current.report.updates.filter((u) => u !== update);
        current.report.winget = current.report.winget.map((p) => (p.id === update.id ? { ...p, version: update.available, available: null } : p));
        return { result: "done", note: null };
      },
      installCustom: async () => ({ result: "done", note: null }),
      cancel: async () => {},
      activate: async () => {},
      unpatchBetterDiscordPtb: async () => {},
      exportList: async () => true,
      importList: async () => null,
    },
  });
  vm.runInContext(code, context);
  return { state: context.appsState, installs, report: () => current.report, dropUpdate(id) {
    current.report.updates = current.report.updates.filter((u) => u.id !== id);
    current.report.winget = current.report.winget.filter((p) => p.id !== id);
  } };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 30));
test("twenty winget updates become twenty cards in Updates", async () => {
  const { state } = harness();
  await state.checkInstalled();
  state.filter = "updates";
  assert.equal(state.counts.updates, 20, "the badge counts the cards it shows");
  assert.equal(state.visible.length, 20);
  assert.equal(state.autoUpdates.length, 18, "the two catalog apps keep their own cards");
  assert.equal(new Set(state.visible.map((app) => app.id.toLowerCase())).size, 20, "every package id once");
  const spotify = state.visible.find((app) => app.id === "Spotify.Spotify");
  assert.equal(spotify.name, "Spotify", "a known app keeps its catalog name");
  assert.equal(spotify.source, "winget");
  assert.equal(state.autoUpdates.find((app) => app.id === "Vendor.App0").name, "Vendor App 0");
});
test("unknown versions and self-updating apps are updates too", async () => {
  const { state } = harness();
  await state.checkInstalled();
  const spotify = state.all.find((app) => app.id === "Spotify.Spotify");
  assert.equal(spotify.selfUpdating, true);
  assert.equal(state.statusOf(spotify), "update", "winget's list wins over selfUpdating");
  assert.equal(state.statusInfo(spotify).version, "Unknown", "the unreadable current version is kept");
  const unknown = state.autoUpdates.find((app) => app.id === "Vendor.App1");
  assert.equal(state.statusOf(unknown), "update");
  const info = state.statusInfo(unknown);
  assert.equal(info.installed, true);
  assert.equal(info.version, "1.0");
  assert.equal(info.available, "2.0");
});
test("the check leaves every checkbox empty", async () => {
  const { state } = harness();
  await state.checkInstalled();
  assert.equal(state.selected.size, 0);
});
test("ticking one update card updates exactly that package id", async () => {
  const { state, installs } = harness();
  await state.checkInstalled();
  state.filter = "updates";
  const card = state.autoUpdates.find((app) => app.id === "Vendor.App3");
  state.toggle(card);
  assert.deepEqual([...state.selected], ["Vendor.App3"]);
  await state.installSelected();
  await settle();
  assert.deepEqual(installs, [["Vendor.App3", "upgrade"]]);
  state.filter = "updates";
  assert.ok(!state.visible.some((app) => app.id === "Vendor.App3"), "the updated card leaves the list");
  assert.equal(state.selected.size, 0);
});
test("a recheck unticks updates that are no longer offered, so they cannot install", async () => {
  const { state, installs, dropUpdate } = harness();
  await state.checkInstalled();
  state.filter = "updates";
  const card = state.autoUpdates.find((app) => app.id === "Vendor.App4");
  state.toggle(card);
  assert.equal(state.selected.size, 1);
  dropUpdate("Vendor.App4");
  await state.checkInstalled();
  assert.equal(state.selected.size, 0, "the stale tick is dropped");
  await state.installSelected();
  await settle();
  assert.deepEqual(installs, [], "nothing is installed behind it");
});
test("pinned cards are not carded twice in Updates", async () => {
  const { state } = harness();
  await state.checkInstalled();
  state.pinned = [{ id: "Vendor.App5", name: "Vendor App 5", source: "catalog", category: "Others" }];
  state.filter = "updates";
  const ids = state.visible.map((app) => app.id).concat(state.visiblePinned.map((app) => app.id));
  assert.equal(ids.filter((id) => id === "Vendor.App5").length, 1);
  assert.equal(state.counts.updates, 20);
});
