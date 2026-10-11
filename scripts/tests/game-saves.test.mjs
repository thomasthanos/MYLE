import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";

const storage = new Map();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
} });

// Exercise the actual client state, with only native IPC replaced. Compile
// Svelte runes just as the frontend does; no Windows files are touched.
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
    return { format: "module", source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
  },
});
const { gameSavesState: state, samePath } = await import("../../frontend/app/pages/game-saves/state.svelte.ts");
const { gameSavesApi: api } = await import("../../frontend/app/pages/game-saves/api.ts");
const { confirmState } = await import("../../frontend/lib/confirm.svelte.ts");
const { toast } = await import("../../frontend/lib/toast.svelte.ts");
hooks.deregister();

const original = { ...api };
const clone = (value) => JSON.parse(JSON.stringify(value));
const settings = clone(state.page.settings);
const entry = (id, extra = {}) => ({ id, title: id, status: "notBackedUp", platformBadges: ["Steam"], fileCount: 2, totalBytes: 2048, lastSaveAt: null, lastBackupAt: null, paths: ["C:\\Saves\\" + id], autoBackup: true, hasLocalData: true, hasBackup: false, error: null, snapshots: [], ...extra });
const scan = (extra = {}) => ({ generatedAt: Date.now(), stats: { localGames: 2, backupGames: 1, totalBytes: 4096, databaseGames: 100 }, onThisPc: [entry("a"), entry("b")], inBackup: [entry("a", { hasBackup: true, status: "backedUp", snapshots: [{ id: "latest", timestamp: "2026-09-01", bytes: 2048, label: null, isSafety: false }] })], fullScanAt: Date.now(), quick: false, discoveryDue: false, ...extra });
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const settle = () => new Promise((resolve) => setImmediate(resolve));

test.beforeEach(() => {
  storage.clear();
  Object.assign(api, original);
  state.page = { ...state.page, engineAvailable: true, activeOperation: null, settings: { ...clone(settings), backupFolder: "D:\\Backups" }, undoRestore: null };
  state.scanResult = scan(); state.loading = false; state.operation = null; state.settingsBusy = null;
  state.tab = "pc"; state.filter = "all"; state.query = ""; state.selected.clear(); state.snapshotChoices = {}; state.error = null;
  state.restoreDialogOpen = false; state.customDialogOpen = false; state.failures = null;
  api.getState = async () => clone({ ...state.page, cachedScan: state.scanResult });
});
test.afterEach(async () => {
  await state.stopDiscovery();
  while (toast.visible.length) toast.dismiss(toast.visible[0].id);
  if (confirmState.current) confirmState.answer(false);
});

test("a quick refresh starts discovery after releasing its operation", async () => {
  const calls = [];
  api.scan = async (mode) => { calls.push(mode); return scan({ discoveryDue: mode === "quick" }); };
  await state.refresh(); await settle();
  assert.deepEqual(calls, ["quick", "full"]);
  assert.equal(state.busy, false);
});

test("automatic backup saves are serialized without blocking list selection", async () => {
  const wait = deferred(); const calls = [];
  api.setGameAutoBackup = (id, enabled) => { calls.push([id, enabled]); return calls.length === 1 ? wait.promise : Promise.resolve({ ...state.page.settings, autoBackupExcludedGameIds: ["a", "b"] }); };
  const saving = state.setAutoBackup(state.games[0], false);
  await settle();
  assert.equal(state.locked, true);
  assert.equal(state.selectionLocked, false);
  const second = state.setAutoBackup(state.games[1], false);
  state.toggleSelected(state.games[1]);
  assert.deepEqual([...state.selected], ["b"]);
  assert.deepEqual(calls, [["a", false]]);
  wait.resolve({ ...state.page.settings, autoBackupExcludedGameIds: ["a"] }); await Promise.all([saving, second]);
  assert.deepEqual(calls, [["a", false], ["b", false]]);
  assert.equal(state.scanResult.onThisPc[0].autoBackup, false);
  assert.equal(state.scanResult.inBackup[0].autoBackup, false);
  assert.equal(state.scanResult.onThisPc[1].autoBackup, false);
  assert.equal(state.locked, false);
});

test("switch spam saves the final choice without refreshing or replacing unrelated state", async () => {
  const wait = deferred(); const calls = [];
  const settingsObject = state.page.settings; const scanObject = state.scanResult; const untouched = state.games[1];
  api.scan = async () => { throw new Error("A preference must never scan"); };
  api.getState = async () => { throw new Error("A preference must never reload the page"); };
  api.setGameAutoBackup = (id, enabled) => { calls.push([id, enabled]); return calls.length === 1 ? wait.promise : Promise.resolve({ ...state.page.settings, autoBackupExcludedGameIds: [] }); };
  const first = state.setAutoBackup(state.games[0], false); await settle();
  const queued = [state.setAutoBackup(state.games[0], true), state.setAutoBackup(state.games[0], false), state.setAutoBackup(state.games[0], true)];
  assert.equal(state.games[0].autoBackup, true);
  assert.equal(state.autoBackupPending.has("a"), true);
  assert.equal(state.autoBackupPending.has("b"), false);
  wait.resolve({ ...state.page.settings, autoBackupExcludedGameIds: ["a"] }); await Promise.all([first, ...queued]);
  assert.deepEqual(calls, [["a", false], ["a", true]]);
  assert.equal(state.scanResult.onThisPc[0].autoBackup, true);
  assert.equal(state.scanResult.inBackup[0].autoBackup, true);
  assert.equal(state.page.settings, settingsObject);
  assert.equal(state.scanResult, scanObject);
  assert.equal(state.games[1], untouched);
  assert.equal(state.autoBackupPending.size, 0);
});

test("a rejected automatic backup change restores both tabs and unlocks", async () => {
  api.setGameAutoBackup = async () => { throw new Error("IPC unavailable"); };
  await state.setAutoBackup(state.games[0], false);
  assert.equal(state.scanResult.onThisPc[0].autoBackup, true);
  assert.equal(state.scanResult.inBackup[0].autoBackup, true);
  assert.equal(state.settingsBusy, null);
});

test("changing backup folder drops old snapshots and refreshes the new folder", async () => {
  state.tab = "backup"; state.selected.add("a"); state.snapshotChoices = { a: "latest" };
  api.pickBackupFolder = async () => ({ ...state.page.settings, backupFolder: "E:\\Other" });
  const wait = deferred(); api.scan = () => wait.promise;
  const changing = state.chooseBackupFolder(); await settle();
  assert.equal(state.scanResult.inBackup.length, 0);
  assert.equal(state.selected.size, 0);
  assert.deepEqual(state.snapshotChoices, {});
  wait.resolve(scan({ inBackup: [] })); await changing;
  assert.equal(state.page.settings.backupFolder, "E:\\Other");
  assert.equal(state.busy, false);
});

test("selection skips missing snapshots and survives search and filters", () => {
  state.tab = "backup";
  state.scanResult.inBackup.push(entry("broken", { hasBackup: true, hasLocalData: false, status: "error" }));
  state.toggleAllVisible();
  assert.deepEqual([...state.selected], ["a"]);
  state.setQuery("nothing"); state.setFilter("problems");
  assert.equal(state.visibleGames.length, 0);
  assert.deepEqual(state.selectedGames.map((game) => game.id), ["a"]);
});

test("restore location picker holds the lock until the mapping is saved", async () => {
  const picking = deferred(); const writes = [];
  api.pickFolder = () => picking.promise;
  api.setPathMapping = async (mapping) => { writes.push(mapping); return { ...state.page.settings, pathMappings: [mapping] }; };
  const choosing = state.chooseRestoreLocation(state.games[0]); await settle();
  assert.equal(state.locked, true);
  picking.resolve("E:\\Saves"); await choosing;
  assert.deepEqual(writes, [{ gameId: "a", source: "C:\\Saves\\a", target: "E:\\Saves" }]);
  assert.equal(state.locked, false);
});

test("backup uses selected IDs even when their rows are filtered out", async () => {
  state.selected.add("b"); state.setQuery("a"); const calls = [];
  api.backup = async (ids) => { calls.push(ids); return { kind: "backup", processedGames: 1, failedGames: [], failures: [] }; };
  api.scan = async () => scan();
  const backingUp = state.backupSelected(); confirmState.answer(true); await backingUp;
  assert.deepEqual(calls, [["b"]]);
});

test("Windows folder comparisons handle separators and trailing slashes", () => {
  assert.equal(samePath("C:\\Games\\Saves\\", "c:/games/saves"), true);
  assert.equal(samePath("C:\\Games\\Saves", "C:\\Games\\Other"), false);
});

test("a late scan settings response cannot overwrite a newer preference", async () => {
  const wait = deferred();
  api.getState = () => wait.promise;
  api.scan = async () => scan();
  const old = clone(state.page);
  await state.refresh();
  api.setSchedule = async () => ({ ...state.page.settings, schedule: "daily" });
  await state.setSchedule("daily", "03:00", "sunday");
  wait.resolve(old); await settle();
  assert.equal(state.page.settings.schedule, "daily");
});

test("a pending folder picker prevents starting scans or backup", async () => {
  const wait = deferred(); const calls = [];
  api.pickFolder = () => wait.promise;
  api.scan = async () => { calls.push("scan"); return scan(); };
  api.backup = async () => { calls.push("backup"); };
  state.selected.add("a");
  const picking = state.pickFolder("Test folder");
  await state.scan(); await state.backupSelected();
  assert.deepEqual(calls, []);
  assert.equal(confirmState.current, null);
  wait.resolve(null); await picking;
  assert.equal(state.locked, false);
});

test("partially failed backups keep failed games selected for a retry", async () => {
  state.selected.add("a"); state.selected.add("b");
  api.backup = async () => ({ kind: "backup", processedGames: 1, failedGames: ["b"], failures: [{ game: "b", reason: "File is locked" }] });
  api.scan = async () => scan();
  const backingUp = state.backupSelected(); confirmState.answer(true); await backingUp;
  assert.deepEqual([...state.selected], ["b"]);
  assert.deepEqual(clone(state.failures.items), [{ game: "b", reason: "File is locked" }]);
});

test("restore sends each chosen snapshot ID and uses its size", async () => {
  state.tab = "backup"; state.selected.add("a");
  state.scanResult.inBackup[0].snapshots.push({ id: "older", timestamp: "2026-08-01", bytes: 8192, label: null, isSafety: false });
  state.setSnapshot("a", "older");
  assert.equal(state.selectedBytes, 8192);
  const calls = [];
  api.restore = async (choices) => { calls.push(choices); return { kind: "restore", processedGames: 1, failedGames: [], failures: [] }; };
  api.scan = async () => scan();
  const restoring = state.restore([{ gameId: "a", snapshotId: state.snapshotFor(state.games[0]) }]);
  confirmState.answer(true); await restoring;
  assert.deepEqual(calls, [[{ gameId: "a", snapshotId: "older" }]]);
});

test("restore refuses a snapshot that is no longer in the scan", async () => {
  state.tab = "backup"; state.selected.add("a"); const calls = [];
  api.restore = async (choices) => { calls.push(choices); };
  await state.restore([{ gameId: "a", snapshotId: "deleted" }]);
  assert.deepEqual(calls, []);
  assert.equal(confirmState.current, null);
});

test("one click backs up the new, the changed, or both, whatever is selected", async () => {
  state.scanResult = scan({ onThisPc: [entry("new"), entry("changed", { status: "changedSinceBackup", hasBackup: true }), entry("fine", { status: "backedUp", hasBackup: true }), entry("gone", { hasLocalData: false })] });
  state.selected.add("fine");
  const calls = [];
  api.backup = async (ids) => { calls.push(ids); return { kind: "backup", processedGames: ids.length, failedGames: [], failures: [] }; };
  api.scan = async () => state.scanResult;
  for (const which of ["new", "changed", "both"]) {
    const backingUp = state.backupPending(which); await settle();
    assert.match(confirmState.current.title, which === "both" ? /new saves or new progress/ : which === "new" ? /first backup/ : /new progress of/);
    confirmState.answer(true); await backingUp;
  }
  assert.deepEqual(calls, [["new"], ["changed"], ["new", "changed"]]);
});

test("the filters count their games, and a partly ticked list says so", () => {
  state.scanResult = scan({ onThisPc: [entry("a"), entry("b", { status: "changedSinceBackup" }), entry("c", { status: "backedUp" }), entry("d", { status: "error" })] });
  assert.deepEqual({ ...state.statusCounts }, { all: 4, changed: 1, notBackedUp: 1, backedUp: 1, problems: 1 });
  state.toggleSelected(state.games[0]);
  assert.equal(state.someVisibleSelected, true);
  state.toggleAllVisible();
  assert.equal(state.allVisibleSelected, true);
  assert.equal(state.someVisibleSelected, false);
});

test("covers are asked once per game, and from Steam only when allowed", async () => {
  state.scanResult = scan({ onThisPc: [entry("x", { steamId: 900001 }), entry("y", { steamId: 900002 }), entry("z")], inBackup: [entry("x", { steamId: 900001, hasBackup: true })] });
  state.covers = {}; state.steamCovers = false;
  const calls = [];
  api.covers = async (ids, online) => { calls.push([ids, online]); return online ? { 900002: "data:image/jpeg;base64,Ag==" } : { 900001: "data:image/jpeg;base64,AQ==" }; };
  await state.loadCovers(); await state.loadCovers();
  assert.deepEqual(calls, [[[900001, 900002], false]]);
  assert.equal(state.covers[900001], "data:image/jpeg;base64,AQ==");
  state.setSteamCovers(true); await settle();
  assert.deepEqual(calls[1], [[900002], true], "only the one still missing, now from Steam");
  assert.equal(state.covers[900002], "data:image/jpeg;base64,Ag==");
  api.covers = async () => { throw new Error("IPC unavailable"); };
  state.scanResult = scan({ onThisPc: [entry("w", { steamId: 900003 })] });
  await state.loadCovers();
  assert.equal(state.covers[900003], undefined, "a failure leaves the monogram");
});

test("the preload shows the saved list and its covers without running the engine", async () => {
  state.scanResult = null;
  const calls = [];
  api.scan = async () => { calls.push("scan"); return scan(); };
  api.getState = async () => clone({ ...state.page, cachedScan: scan({ onThisPc: [entry("p", { steamId: 900010, status: "changedSinceBackup" })] }) });
  api.covers = async (ids) => { calls.push(["covers", ids]); return { 900010: "data:image/jpeg;base64,AQ==" }; };
  await state.preload();
  assert.deepEqual(calls, [["covers", [900010]]], "no scan, only the covers");
  assert.equal(state.scanResult.onThisPc[0].id, "p");
  assert.equal(state.covers[900010], "data:image/jpeg;base64,AQ==");
  await state.preload();
  assert.equal(calls.length, 1, "once is enough");
});

test("undoing a restore names the games and says the current saves are kept first", async () => {
  state.page = { ...state.page, undoRestore: { id: "restore-1", createdAt: 1_790_000_000, expiresAt: 1_790_604_800, games: ["Elden Ring", "Hades"] } };
  let undone = false;
  api.undoLastRestore = async () => { undone = true; return { kind: "restore", processedGames: 2, processedBytes: 0, failedGames: [], failures: [], safetyBackupPath: null, rolledBack: [] }; };
  const undoing = state.undoLastRestore();
  await settle();
  assert.match(confirmState.current.message, /Elden Ring, Hades/);
  assert.match(confirmState.current.message, /copied first/);
  confirmState.answer(false);
  await undoing;
  assert.equal(undone, false, "nothing happens without the user's yes");
});

test("a game put back after a restore that did not finish is said plainly", async () => {
  state.selected.add("a");
  state.tab = "backup";
  api.restore = async () => ({ kind: "restore", processedGames: 0, processedBytes: 0, failedGames: ["a"], failures: [{ game: "a", reason: "os error 32", file: null }], safetyBackupPath: "x", rolledBack: ["a"] });
  const restoring = state.restore([{ gameId: "a", snapshotId: "latest" }]);
  await settle();
  confirmState.answer(true);
  await restoring;
  assert.ok(toast.visible.some((item) => /a was put back as before/.test(item.message)));
});
