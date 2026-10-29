import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { compileModule } from "svelte/compiler";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";

const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, {
  compilerOptions: { target: ScriptTarget.ESNext, module },
}).outputText;
const source = readFileSync(new URL("../../frontend/app/account/account.svelte.ts", import.meta.url), "utf8")
  .replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");
const executable = transpile(compileModule(transpile(source) + "\nglobalThis.accountState = account;", {
  filename: "account-lifecycle.svelte.js", generate: "client",
}).js.code, ModuleKind.CommonJS);
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
};
const profile = { id: "fixture", name: "Fixture", email: null, avatarUrl: null, provider: "google" };
const cloud = { version: 1, updatedAt: "2026-10-28T12:00:00.000Z", storage: {}, gameSaves: null };

function harness(overrides = {}) {
  const values = new Map(), notices = [], listeners = [];
  const api = {
    profile: async () => profile, access: async () => false, signOut: async () => {},
    signIn: async () => profile, pull: async () => cloud, push: async () => {}, ...overrides,
  };
  const context = vm.createContext({
    require: createRequire(import.meta.url), exports: {}, setTimeout, clearTimeout,
    isTauri: () => true, window: { addEventListener() {} },
    listen: async (_event, callback) => { listeners.push(callback); return () => {}; }, ACCESS_EVENT: "account-access", onStorageChange() {},
    localStorage: { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) },
    readJson: () => null, confirm: async () => true, accountApi: api,
    toast: Object.fromEntries(["info", "success", "error"].map(kind => [kind, text => notices.push([kind, text])])),
    PROJECTS_UPDATED_AT_KEY: "projects", GITHUB_PROJECTS_CHANGE: "githubReleases.projects",
    isSynced: () => true, isSnapshot: value => value?.version === 1,
    collect: async updatedAt => ({ ...cloud, updatedAt }), apply: async () => null, applyGithubProjects: async () => {},
  });
  vm.runInContext(executable, context);
  const account = context.accountState;
  account.profile = profile;
  return { account, api, context, values, notices, listeners };
}

test("an access reply from before sign-out cannot reopen owner pages", async () => {
  const answer = deferred();
  const h = harness({ access: () => answer.promise });
  const checking = h.account.refreshAccess();
  await h.account.signOut();
  answer.resolve(true);
  await checking;
  assert.equal(h.account.profile, null);
  assert.equal(h.account.owner, false);
});

test("a late failed push cannot sign out a freshly signed-in session of the same account", async () => {
  const upload = deferred(), started = deferred();
  const h = harness({ pull: async () => null, push: () => { started.resolve(); return upload.promise; } });
  const syncing = h.account.sync();
  await started.promise;
  await h.account.signOut();
  h.api.pull = async () => ({ ...cloud, updatedAt: "2099-01-01T00:00:00.000Z" });
  await h.account.signIn("google");
  upload.reject(new Error("not signed in"));
  await syncing;
  assert.equal(h.account.profile?.id, "fixture");
  assert.equal(h.account.error, null);
});

test("a completed upload from an old session cannot acknowledge newer local changes", async () => {
  const upload = deferred(), started = deferred();
  const h = harness({ pull: async () => null, push: () => { started.resolve(); return upload.promise; } });
  const syncing = h.account.sync();
  await started.promise;
  await h.account.signOut();
  upload.resolve();
  await syncing;
  assert.equal(h.account.lastSyncedAt, null);
  assert.equal(h.values.has("myle.sync.lastSyncedAt"), false);
});

test("a stale initial profile read cannot overwrite a completed sign-in", async () => {
  const saved = deferred();
  const h = harness({ profile: () => saved.promise });
  h.account.profile = null;
  const initializing = h.account.init();
  await h.account.signIn("google");
  saved.resolve(null);
  await initializing;
  assert.equal(h.account.profile?.id, "fixture");
});

test("a delayed project import cannot continue applying old cloud preferences after sign-out", async () => {
  const importing = deferred();
  let preferenceApplies = 0;
  const h = harness({ pull: async () => ({ ...cloud, githubReleases: { version: 1, paths: [] } }) });
  h.account.owner = true;
  h.context.applyGithubProjects = () => importing.promise;
  h.context.apply = async () => { preferenceApplies++; return null; };
  const syncing = h.account.sync();
  await Promise.resolve();
  await h.account.signOut();
  importing.resolve();
  await syncing;
  assert.equal(preferenceApplies, 0);
  assert.equal(h.values.has("projects"), false);
  assert.equal(h.account.lastSyncedAt, null);
});

test("an expired session closes owner pages while preserving the sync error", async () => {
  const h = harness({ pull: async () => { throw new Error("Please sign in again"); } });
  h.account.owner = true;
  await h.account.sync();
  assert.equal(h.account.profile, null);
  assert.equal(h.account.owner, false);
  assert.equal(h.account.syncing, false);
  assert.match(h.account.error, /Please sign in again/);
});

test("an old sync finishing cannot release a new session's running sync", async () => {
  const upload = deferred(), uploading = deferred(), pull = deferred(), pulling = deferred();
  const h = harness({ pull: async () => null, push: () => { uploading.resolve(); return upload.promise; } });
  const oldSync = h.account.sync();
  await uploading.promise;
  await h.account.signOut();
  h.api.pull = () => { pulling.resolve(); return pull.promise; };
  const signingIn = h.account.signIn("google");
  await pulling.promise;
  upload.resolve();
  await oldSync;
  assert.equal(h.account.syncing, true);
  pull.resolve({ ...cloud, updatedAt: "2099-01-01T00:00:00.000Z" });
  await signingIn;
  assert.equal(h.account.syncing, false);
});

test("a signed-out session cannot acknowledge a delayed empty project export", async () => {
  const exporting = deferred(), started = deferred();
  const h = harness();
  h.account.owner = true;
  h.values.set("myle.sync.updatedAt", cloud.updatedAt);
  h.context.collect = () => { started.resolve(); return exporting.promise; };
  const syncing = h.account.sync();
  await started.promise;
  await h.account.signOut();
  exporting.resolve({ ...cloud, githubReleases: { version: 1, paths: [] } });
  await syncing;
  assert.equal(h.account.lastSyncedAt, null);
});

test("a delayed access event cannot reopen owner pages after sign-out", async () => {
  const h = harness();
  await h.account.init();
  await h.account.signOut();
  h.listeners[0]({ payload: true });
  assert.equal(h.account.owner, false);
});

test("cancelling sign-out preserves an access change received during confirmation", async () => {
  const h = harness(), answer = deferred();
  await h.account.init();
  h.context.confirm = () => answer.promise;
  const signingOut = h.account.signOut();
  h.listeners[0]({ payload: true });
  answer.resolve(false);
  await signingOut;
  assert.equal(h.account.profile?.id, "fixture");
  assert.equal(h.account.owner, true);
});

test("the latest access check wins when an older reply arrives last", async () => {
  const old = deferred(), current = deferred();
  const answers = [old, current];
  const h = harness({ access: () => answers.shift().promise });
  const first = h.account.refreshAccess();
  const second = h.account.refreshAccess(true);
  current.resolve(false);
  await second;
  old.resolve(true);
  await first;
  assert.equal(h.account.owner, false);
  assert.equal(h.account.accessKnown, true);
});

test("a backend access revocation supersedes a pending access reply", async () => {
  const h = harness(), answer = deferred();
  await h.account.init();
  h.api.access = () => answer.promise;
  const checking = h.account.refreshAccess();
  h.listeners[0]({ payload: false });
  answer.resolve(true);
  await checking;
  assert.equal(h.account.owner, false);
  assert.equal(h.account.accessKnown, true);
});
