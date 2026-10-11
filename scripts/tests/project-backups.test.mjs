import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";

// The real page state, with only the native calls and the browser bits Node
// does not have replaced; see the Game Saves tests for the same pattern.
const storage = new Map();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  },
});
Object.defineProperty(globalThis, "isTauri", { configurable: true, value: true });

const hooks = registerHooks({
  resolve(specifier, context, next) {
    if (specifier === "svelte/reactivity") {
      return next(new URL("../../node_modules/svelte/src/reactivity/index-client.js", import.meta.url).href, context);
    }
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
    return {
      format: "module",
      source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code,
      shortCircuit: true,
    };
  },
});

const { projectBackupsState: state } = await import("../../frontend/app/pages/project-backups/state.svelte.ts");
const { projectBackupsApi: api } = await import("../../frontend/app/pages/project-backups/api.ts");
const { badges } = await import("../../frontend/lib/badges.svelte.ts");
const { toast } = await import("../../frontend/lib/toast.svelte.ts");
const { nav } = await import("../../frontend/lib/nav.svelte.ts");
hooks.deregister();

const DAY = 86_400_000;
const now = Date.UTC(2026, 9, 8, 12, 0, 0);

const project = (id, extra = {}) => ({
  id,
  name: id,
  appName: id,
  sourcePath: `H:\\Projects\\${id}`,
  closeApp: null,
  extraExclusions: [],
  keep: [],
  lastBackup: null,
  lastResult: null,
  ...extra,
});
const backedUp = (id, daysAgo, zipSize = 1_000) =>
  project(id, {
    lastBackup: { name: `${id}_D1_V1`, createdAt: now - daysAgo * DAY, fileCount: 3, zipSize },
    lastResult: { at: now - daysAgo * DAY, ok: true, cancelled: false, code: null, error: null },
  });

function setup(projects, missing = []) {
  state.page = {
    ...state.page,
    settings: { ...state.page.settings, provider: "googleDrive", cloudFolder: "G:\\My Drive", projects },
    missingSources: missing,
    backupRoot: "G:\\My Drive\\Projects Backup",
  };
  state.clock = now;
  state.query = "";
  state.filter = "all";
}

const keys = (groups) => groups.map((group) => group.key);

test.beforeEach(() => {
  setup([]);
});

test("a project is placed by what happened to it last", () => {
  setup(
    [
      backedUp("fresh", 1),
      backedUp("due", 10),
      backedUp("stale", 45),
      project("never"),
      project("moved"),
      project("failed", {
        lastBackup: { name: "failed_D1_V1", createdAt: now - 40 * DAY, fileCount: 1, zipSize: 10 },
        lastResult: { at: now - 2 * DAY, ok: false, cancelled: false, code: "CLOUD_NOT_READY", error: "not running" },
      }),
    ],
    ["moved"],
  );
  assert.equal(state.health(state.projects[0]), "ok");
  assert.equal(state.health(state.projects[1]), "due");
  assert.equal(state.health(state.projects[2]), "stale");
  assert.equal(state.health(state.projects[3]), "never");
  assert.equal(state.health(state.projects[4]), "missing");
  assert.equal(state.health(state.projects[5]), "failed");
  assert.equal(state.needsAttention(state.projects[0]), false);
  assert.equal(state.needsAttention(state.projects[1]), false, "ten days old is not a problem yet");
  assert.equal(state.needsAttention(state.projects[2]), true);
});

test("a cancelled attempt is told apart from a failed one", () => {
  setup([project("stopped", { lastResult: { at: now - 3_600_000, ok: false, cancelled: true, code: "CANCELLED", error: null } })]);
  assert.equal(state.health(state.projects[0]), "cancelled");
  assert.equal(state.needsAttention(state.projects[0]), true, "it has no backup at all");
  assert.deepEqual(keys(state.groups), ["never"], "…and it lands in the group that has no backup yet");
});

test("the summary counts what the page says about itself", () => {
  setup([
    backedUp("a", 1, 2_000),
    backedUp("b", 10, 3_000),
    backedUp("c", 60, 4_000),
    project("d"),
  ]);
  const summary = state.summary;
  assert.equal(summary.projects, 4);
  assert.equal(summary.ok, 2, "fresh and due count as up to date");
  assert.equal(summary.stale, 1);
  assert.equal(summary.never, 1);
  assert.equal(summary.attention, 2);
  assert.equal(summary.missing, 0);
  assert.equal(summary.backupBytes, 9_000, "every kept zip");
  assert.equal(summary.knownSizes, 3);
  assert.equal(summary.newest, now - DAY);
  assert.equal(summary.oldest, now - 60 * DAY);
});

test("the groups come in the order the user should read them", () => {
  setup(
    [
      backedUp("current", 1),
      backedUp("old", 40),
      project("waiting"),
      project("gone"),
      project("broken", { lastResult: { at: now - 60_000, ok: false, cancelled: false, code: "NO_FILES", error: "nothing to zip" } }),
    ],
    ["gone"],
  );
  assert.deepEqual(keys(state.groups), ["attention", "never", "stale", "ok", "missing"]);
  assert.deepEqual(state.groups.map((group) => group.projects.length), [1, 1, 1, 1, 1]);
  assert.equal(state.groups[0].projects[0].id, "broken");
  assert.equal(state.groups[4].projects[0].id, "gone");
});

test("a search filters inside the groups, not across them", () => {
  setup([backedUp("alpha", 1), backedUp("beta", 40), project("gamma")]);
  assert.equal(state.filtered.length, 3);
  state.query = "et";
  assert.deepEqual(state.filtered.map((one) => one.id), ["beta"]);
  assert.deepEqual(keys(state.groups), ["stale"], "an empty group is left out");
  state.query = "";
  state.setFilter("never");
  assert.deepEqual(state.groups.map((group) => group.projects[0].id), ["gamma"]);
  state.setFilter("all");
  assert.deepEqual(keys(state.groups), ["never", "stale", "ok"]);
});
const answer = (projectId, state, checkedAt, extra = {}) => ({
  projectId, state, backupId: null, added: 0, modified: 0, deleted: 0, unreadable: 0, files: [], checkedAt, message: null, ...extra,
});
const deferred = () => {
  let resolve;
  const promise = new Promise((yes) => (resolve = yes));
  return { promise, resolve };
};

test("a check started before a backup never replaces the answer after it", async () => {
  const original = api.changes;
  const pending = [];
  api.changes = () => {
    const call = deferred();
    pending.push(call);
    return call.promise;
  };
  try {
    state.changes = {};
    const stale = state.checkChanges(["A"]);
    // The backup of A ended: its check is always asked for, even while the
    // older one still runs.
    const fresh = state.checkChanges(["A"], true);
    assert.equal(pending.length, 2, "the check after the backup is not dropped");
    pending[1].resolve([answer("A", "upToDate", 2_000)]);
    await fresh;
    pending[0].resolve([answer("A", "changed", 1_000, { modified: 1 })]);
    await stale;
    assert.equal(state.changes.A.state, "upToDate");
    assert.deepEqual([...state.checking], [], "nothing is shown as being checked any more");
  } finally {
    api.changes = original;
  }
});

test("a backups folder out of reach or a failed check is not a change", () => {
  state.page = { ...state.page, settings: { ...state.page.settings, projects: ["A", "B", "C", "D", "E"].map((id) => project(id)) } };
  state.changes = {};
  state.applyChanges([
    answer("A", "unreachable", 1, { message: "Is Google Drive running?" }),
    answer("B", "error", 1, { message: "not a readable zip" }),
    answer("C", "changed", 1, { modified: 2 }),
    answer("D", "noBackup", 1, { added: 4 }),
    answer("E", "upToDate", 1),
  ]);
  assert.deepEqual(state.changedIds, ["C", "D"], "Back up changed takes only what changed or has no backup");
  assert.equal(state.changedCount, 1);
  assert.equal(badges.of("project-backups"), 1, "the sidebar shows the changed projects");
});

test("more changed projects while another page is open are told once", () => {
  while (toast.visible.length) toast.dismiss(toast.visible[0].id);
  state.page = { ...state.page, settings: { ...state.page.settings, projects: ["A", "B"].map((id) => project(id)) } };
  state.changes = {};
  state.applyChanges([answer("A", "upToDate", 10), answer("B", "upToDate", 10)]);
  nav.current = "install-apps";
  state.applyChanges([answer("A", "changed", 20, { modified: 1 })]);
  assert.equal(toast.visible.length, 1);
  assert.match(toast.visible[0].message, /1 project has changes not backed up yet/);
  // The same count again says nothing new.
  state.applyChanges([answer("A", "changed", 30, { modified: 2 })]);
  assert.equal(toast.visible[0].count, 1);
  assert.equal(badges.of("project-backups"), 1);
  while (toast.visible.length) toast.dismiss(toast.visible[0].id);
});

test("backing up the changes asks for an increment, Back up all does not", async () => {
  const original = { backup: api.backup, getState: api.getState, changes: api.changes };
  const asked = [];
  api.backup = async (ids, _onEvent, incremental) => {
    asked.push([ids, incremental]);
    return { results: [], cancelled: false, settings: state.page.settings };
  };
  api.getState = async () => state.page;
  api.changes = async () => [];
  try {
    state.page = { ...state.page, settings: { ...state.page.settings, provider: "googleDrive", projects: [project("A")] } };
    await state.backup(["A"], true);
    await state.backup(["A"]);
    assert.deepEqual(asked, [[["A"], true], [["A"], false]]);
  } finally {
    Object.assign(api, original);
  }
});

test("a restore names what it was put together from", async () => {
  while (toast.visible.length) toast.dismiss(toast.visible[0].id);
  const original = { restore: api.restore, pickFolder: api.pickFolder };
  api.pickFolder = async () => "D:\\Restored";
  api.restore = async (projectId, folder) => ({ folder, files: 12, bytes: 4096, from: ["A_D5_V12", "increment-V12-1", "increment-V12-2"] });
  try {
    await state.restore(project("A"));
    assert.match(toast.visible[0].message, /A: 12 files restored and checked from A_D5_V12 and 2 increments\./);
  } finally {
    Object.assign(api, original);
    while (toast.visible.length) toast.dismiss(toast.visible[0].id);
  }
});
