import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";

const hooks = registerHooks({
  resolve(specifier, context, next) {
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
const { cloudStorage } = await import("../../frontend/app/pages/disk-analyzer/cloud-storage/state.svelte.ts");
const { cloudStorageApi: api } = await import("../../frontend/app/pages/disk-analyzer/cloud-storage/api.ts");
hooks.deregister();
const original = { ...api };
const tick = () => new Promise(setImmediate);
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const native = () => ({ provider: "oneDrive", name: "OneDrive", mode: "native", detail: "Ready", folders: [{ label: "Personal", path: "C:/OneDrive" }] });
const outcome = (extra = {}) => ({ visited: 5, requested: 3, alreadyOnline: 0, pendingUploads: 2, skipped: 0, failed: 0, cancelled: false, errors: [], ...extra });

test.beforeEach(() => {
  Object.assign(api, original);
  Object.assign(cloudStorage, { providers: [native()], loading: false, loadError: null, selected: "oneDrive", running: false, cancelling: false, requestId: null, progress: null, result: null, error: null });
  api.status = async () => ({ providers: [native()] });
});

test("choosing a provider opens its review without requesting any changes", () => {
  let calls = 0; api.onlineOnly = async () => { calls++; return outcome(); };
  cloudStorage.choose("oneDrive");
  assert.equal(cloudStorage.selected, "oneDrive");
  assert.equal(calls, 0);
});

test("guided and unavailable providers never invoke the native online-only action", async () => {
  let calls = 0; api.onlineOnly = async () => { calls++; return outcome(); };
  for (const mode of ["guided", "unavailable"]) {
    cloudStorage.providers = [{ ...native(), provider: "googleDrive", mode }];
    cloudStorage.selected = "googleDrive";
    await cloudStorage.makeOnlineOnly();
    assert.equal(calls, 0);
    assert.equal(cloudStorage.result, null);
    assert.equal(cloudStorage.running, false);
  }
});

test("a native request freezes the reviewed folders and preserves pending uploads in its result", async () => {
  const wait = deferred(); const calls = [];
  api.onlineOnly = (provider, folders, requestId, onProgress) => {
    calls.push({ provider, folders, requestId });
    onProgress({ ...outcome(), current: "C:/OneDrive/upload.zip" });
    return wait.promise;
  };
  const requesting = cloudStorage.makeOnlineOnly(); await tick();
  assert.equal(cloudStorage.running, true);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].provider, "oneDrive");
  assert.deepEqual(calls[0].folders, ["C:/OneDrive"]);
  assert.ok(calls[0].requestId);
  assert.equal(cloudStorage.progress.pendingUploads, 2);
  await cloudStorage.makeOnlineOnly();
  assert.equal(calls.length, 1, "double clicks cannot start another action");
  wait.resolve(outcome()); await requesting;
  assert.equal(cloudStorage.result.requested, 3);
  assert.equal(cloudStorage.result.pendingUploads, 2);
  assert.equal(cloudStorage.running, false);
});

test("cancelling addresses only this request and waits for the backend to finish", async () => {
  const wait = deferred(); let active; const cancelled = [];
  api.onlineOnly = (_provider, _folders, id) => { active = id; return wait.promise; };
  api.cancel = async (id) => cancelled.push(id);
  const requesting = cloudStorage.makeOnlineOnly(); await tick();
  await cloudStorage.cancel();
  assert.deepEqual(cancelled, [active]);
  assert.equal(cloudStorage.running, true);
  wait.resolve(outcome({ cancelled: true, requested: 1 })); await requesting;
  assert.equal(cloudStorage.result.cancelled, true);
});

test("a request failure is shown and never becomes a successful result", async () => {
  api.onlineOnly = async () => { throw new Error("Cloud folders changed. Refresh and try again."); };
  await cloudStorage.makeOnlineOnly();
  assert.match(cloudStorage.error, /Cloud folders changed/);
  assert.equal(cloudStorage.result, null);
  assert.equal(cloudStorage.running, false);
});

test("a running request keeps its provider and cancel control visible", async () => {
  const wait = deferred(); api.onlineOnly = () => wait.promise;
  const requesting = cloudStorage.makeOnlineOnly(); await tick();
  cloudStorage.close(); cloudStorage.choose("mega");
  assert.equal(cloudStorage.selected, "oneDrive");
  wait.resolve(outcome()); await requesting;
  cloudStorage.close();
  assert.equal(cloudStorage.selected, null);
});

test("an older status response cannot overwrite a newer provider check", async () => {
  const first = deferred(); let calls = 0;
  api.status = () => ++calls === 1 ? first.promise : Promise.resolve({ providers: [{ ...native(), mode: "unavailable", folders: [] }] });
  const loading = cloudStorage.load(); await cloudStorage.load();
  first.resolve({ providers: [native()] }); await loading;
  assert.equal(cloudStorage.providers[0].mode, "unavailable");
  await cloudStorage.makeOnlineOnly();
  assert.equal(cloudStorage.result, null);
});

test("late progress from a completed request cannot replace a later result", async () => {
  let late;
  api.onlineOnly = async (_provider, _folders, _id, onProgress) => { late = onProgress; return outcome(); };
  await cloudStorage.makeOnlineOnly();
  const before = cloudStorage.progress;
  late({ ...outcome({ requested: 99 }), current: "old" });
  assert.equal(cloudStorage.progress, before);
  assert.equal(cloudStorage.result.requested, 3);
});

test("failed refresh preserves visible folders but blocks native changes until a successful refresh", async () => {
  let calls = 0;
  api.onlineOnly = async () => { calls++; return outcome(); };
  api.status = async () => { throw new Error("Cloud detection failed"); };
  await cloudStorage.load();
  assert.match(cloudStorage.loadError, /detection failed/);
  assert.equal(cloudStorage.current.folders[0].path, "C:/OneDrive");
  await cloudStorage.makeOnlineOnly();
  assert.equal(calls, 0);
  api.status = async () => ({ providers: [native()] });
  await cloudStorage.load();
  await cloudStorage.makeOnlineOnly();
  assert.equal(calls, 1);
});

test("a failed cancellation keeps the request active and allows retrying Stop", async () => {
  const wait = deferred();
  api.onlineOnly = () => wait.promise;
  api.cancel = async () => { throw new Error("Stop failed"); };
  const running = cloudStorage.makeOnlineOnly();
  await cloudStorage.cancel();
  assert.equal(cloudStorage.running, true);
  assert.equal(cloudStorage.cancelling, false);
  assert.match(cloudStorage.error, /Stop failed/);
  let retried = false;
  api.cancel = async () => { retried = true; };
  await cloudStorage.cancel();
  assert.equal(retried, true);
  wait.resolve(outcome({ cancelled: true }));
  await running;
});

test("folder and official-help launch failures are visible without starting a native request", async () => {
  api.openFolder = async () => { throw new Error("Folder unavailable"); };
  api.openHelp = async () => { throw new Error("Browser unavailable"); };
  await cloudStorage.openFolder("C:/OneDrive");
  assert.match(cloudStorage.error, /Folder unavailable/);
  await cloudStorage.help();
  assert.match(cloudStorage.error, /Browser unavailable/);
  assert.equal(cloudStorage.running, false);
  assert.equal(cloudStorage.result, null);
});
