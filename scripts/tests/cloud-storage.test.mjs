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
const drive = () => ({ provider: "googleDrive", name: "Google Drive", mode: "native", detail: "Ready", folders: [{ label: "Google Drive", path: "G:/My Drive" }] });
const guided = (provider, name) => ({ provider, name, mode: "guided", detail: "Own controls", folders: [{ label: name, path: `C:/${name}` }] });
const outcome = (extra = {}) => ({ visited: 5, requested: 3, alreadyOnline: 0, pendingUploads: 2, skipped: 0, failed: 0, foldersRequested: 0, foldersAlreadyOnline: 0, cancelled: false, errors: [], driveCache: null, ...extra });

test.beforeEach(() => {
  Object.assign(api, original);
  Object.assign(cloudStorage, { providers: [native()], loading: false, loadError: null, running: false, cancelling: false, requestId: null, progress: null, result: null, error: null });
  api.status = async () => ({ providers: [native()] });
});

test("one request covers every ready app at once and leaves guided and missing apps out", async () => {
  const calls = [];
  cloudStorage.providers = [drive(), guided("mega", "MEGA"), { ...native(), provider: "dropbox", name: "Dropbox", mode: "unavailable", folders: [] }, native(), { ...native(), provider: "sync:iCloudDrive", name: "iCloud Drive", folders: [{ label: "iCloud Drive", path: "C:/iCloud" }] }];
  api.onlineOnly = async (clouds, folders) => { calls.push({ clouds, folders }); return outcome({ foldersRequested: 1, driveCache: { state: "cleared", size: "94.4 GB" } }); };
  assert.deepEqual(cloudStorage.left.map((provider) => provider.name), ["MEGA"]);
  await cloudStorage.makeOnlineOnly();
  assert.deepEqual(calls, [{ clouds: ["googleDrive", "oneDrive", "sync:iCloudDrive"], folders: ["G:/My Drive", "C:/OneDrive", "C:/iCloud"] }]);
  assert.deepEqual(cloudStorage.result.driveCache, { state: "cleared", size: "94.4 GB" });
});

test("nothing is requested when no app allows online-only requests", async () => {
  let calls = 0; api.onlineOnly = async () => { calls++; return outcome(); };
  for (const providers of [[guided("mega", "MEGA")], [{ ...native(), mode: "unavailable" }], [{ ...native(), folders: [] }]]) {
    cloudStorage.providers = providers;
    await cloudStorage.makeOnlineOnly();
  }
  assert.equal(calls, 0);
  assert.equal(cloudStorage.result, null);
  assert.equal(cloudStorage.running, false);
});

test("a request freezes the checked folders and keeps pending uploads in its result", async () => {
  const wait = deferred(); const calls = [];
  api.onlineOnly = (clouds, folders, requestId, onProgress) => {
    calls.push({ clouds, folders, requestId });
    onProgress({ ...outcome(), current: "C:/OneDrive/upload.zip", waitingForDrive: false });
    return wait.promise;
  };
  const requesting = cloudStorage.makeOnlineOnly(); await tick();
  assert.equal(cloudStorage.running, true);
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].clouds, ["oneDrive"]);
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

test("waiting for Google Drive's storage window shows in the progress", async () => {
  const wait = deferred();
  api.onlineOnly = (_clouds, _folders, _id, onProgress) => { onProgress({ ...outcome(), current: null, waitingForDrive: true }); return wait.promise; };
  cloudStorage.providers = [drive()];
  const requesting = cloudStorage.makeOnlineOnly(); await tick();
  assert.equal(cloudStorage.progress.waitingForDrive, true);
  wait.resolve(outcome({ driveCache: { state: "notOpen" } })); await requesting;
  assert.equal(cloudStorage.result.driveCache.state, "notOpen");
});

test("stopping addresses only this request and waits for the backend to finish", async () => {
  const wait = deferred(); let active; const cancelled = [];
  api.onlineOnly = (_clouds, _folders, id) => { active = id; return wait.promise; };
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

test("an older status response cannot overwrite a newer check", async () => {
  const first = deferred(); let calls = 0;
  api.status = () => ++calls === 1 ? first.promise : Promise.resolve({ providers: [{ ...native(), mode: "unavailable", folders: [] }] });
  const loading = cloudStorage.load(); await cloudStorage.load();
  first.resolve({ providers: [native()] }); await loading;
  assert.equal(cloudStorage.providers[0].mode, "unavailable");
  let requests = 0; api.onlineOnly = async () => { requests++; return outcome(); };
  await cloudStorage.makeOnlineOnly();
  assert.equal(requests, 0);
});

test("late progress from a completed request cannot replace a later result", async () => {
  let late;
  api.onlineOnly = async (_clouds, _folders, _id, onProgress) => { late = onProgress; return outcome(); };
  await cloudStorage.makeOnlineOnly();
  const before = cloudStorage.progress;
  late({ ...outcome({ requested: 99 }), current: "old", waitingForDrive: false });
  assert.equal(cloudStorage.progress, before);
  assert.equal(cloudStorage.result.requested, 3);
});

test("a failed check blocks requests until a check succeeds", async () => {
  let calls = 0;
  api.onlineOnly = async () => { calls++; return outcome(); };
  api.status = async () => { throw new Error("Cloud detection failed"); };
  await cloudStorage.load();
  assert.match(cloudStorage.loadError, /detection failed/);
  await cloudStorage.makeOnlineOnly();
  assert.equal(calls, 0);
  api.status = async () => ({ providers: [native()] });
  await cloudStorage.load();
  await cloudStorage.makeOnlineOnly();
  assert.equal(calls, 1);
});

test("a failed Stop keeps the request running and allows retrying it", async () => {
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
