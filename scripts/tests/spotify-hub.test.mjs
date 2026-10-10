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
  spicetify: { installed: true, version: "2.45.3", healthy: true },
  marketplace: { installed: true, version: "1.0.11" },
  prerequisites: { desktopSpotify: true, supported: true, message: null },
  activeJob: null, lastOutcome: null, ...overrides,
});
const latest = { cli: "2.45.3", marketplace: "1.0.11" };

function harness(api = {}) {
  let runs = 0, checks = 0, locked = false;
  const context = vm.createContext({
    require, exports: {}, Date, setTimeout, clearTimeout, isTauri: () => true,
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

test("newer installed versions are not downgraded and unknown Marketplace versions are not claimed current", async () => {
  const h = harness({ getState: async () => snapshot({ spicetify: { installed: true, version: "2.46.0", healthy: true } }) });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Up to date");
  const unknown = harness({ getState: async () => snapshot({ marketplace: { installed: true, version: null } }) });
  await unknown.hub.load();
  assert.equal(unknown.hub.installLabel(), "Installed");
  await unknown.hub.install();
  assert.equal(unknown.runs, 0);
});

test("successful installation refreshes detection and stops offering the same update", async () => {
  let installed = false;
  const h = harness({
    getState: async () => snapshot({ spicetify: { installed: true, version: installed ? "2.45.3" : "2.45.1", healthy: true } }),
    run: async () => { installed = true; return { result: "done", jobId: "new", action: "installSpicetify" }; },
  });
  await h.hub.load();
  assert.equal(h.hub.installLabel(), "Update");
  await h.hub.install();
  assert.equal(h.hub.installLabel(), "Up to date");
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
  const fresh = harness({ getState: async () => snapshot({ spicetify: { installed: false, version: null, healthy: false } }) });
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
