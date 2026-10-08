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