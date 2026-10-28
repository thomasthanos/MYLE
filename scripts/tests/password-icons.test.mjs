import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { compileModule } from "svelte/compiler";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";
// Compile the actual page state, replacing native calls with fictional logins.
const source = readFileSync(new URL("../../frontend/app/pages/password-manager/state.svelte.ts", import.meta.url), "utf8")
  .replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");
const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, {
  compilerOptions: { target: ScriptTarget.ESNext, module },
}).outputText;
const executable = transpile(compileModule(transpile(source) + "\nglobalThis.vault = passwords;", {
  filename: "password-icons.svelte.js", generate: "client",
}).js.code, ModuleKind.CommonJS);
const settle = () => new Promise((resolve) => setImmediate(resolve));
const retryAfter = 5 * 60_000;
function harness(icons) {
  let now = 1_000_000, nextTimer = 0, calls = 0;
  const timers = new Map();
  const api = {
    list: async () => [{ id: "fixture", title: "Example", urls: ["https://www.example.com/login"] }],
    icons: async (skip) => { calls++; return icons(skip); },
    lock: async () => {}, setWebsiteIcons: async () => {},
  };
  const context = vm.createContext({
    require: createRequire(import.meta.url), exports: {}, URL,
    Date: class extends Date { static now() { return now; } },
    readJson: (_key, fallback) => fallback, readFlag: () => false,
    DEVICE: "PC", MOBILE: false, QUICK_UNLOCK: "Windows Hello", api,
    toast: { error() {}, success() {}, info() {} },
    setTimeout: (fn, delay) => { const id = ++nextTimer; timers.set(id, { fn, at: now + delay }); return id; },
    clearTimeout: (id) => timers.delete(id),
  });
  vm.runInContext(executable, context);
  const state = context.vault;
  state.status = "unlocked";
  return { state, api, timers, get calls() { return calls; }, async advance(ms) {
    now += ms;
    for (const [id, timer] of [...timers]) if (timer.at <= now) { timers.delete(id); timer.fn(); }
    await settle();
  } };
}
test("a missing icon is retried while the vault stays open", async () => {
  const h = harness(() => h.calls === 1 ? {} : { "example.com": "data:image/png;base64,fixture" });
  await h.state.refresh(); await settle();
  assert.equal(h.calls, 1);
  await h.advance(retryAfter);
  assert.equal(h.calls, 2);
  assert.equal(h.state.iconFor(["https://example.com/"]), "data:image/png;base64,fixture");
  assert.equal(h.timers.size, 0, "found icons do not keep polling");
});
test("a failed native call does not suppress the next refresh", async () => {
  const h = harness(() => { if (h.calls === 1) throw new Error("temporary failure"); return { "example.com": "icon" }; });
  await h.state.refresh(); await settle();
  assert.equal(h.calls, 1);
  await h.state.refresh(); await settle();
  assert.equal(h.calls, 2);
  assert.equal(h.state.iconFor(["example.com"]), "icon");
  assert.equal(h.timers.size, 0);
});
test("locking cancels retries and ignores a response from the previous unlock", async () => {
  let resolve;
  const response = new Promise((yes) => { resolve = yes; });
  const h = harness(() => response);
  await h.state.refresh(); await settle();
  await h.state.lock();
  h.state.status = "unlocked";
  resolve({ "example.com": "stale icon" }); await settle();
  assert.equal(h.state.iconFor(["example.com"]), null);
  assert.equal(h.timers.size, 0);
});
test("turning website icons off cancels all pending retries", async () => {
  const h = harness(() => ({}));
  await h.state.refresh(); await settle();
  assert.equal(h.timers.size, 1);
  await h.state.setWebsiteIcons(false);
  await h.advance(retryAfter);
  assert.equal(h.calls, 1);
  assert.equal(h.timers.size, 0);
});
test("clearing local data forgets decrypted UI state and ignores a late list", async () => {
  const h = harness(() => ({}));
  h.state.revealed = { fixture: "fictional password" };
  h.state.query = "private search"; h.state.filter = "favorites";
  h.state.hello = { available: true, enabled: true };
  h.state.icons = { "example.com": "fixture" };
  h.state.panel = { kind: "view", id: "fixture" };
  let resolve;
  h.api.list = () => new Promise((yes) => { resolve = yes; });
  const reading = h.state.refresh();
  h.state.resetLocal();
  resolve([{ id: "late", urls: ["https://example.com/"] }]);
  await reading;
  assert.equal(h.state.status, "new");
  assert.equal(h.state.entries.length, 0);
  assert.equal(Object.keys(h.state.revealed).length, 0);
  assert.equal(Object.keys(h.state.icons).length, 0);
  assert.equal(h.state.query, "");
  assert.equal(h.state.filter, "all");
  assert.equal(h.state.hello.enabled, false);
  assert.equal(h.state.panel.kind, "none");
  assert.equal(h.timers.size, 0);
});
test("a sync started before local reset cannot restore the previous account state", async () => {
  const h = harness(() => ({}));
  let resolve;
  h.api.sync = () => new Promise((yes) => { resolve = yes; });
  const syncing = h.state.syncNow();
  h.state.resetLocal();
  resolve({ state: "adopted", pending: 0 });
  await syncing;
  assert.equal(h.state.status, "new");
  assert.equal(h.state.sync.kind, "signedOut");
});
