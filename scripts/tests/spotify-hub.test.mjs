import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";
import vm from "node:vm";
import { compileModule } from "svelte/compiler";
import { compareVersions } from "../../frontend/installer/versions.ts";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";
const require = createRequire(import.meta.url);
const source = readFileSync(new URL("../../frontend/app/pages/spotify-hub/state.svelte.ts", import.meta.url), "utf8")
  .replace(/^import[\s\S]*?;\r?\n/gm, "").replace("export const spotifyHubState", "const spotifyHubState");
const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, {
  compilerOptions: { target: ScriptTarget.ESNext, module },
}).outputText;
const executable = transpile(compileModule(transpile(source) + "\nglobalThis.hub = spotifyHubState;", {
  filename: "hub-test.svelte.js", generate: "client",
}).js.code, ModuleKind.CommonJS);
const snapshot = (overrides = {}) => ({
  desktop: { installed: true, version: "1.3.1" },
  store: { installed: false, version: null },
  spicetify: { installed: true, version: "2.45.3", healthy: true, applied: true },
  marketplace: { installed: true, version: "1.0.11" },
  prerequisites: { desktopSpotify: true, supported: true, message: null },
  activeJob: null, lastOutcome: null, ...overrides,
});
const latest = { cli: "2.45.3", marketplace: "1.0.11" };
function harness(api = {}, { setTimer = setTimeout, clearTimer = clearTimeout } = {}) {
  let runs = 0, checks = 0, locked = false;
  const context = vm.createContext({
    require, exports: {}, Date, setTimeout: setTimer, clearTimeout: clearTimer, isTauri: () => true,
    compareVersions,
    appsState: { busy: false }, confirm: async () => true, nav: {},
    toast: { error() {}, success() {}, info() {} },
    operationGate: { lockedFor: () => false, begin: () => !locked && (locked = true), end: () => { locked = false; } },
    spotifyHubApi: {
      getState: async () => snapshot(),
      checkUpdates: async () => { checks++; return latest; },
      run: async () => { runs++; return { result: "done", jobId: "new", action: "installSpicetify" }; },
      ...api,
    },
  });
  vm.runInContext(executable, context);
  return { hub: context.hub, get runs() { return runs; }, get checks() { return checks; } };
}
test("a current Spicetify and Marketplace do not offer another update or run an installer", async () => {
  const h = harness();
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Up to date");
  await h.hub.install();
  assert.equal(h.runs, 0);
});
test("Marketplace updates are offered even when the CLI is current", async () => {
  const h = harness({ getState: async () => snapshot({ marketplace: { installed: true, version: "1.0.10" } }) });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Update");
});
test("newer installed versions stay current and unknown Marketplace versions offer verification through repair", async () => {
  const h = harness({ getState: async () => snapshot({ spicetify: { installed: true, version: "2.46.0", healthy: true, applied: true } }) });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Up to date");
  const unknown = harness({ getState: async () => snapshot({ marketplace: { installed: true, version: null } }) });
  await unknown.hub.load();
  assert.equal(unknown.hub.installLabel(), "Repair");
  await unknown.hub.install();
  assert.equal(unknown.runs, 1);
});
test("successful installation refreshes detection and stops offering the same update", async () => {
  let installed = false;
  const h = harness({
    getState: async () => snapshot({ spicetify: { installed: true, version: installed ? "2.45.3" : "2.45.1", healthy: true, applied: true } }),
    run: async () => { installed = true; return { result: "done", jobId: "new", action: "installSpicetify" }; },
  });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Update");
  await h.hub.install();
  assert.equal(h.hub.installLabel(), "Up to date");
  assert.equal(h.hub.consoles.installSpicetify.lines[0], "Update requested.");
  assert.equal(h.hub.consoles.installSpicetify.open, false);
});
test("a failed installation keeps its console and offers one repair action", async () => {
  const h = harness({ run: async () => { throw Error("setup failed"); } });
  await h.hub.load();
  await h.hub.repair();
  assert.equal(h.hub.consoles.installSpicetify.open, true);
  assert.equal(h.hub.installLabel(), "Repair");
});
test("completion releases a revisited job even when the final detection fails", async () => {
  const timers = [];
  const job = { jobId: "current", action: "installSpicetify", stage: "installing", progress: 0.5 };
  let reads = 0, finishRun;
  const h = harness({
    getState: async () => {
      reads++;
      if (reads === 1) return snapshot();
      if (reads === 2) return snapshot({ activeJob: job });
      throw new Error("detection failed");
    },
    run: (_action, _token, onEvent) => {
      onEvent({ event: "stage", data: { jobId: job.jobId, stage: job.stage } });
      return new Promise((resolve) => { finishRun = resolve; });
    },
  }, { setTimer: (run) => { timers.push(run); return timers.length; }, clearTimer() {} });
  await h.hub.load(false, { checkUpdates: false });
  const repairing = h.hub.repair();
  await h.hub.load(true, { checkUpdates: false });
  // The adopted-job watcher defers to the locally owned command while it runs.
  await timers.shift()();
  finishRun({ result: "done", jobId: job.jobId, action: job.action, note: null });
  await repairing;
  assert.match(h.hub.error, /detection failed/);
  assert.equal(h.hub.statuses.installSpicetify, "Completed");
  assert.equal(h.hub.activeJob, null, "a completed job cannot survive through its cached snapshot");
  assert.equal(h.hub.locked, false, "the next action remains available after a failed refresh");
});
test("failed release checks remain retryable without reinstalling", async () => {
  let failed = true;
  const h = harness({ checkUpdates: async () => { if (failed) throw Error("offline"); return latest; } });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Check for updates");
  failed = false;
  await h.hub.install();
  assert.equal(h.hub.installLabel(), "Up to date");
  assert.equal(h.runs, 0);
});
test("first installation and a missing Marketplace keep their install and repair actions", async () => {
  const fresh = harness({ getState: async () => snapshot({ spicetify: { installed: false, version: null, healthy: false, applied: false } }) });
  await fresh.hub.load();
  assert.equal(fresh.hub.installLabel(), "Install Spicetify");
  await fresh.hub.install();
  assert.equal(fresh.runs, 1);
  const broken = harness({ getState: async () => snapshot({ marketplace: { installed: false, version: null } }) });
  await broken.hub.load();
  assert.equal(broken.hub.installLabel(), "Repair");
  await broken.hub.install();
  assert.equal(broken.runs, 1);
});
test("a Spotify self-update exposes repair even with the latest CLI and Marketplace", async () => {
  const h = harness({ getState: async () => snapshot({ spicetify: { installed: true, version: "2.45.3", healthy: true, applied: false } }) });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Repair");
  await h.hub.install();
  assert.equal(h.runs, 1);
});
test("an old check cannot overwrite the check after a completed repair", async () => {
  let resolveOld, calls = 0;
  const h = harness({ checkUpdates: () => ++calls === 1 ? new Promise(resolve => { resolveOld = resolve; }) : Promise.resolve(latest) });
  const loading = h.hub.load();
  await new Promise(resolve => setImmediate(resolve));
  await h.hub.repair();
  assert.equal(h.hub.installLabel(), "Up to date");
  resolveOld({ cli: "99.0.0", marketplace: "99.0.0" });
  await loading;
  assert.equal(h.hub.installLabel(), "Up to date");
});
