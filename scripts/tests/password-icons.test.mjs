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

test("a list reply received after locking never restores decrypted entries", async () => {
  const h = harness(() => ({}));
  let reply;
  h.api.list = () => new Promise(resolve => { reply = resolve; });
  const reading = h.state.refresh();
  await h.state.lock();
  reply([{ id: "old", urls: [] }]);
  await reading;
  assert.equal(h.state.status, "locked");
  assert.equal(h.state.entries.length, 0);
});

test("a reveal from the previous unlock cannot display its password after reopening", async () => {
  const h = harness(() => ({}));
  let reply;
  h.api.reveal = () => new Promise(resolve => { reply = resolve; });
  const revealing = h.state.toggleReveal("fixture");
  await h.state.lock();
  h.state.status = "unlocked";
  reply("fictional previous password");
  await revealing;
  assert.equal(Object.keys(h.state.revealed).length, 0);
  assert.equal(h.timers.size, 0);
});

test("an older list reply cannot overwrite a later refresh", async () => {
  const h = harness(() => ({}));
  let reply;
  h.api.list = () => new Promise(resolve => { reply = resolve; });
  const older = h.state.refresh();
  h.api.list = async () => [{ id: "current", urls: [] }];
  await h.state.refresh();
  reply([{ id: "old", urls: [] }]);
  await older;
  assert.equal(h.state.entries[0].id, "current");
});

test("a save received after locking leaves the panel closed and does not reload entries", async () => {
  const h = harness(() => ({}));
  let reply;
  h.api.save = () => new Promise(resolve => { reply = resolve; });
  h.api.list = async () => { throw new Error("must not reload the locked vault"); };
  const saving = h.state.save({ id: "fixture" });
  await h.state.lock();
  reply("fixture");
  await saving;
  assert.equal(h.state.panel.kind, "none");
  assert.equal(h.timers.size, 0);
});

function deferredLoad(h) {
  const registrations = []; const calls = { locked: 0, synced: 0, icon: 0, changed: 0, status: 0 };
  h.api.onLocked = () => { calls.locked++; return new Promise(resolve => { registrations.push(resolve); }); };
  for (const [method, key] of [["onSynced", "synced"], ["onIcon", "icon"], ["onChanged", "changed"]]) {
    h.api[method] = async () => { calls[key]++; return () => {}; };
  }
  h.api.helloStatus = async () => ({ available: false, enabled: false });
  h.api.status = async () => { calls.status++; return { status: "locked", autoLockMinutes: 5, damaged: 0, websiteIcons: true }; };
  h.api.sync = async () => ({ state: "nothing" });
  return { calls, register: () => registrations.forEach(resolve => resolve(() => {})) };
}

test("overlapping page loads share native listener registration and vault reads", async () => {
  const h = harness(() => ({})); const setup = deferredLoad(h);
  const first = h.state.load(); const second = h.state.load();
  setup.register(); await Promise.all([first, second]);
  assert.deepEqual(setup.calls, { locked: 1, synced: 1, icon: 1, changed: 1, status: 1 });
});

test("a page opened after local reset rechecks its account once an earlier load finishes", async () => {
  const h = harness(() => ({})); const setup = deferredLoad(h);
  const first = h.state.load();
  h.state.resetLocal();
  const reopened = h.state.load();
  setup.register(); await Promise.all([first, reopened]);
  assert.equal(h.state.checkedAccount, true);
  assert.equal(h.state.status, "locked");
  assert.equal(setup.calls.locked, 1);
  assert.equal(setup.calls.status, 1);
});

test("locking invalidates an unfinished unlock's busy state", async () => {
  const h = harness(() => ({})); h.state.status = "locked";
  let reply;
  h.api.unlock = () => new Promise(resolve => { reply = resolve; });
  const opening = h.state.unlock("fictional");
  await h.state.lock();
  const stillBusy = h.state.busy;
  reply(); await opening;
  assert.equal(stillBusy, false, "the invalidated unlock must not keep the new lock screen disabled");
});

test("an invalidated unlock's completion cannot clear a newer unlock's busy state", async () => {
  const h = harness(() => ({})); h.state.status = "locked";
  let oldReply, rejectCurrent;
  h.api.unlock = () => new Promise(resolve => { oldReply = resolve; });
  const previous = h.state.unlock("old fictional");
  await h.state.lock();
  h.api.unlock = () => new Promise((_resolve, reject) => { rejectCurrent = reject; });
  const current = h.state.unlock("current fictional");
  oldReply(); await previous;
  const stillBusy = h.state.busy;
  rejectCurrent(new Error("fixture unlock declined")); await current;
  assert.equal(stillBusy, true, "the current unlock still owns the busy indicator");
  assert.equal(h.state.busy, false);
});
