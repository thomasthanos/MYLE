import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire, registerHooks } from "node:module";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { compileModule } from "svelte/compiler";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";

const settingsSource = readFileSync(new URL("../../frontend/app/pages/settings/Settings.svelte", import.meta.url), "utf8");
const splashSource = readFileSync(new URL("../../frontend/splash/Splash.svelte", import.meta.url), "utf8");
const stateUrl = new URL("../../frontend/lib/updater-state.svelte.ts", import.meta.url);
const asset = { name: "MYLE.exe", version: "2.0.0", url: "https://downloads.thomast.uk/MYLE.exe", size: 100, digest: "sha256:" + "0".repeat(64) };
const available = { status: "available", current: "1.0.0", latest: "2.0.0", notes: "", asset };
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
};
const transpile = (source) => transpileModule(source, {
  compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext },
}).outputText;
const require = createRequire(import.meta.url);

let instance = 0;
async function settingsHarness() {
  const checking = deferred(), installing = deferred();
  const errors = [];
  let checks = 0, installs = 0, onEvent;
  const native = {
    check: () => { checks++; return checking.promise; },
    install: (_asset, callback) => { installs++; onEvent = callback; return installing.promise; },
  };
  globalThis.__updaterNative = native;
  globalThis.__updaterErrors = errors;
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (context.parentURL?.includes("updater-state.svelte.ts")) {
        if (specifier === "./updater") return { url: "data:text/javascript," + encodeURIComponent("export const checkForUpdate=()=>globalThis.__updaterNative.check(); export const installUpdate=(a,e)=>globalThis.__updaterNative.install(a,e); export const formatBytes=b=>`${b} B`;"), shortCircuit: true };
        if (specifier === "./toast.svelte") return { url: "data:text/javascript," + encodeURIComponent("export const toast={error:m=>globalThis.__updaterErrors.push(m)};"), shortCircuit: true };
      }
      return next(specifier, context);
    },
    load(url, context, next) {
      if (!new URL(url).pathname.endsWith("updater-state.svelte.ts")) return next(url, context);
      return { format: "module", source: compileModule(transpile(readFileSync(fileURLToPath(url), "utf8")), { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
    },
  });
  let settingsUpdater;
  try {
    settingsUpdater = (await import(`${stateUrl.href}?test=${instance++}`)).settingsUpdater;
  } finally {
    hooks.deregister();
  }
  const script = transpile(settingsSource.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1]).replace(/^import[\s\S]*?;\s*$/gm, "");
  // Compile the page's actual reactive script, with only native calls and
  // mounting replaced. A second execution models leaving and returning.
  const compiled = compileModule(`${script}\nglobalThis.page={get view(){return update},check,install};`, { filename: "settings-test.svelte.js", generate: "client" }).js.code;
  const executable = transpileModule(compiled, { compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.CommonJS } }).outputText;
  const mount = () => {
    const context = { onMount: () => {}, settingsUpdater, require, exports: {} };
    vm.createContext(context);
    vm.runInContext(executable, context);
    return context.page;
  };
  return { mount, checking, installing, errors, event: (event) => onEvent(event), get checks() { return checks; }, get installs() { return installs; } };
}

test("splash respects a total withdrawn after a stale manifest size is exceeded", () => {
  const functionSource = splashSource.slice(splashSource.indexOf("  function onProgress("), splashSource.indexOf("  function timeLeft("));
  const context = { performance: { now: () => 1000 }, sample: { at: 0, bytes: 0 }, speed: 0, lastMeta: 0, assetSize: 100, progress: null, percent: null, transfer: "", rate: "", SPEED_WINDOW_MS: 500, META_EVERY_MS: 250, formatBytes: (bytes) => `${bytes} B`, timeLeft: () => "1s left" };
  vm.createContext(context);
  vm.runInContext(`${transpile(functionSource)}\nonProgress(150,null);`, context);
  assert.equal(context.progress, null);
  assert.equal(context.percent, null);
  assert.equal(context.transfer, "150 B");
  vm.runInContext("onProgress(50,100);", context);
  assert.equal(context.progress, 0.5);
  assert.equal(context.percent, 50);
  vm.runInContext("onProgress(150,0);", context);
  assert.equal(context.progress, null);
  assert.equal(context.percent, null);
});

test("settings keeps download progress when the page remounts", async () => {
  const h = await settingsHarness();
  const first = h.mount();
  const running = first.install(available.latest, asset);
  h.event({ event: "progress", data: { downloaded: 40, total: 100 } });
  const second = h.mount();
  assert.equal(second.view.state, "downloading");
  assert.equal(second.view.progress, 0.4);
  h.installing.resolve();
  await running;
});

test("settings shows verification instead of a completed download", async () => {
  const h = await settingsHarness();
  const page = h.mount();
  const running = page.install(available.latest, asset);
  h.event({ event: "progress", data: { downloaded: 100, total: 100 } });
  h.event({ event: "verifying" });
  assert.equal(page.view.state, "verifying");
  h.installing.resolve();
  await running;
});

test("a pending check excludes another check and an install across remounts", async () => {
  const h = await settingsHarness();
  const first = h.mount();
  const running = first.check();
  const second = h.mount();
  void second.check();
  void second.install(available.latest, asset);
  assert.equal(h.checks, 1);
  assert.equal(h.installs, 0);
  h.checking.resolve(available);
  await running;
  assert.equal(second.view.state, "available");
});

test("a pending install excludes another install and check across remounts", async () => {
  const h = await settingsHarness();
  const first = h.mount();
  const running = first.install(available.latest, asset);
  const second = h.mount();
  void second.install(available.latest, asset);
  void second.check();
  assert.equal(h.installs, 1);
  assert.equal(h.checks, 0);
  h.installing.resolve();
  await running;
});

test("an update failure survives remounting, reports a toast and permits checking again", async () => {
  const h = await settingsHarness();
  const first = h.mount();
  const running = first.install(available.latest, asset);
  h.installing.reject(new Error("setup closed"));
  await running;
  const second = h.mount();
  assert.equal(second.view.state, "error");
  assert.equal(second.view.message, "setup closed");
  assert.deepEqual(h.errors, ["The update failed: setup closed"]);
  const retry = second.check();
  h.checking.resolve(available);
  await retry;
  assert.equal(second.view.state, "available");
});

test("settings keeps unknown totals indeterminate and follows installation and restart events", async () => {
  const h = await settingsHarness();
  const page = h.mount();
  const running = page.install(available.latest, asset);
  h.event({ event: "started", data: { total: null } });
  assert.equal(page.view.progress, null);
  h.event({ event: "progress", data: { downloaded: 150, total: null } });
  assert.equal(page.view.progress, null);
  assert.equal(page.view.detail, "150 B");
  h.event({ event: "installing" });
  assert.equal(page.view.state, "installing");
  h.event({ event: "restarting", data: { version: "" } });
  assert.equal(page.view.state, "restarting");
  assert.equal(page.view.version, "2.0.0");
  h.installing.resolve();
  await running;
});

test("a failed check releases the operation guard so installation can retry", async () => {
  const h = await settingsHarness();
  const page = h.mount();
  const checking = page.check();
  h.checking.reject("offline");
  await checking;
  assert.equal(page.view.state, "error");
  assert.equal(page.view.message, "offline");
  assert.deepEqual(h.errors, []);
  const retry = h.mount().install(available.latest, asset);
  assert.equal(h.installs, 1);
  h.installing.resolve();
  await retry;
});
