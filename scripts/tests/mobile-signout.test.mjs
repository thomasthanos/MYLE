import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { compileModule } from "svelte/compiler";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";
const source = readFileSync(new URL("../../frontend/mobile/account.svelte.ts", import.meta.url), "utf8")
  .replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");
const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, {
  compilerOptions: { target: ScriptTarget.ESNext, module },
}).outputText;
const executable = transpile(compileModule(transpile(source) + "\nglobalThis.mobileAccount = account;", {
  filename: "mobile-account.svelte.js", generate: "client",
}).js.code, ModuleKind.CommonJS);
function harness({ accept = true, fail = false, signIn, profile = async () => null, syncNow = async () => {}, cancelSignIn = async () => true } = {}) {
  const calls = [], questions = [], errors = [], timers = [];
  const listeners = [];
  const context = vm.createContext({
    require: createRequire(import.meta.url), exports: {}, URLSearchParams, navigator: { userAgent: "Android" },
    isTauri: () => true, shell: { settingsOpen: true }, document: { visibilityState: "visible" },
    setTimeout: (run) => { timers.push(run); return timers.length; },
    invoke: (command) => { assert.equal(command, "account_cancel_sign_in"); return cancelSignIn(); },
    listen: async (event, callback) => { listeners.push({ event, callback }); return () => {}; },
    confirm: async (options) => { questions.push(options); return accept; },
    passwords: { lock: async () => calls.push("lock"), resetLocal: () => calls.push("reset"), syncNow },
    accountApi: { signIn, profile, cancelSignIn, signOut: async () => { calls.push("native"); if (fail) throw new Error("file cannot be removed"); } },
    localStorage: { clear: () => calls.push("localStorage") }, sessionStorage: { clear: () => calls.push("sessionStorage") },
    toast: { info() {}, success() {}, error: (message) => errors.push(message) },
  });
  vm.runInContext(executable, context);
  const account = context.mobileAccount;
  account.profile = { id: "fixture", name: "Fixture", email: "fixture@example.com" };
  return { account, calls, questions, errors, timers, listeners, shell: context.shell };
}
test("confirmed sign out clears the phone and tells the shell to show welcome", async () => {
  const h = harness();
  await h.account.signOut();
  assert.deepEqual(h.calls, ["lock", "native", "reset", "localStorage", "sessionStorage"]);
  assert.equal(h.account.profile, null);
  assert.equal(h.account.resetVersion, 1);
  assert.equal(h.account.signingOut, false);
  assert.equal(h.shell.settingsOpen, false);
  assert.match(h.questions[0].message, /have not synced will be lost/);
});
test("cancelling sign out preserves the vault and account", async () => {
  const h = harness({ accept: false });
  await h.account.signOut();
  assert.equal(h.calls.length, 0);
  assert.equal(h.account.profile.id, "fixture");
  assert.equal(h.account.resetVersion, 0);
});
test("cleanup failure keeps a locked retryable account and reports the error", async () => {
  const h = harness({ fail: true });
  await h.account.signOut();
  assert.deepEqual(h.calls, ["lock", "native"]);
  assert.equal(h.account.profile.id, "fixture");
  assert.equal(h.account.resetVersion, 0);
  assert.equal(h.account.signingOut, false);
  assert.match(h.errors[0], /Could not sign out/);
});
test("a delayed sign-in response cannot restore an account after sign out", async () => {
  let answer;
  const h = harness({ signIn: () => new Promise((resolve) => { answer = resolve; }) });
  const pending = h.account.signIn("discord");
  await h.account.signOut();
  answer({ id: "late", name: "Late fixture" });
  await pending;
  assert.equal(h.account.profile, null);
  assert.equal(h.errors.length, 0);
});
test("a cancelled sign-in response is ignored", async () => {
  let answer;
  const h = harness({ signIn: () => new Promise((resolve) => { answer = resolve; }) });
  const pending = h.account.signIn("google");
  await h.account.cancelSignIn();
  answer({ id: "late", name: "Late fixture" });
  await pending;
  assert.equal(h.account.profile.id, "fixture");
  assert.equal(h.errors.length, 0);
});

test("resuming during a slow OAuth callback keeps its eventual successful account", async () => {
  let answer;
  const h = harness({ signIn: () => new Promise((resolve) => { answer = resolve; }), cancelSignIn: async () => false });
  h.account.profile = null;
  const pending = h.account.signIn("google");
  h.account.resumed();
  await h.timers[0]();
  answer({ id: "returned", name: "Returned fixture" });
  await pending;
  assert.equal(h.account.profile?.id, "returned");
  assert.equal(h.account.signingIn, null);
  assert.equal(h.errors.length, 0);
});

test("an old resume timer cannot cancel a new sign-in to the same provider", async () => {
  const answers = [];
  let cancellations = 0;
  const h = harness({
    signIn: () => new Promise((resolve) => answers.push(resolve)),
    cancelSignIn: async () => { cancellations++; return true; },
  });
  const first = h.account.signIn("google");
  h.account.resumed();
  await h.account.cancelSignIn();
  const second = h.account.signIn("google");
  await h.timers[0]();
  answers[0]({ id: "stale" });
  answers[1]({ id: "current", name: "Current fixture" });
  await Promise.all([first, second]);
  assert.equal(h.account.profile?.id, "current");
  assert.equal(cancellations, 1);
});

test("repeated initialization registers one account-return listener", async () => {
  const h = harness();
  await Promise.all([h.account.init(), h.account.init()]);
  assert.equal(h.listeners.length, 1);
  assert.equal(h.account.loaded, true);
});

test("the initial profile read cannot restore an account after sign-out", async () => {
  let answer, started;
  const reading = new Promise(resolve => { started = resolve; });
  const h = harness({ profile: () => { started(); return new Promise(resolve => { answer = resolve; }); } });
  const initializing = h.account.init();
  await reading;
  await h.account.signOut();
  answer({ id: "signed-out", name: "Old fixture" });
  await initializing;
  assert.equal(h.account.profile, null);
  assert.equal(h.account.loaded, true);
});

test("the initial profile read cannot replace a completed sign-in", async () => {
  let answer, started;
  const reading = new Promise(resolve => { started = resolve; });
  const h = harness({
    profile: () => { started(); return new Promise(resolve => { answer = resolve; }); },
    signIn: async () => ({ id: "current", name: "Current fixture" }),
  });
  h.account.profile = null;
  const initializing = h.account.init();
  await reading;
  await h.account.signIn("google");
  answer(null);
  await initializing;
  assert.equal(h.account.profile?.id, "current");
  assert.equal(h.account.loaded, true);
});

test("the initial profile read cannot replace an accepted browser-return event", async () => {
  let answer, started, syncFinished;
  const reading = new Promise(resolve => { started = resolve; });
  const syncing = new Promise(resolve => { syncFinished = resolve; });
  const current = { id: "returned", name: "Returned fixture" };
  let reads = 0;
  const h = harness({
    profile: () => ++reads === 1 ? new Promise(resolve => { answer = resolve; started(); }) : Promise.resolve(current),
    syncNow: async () => { syncFinished(); },
  });
  h.account.profile = null;
  const initializing = h.account.init();
  await reading;
  h.listeners[0].callback({ payload: current });
  await syncing;
  answer(null);
  await initializing;
  assert.equal(h.account.profile?.id, "returned");
});

test("an old browser-return event cannot invalidate a newer explicit sign-in", async () => {
  let answer, reads = 0;
  const h = harness({
    profile: async () => ++reads === 1 ? null : { id: "old", name: "Old fixture" },
    signIn: () => new Promise(resolve => { answer = resolve; }),
  });
  h.account.profile = null;
  await h.account.init();
  const signingIn = h.account.signIn("google");
  h.listeners[0].callback({ payload: { id: "old", name: "Old fixture" } });
  answer({ id: "current", name: "Current fixture" });
  await signingIn;
  assert.equal(h.account.profile?.id, "current");
  assert.equal(h.account.signingIn, null);
});
