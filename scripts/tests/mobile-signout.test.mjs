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
function harness({ accept = true, fail = false, signIn, cancelSignIn = async () => true } = {}) {
  const calls = [], questions = [], errors = [], timers = [];
  const context = vm.createContext({
    require: createRequire(import.meta.url), exports: {}, URLSearchParams, navigator: { userAgent: "Android" },
    isTauri: () => true, shell: { settingsOpen: true }, document: { visibilityState: "visible" },
    setTimeout: (run) => { timers.push(run); return timers.length; },
    invoke: (command) => { assert.equal(command, "account_cancel_sign_in"); return cancelSignIn(); },
    confirm: async (options) => { questions.push(options); return accept; },
    passwords: { lock: async () => calls.push("lock"), resetLocal: () => calls.push("reset"), syncNow: async () => {} },
    accountApi: { signIn, cancelSignIn, signOut: async () => { calls.push("native"); if (fail) throw new Error("file cannot be removed"); } },
    localStorage: { clear: () => calls.push("localStorage") }, sessionStorage: { clear: () => calls.push("sessionStorage") },
    toast: { info() {}, success() {}, error: (message) => errors.push(message) },
  });
  vm.runInContext(executable, context);
  const account = context.mobileAccount;
  account.profile = { id: "fixture", name: "Fixture", email: "fixture@example.com" };
  return { account, calls, questions, errors, timers, shell: context.shell };
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
