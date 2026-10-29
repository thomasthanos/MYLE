import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";
import { stageIsStep, stepReached } from "../../frontend/app/pages/github-releases/pipeline.ts";
const storage = new Map();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
} });
// The page's own state class, with its Svelte runes compiled as the frontend does.
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
    return { format: "module", source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
  },
});
const { BuildSession, ReleaseSession, failureKey, githubReleases } = await import("../../frontend/app/pages/github-releases/state.svelte.ts");
const { githubReleasesApi } = await import("../../frontend/app/pages/github-releases/api.ts");
const { onStorageChange } = await import("../../frontend/lib/storage.ts");
hooks.deregister();
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const repo = { id: "fixture", path: "C:/Synthetic/Fixture", name: "Fixture", exists: true };
const page = (repos = [repo]) => ({ repos, account: null, git: "git", gh: "gh", ai: { providers: [], order: [] }, gbrImport: null });
const status = (behind = 0) => ({ repoId: repo.id, path: repo.path, exists: true, isRepo: true, remote: null, branch: { branch: "main", head: null, upstream: "origin/main", ahead: 0, behind }, changes: 0, entries: [], workflows: [], problem: null });

test("an older repository status cannot replace a newer refresh", async () => {
  const original = githubReleasesApi.status;
  const state = new githubReleases.constructor(); state.page = page();
  const pending = deferred();
  githubReleasesApi.status = () => pending.promise;
  try {
    const older = state.refresh(repo.id);
    githubReleasesApi.status = async () => status(2);
    await state.refresh(repo.id);
    pending.resolve(status(0)); await older;
    assert.equal(state.statuses.fixture.branch.behind, 2);
  } finally { githubReleasesApi.status = original; }
});

test("an obsolete status failure leaves the current request loading", async () => {
  const original = githubReleasesApi.status;
  const state = new githubReleases.constructor(); state.page = page();
  const first = deferred(), second = deferred();
  githubReleasesApi.status = () => first.promise;
  try {
    const older = state.refresh(repo.id);
    githubReleasesApi.status = () => second.promise;
    const newer = state.refresh(repo.id);
    first.reject(new Error("old failure")); await older;
    assert.equal(state.refreshing.fixture, true);
    second.resolve(status(3)); await newer;
    assert.equal(state.statuses.fixture.problem, null);
  } finally { second.resolve(status()); githubReleasesApi.status = original; }
});

test("removing a repository discards its pending local and remote replies", async () => {
  const original = { status: githubReleasesApi.status, remote: githubReleasesApi.remote, removeRepo: githubReleasesApi.removeRepo };
  const state = new githubReleases.constructor(); state.page = page();
  const local = deferred(), remote = deferred();
  githubReleasesApi.status = () => local.promise;
  githubReleasesApi.remote = () => remote.promise;
  githubReleasesApi.removeRepo = async () => page([]);
  try {
    const reading = [state.refresh(repo.id), state.loadRemote(repo.id)];
    await state.removeRepo(repo.id);
    local.resolve(status(4)); remote.resolve({ releases: [], lastRelease: {}, run: null, problem: null });
    await Promise.all(reading);
    assert.equal(state.statuses.fixture, undefined);
    assert.equal(state.remotes.fixture, undefined);
    assert.equal(state.refreshing.fixture, undefined);
  } finally { Object.assign(githubReleasesApi, original); }
});

test("an older remote failure cannot replace a successful reload", async () => {
  const original = githubReleasesApi.remote;
  const state = new githubReleases.constructor(); state.page = page();
  const pending = deferred(); githubReleasesApi.remote = () => pending.promise;
  try {
    const older = state.loadRemote(repo.id);
    githubReleasesApi.remote = async () => ({ releases: [], lastRelease: {}, run: null, problem: null });
    await state.loadRemote(repo.id);
    pending.reject(new Error("outdated failure")); await older;
    assert.equal(state.remotes.fixture.problem, null);
  } finally { githubReleasesApi.remote = original; }
});

test("a quiet fetch received after removal cannot restore a repository or start a remote read", async () => {
  const original = { fetch: githubReleasesApi.fetch, remote: githubReleasesApi.remote, removeRepo: githubReleasesApi.removeRepo };
  const state = new githubReleases.constructor(); state.page = page();
  const pending = deferred(); let remotes = 0;
  githubReleasesApi.fetch = () => pending.promise;
  githubReleasesApi.remote = async () => { remotes++; return { releases: [], lastRelease: {}, run: null, problem: null }; };
  githubReleasesApi.removeRepo = async () => page([]);
  try {
    const fetching = state.fetchQuietly(repo.id);
    await state.removeRepo(repo.id);
    pending.resolve(status(4)); await fetching;
    assert.equal(state.statuses.fixture, undefined);
    assert.equal(remotes, 0);
  } finally { Object.assign(githubReleasesApi, original); }
});

test("a status read started before fetching cannot overwrite the fetched branch", async () => {
  const original = { status: githubReleasesApi.status, fetch: githubReleasesApi.fetch, remote: githubReleasesApi.remote };
  const state = new githubReleases.constructor(); state.page = page();
  const pending = deferred(); githubReleasesApi.status = () => pending.promise;
  githubReleasesApi.fetch = async () => status(3);
  githubReleasesApi.remote = async () => ({ releases: [], lastRelease: {}, run: null, problem: null });
  try {
    const reading = state.refresh(repo.id);
    await state.fetch(repo.id);
    pending.resolve(status(0)); await reading;
    assert.equal(state.statuses.fixture.branch.behind, 3);
    assert.equal(state.refreshing.fixture, undefined);
  } finally { Object.assign(githubReleasesApi, original); }
});

test("a build diagnostic flood retains a bounded recent window through completion", () => {
  const session = new BuildSession(); session.reset("synthetic", false);
  const diagnostic = index => ({ severity: "error", message: `error ${index}`, file: null, line: null, column: null, code: null, tool: "fixture", logLine: index });
  for (let index = 0; index < 1500; index++) session.apply({ event: "diagnostic", data: { diagnostic: diagnostic(index) } });
  assert.equal(session.diagnostics.length, 1000);
  assert.equal(session.diagnostics[0].message, "error 500");
  session.finish({ ok: false, cancelled: false, exitCode: 1, durationMs: 10, lines: 1500, errors: 1500, warnings: 0, diagnostics: Array.from({ length: 1000 }, (_, index) => diagnostic(index)), logPath: null, failure: null });
  assert.equal(session.diagnostics.at(-1).message, "error 1499");
});
test("adding and removing project paths notify account sync only after persistence succeeds", async () => {
  const original = { addRepos: githubReleasesApi.addRepos, removeRepo: githubReleasesApi.removeRepo, refresh: githubReleases.refresh };
  const changes = []; onStorageChange(key => changes.push(key));
  githubReleases.refresh = async () => {};
  githubReleases.page = { repos: [], account: null, git: null, gh: null, ai: { providers: [], order: [] }, gbrImport: null };
  githubReleasesApi.addRepos = async () => ({ ...githubReleases.page, repos: [{ id: "repo", name: "One", path: "C:/Code/One", exists: true }] });
  githubReleasesApi.removeRepo = async () => ({ ...githubReleases.page, repos: [] });
  try {
    await githubReleases.addRepos(["C:/Code/One"]);
    await githubReleases.removeRepo("repo");
    assert.equal(changes.filter(key => key === "githubReleases.projects").length, 2);
    githubReleasesApi.addRepos = async () => { throw new Error("Not saved"); };
    await assert.rejects(githubReleases.addRepos(["C:/Code/Two"]), /Not saved/);
    assert.equal(changes.filter(key => key === "githubReleases.projects").length, 2);
  } finally {
    Object.assign(githubReleasesApi, { addRepos: original.addRepos, removeRepo: original.removeRepo });
    githubReleases.refresh = original.refresh;
    onStorageChange(() => {});
  }
});
test("an older page-load reply cannot overwrite project paths restored while loading", async () => {
  const original = githubReleasesApi.getState;
  let reply;
  githubReleasesApi.getState = () => new Promise(resolve => { reply = resolve; });
  const page = { repos: [], account: null, git: "git", gh: "gh", ai: { providers: [], order: [] }, gbrImport: null };
  try {
    const initializing = githubReleases.init();
    await githubReleases.applySyncedProjects(page);
    reply({ ...page, repos: [{ id: "stale", path: "C:/Old", name: "Old", exists: true }] });
    await initializing;
    assert.deepEqual(githubReleases.repos, []);
    assert.equal(githubReleases.page.git, "git");
  } finally { githubReleasesApi.getState = original; }
});
test("a later failure report adds to the failures shown and never wipes them", () => {
  const session = new ReleaseSession();
  const job = (name, url, errors = []) => ({ job: name, step: "Build", errors, logTail: "", url });
  session.start();
  session.apply({ event: "failure", data: { jobs: [job("build", "https://github.com/a/b/runs/1")] } });
  // The same run once it ended: its job, now with its errors, stays where it was.
  session.apply({ event: "failure", data: { jobs: [job("build", "https://github.com/a/b/runs/1", ["error: x"])] } });
  // Another workflow on the tag with a job of the same name: shown beside it.
  session.apply({ event: "failure", data: { jobs: [job("build", "https://github.com/a/b/runs/9")] } });
  assert.deepEqual(session.failures.map((f) => [f.url, f.errors.length]), [
    ["https://github.com/a/b/runs/1", 1],
    ["https://github.com/a/b/runs/9", 0],
  ]);
  assert.equal(new Set(session.failures.map(failureKey)).size, 2, "each failure has its own key on the page");
  session.start();
  assert.deepEqual(session.failures, [], "a new release starts with none");
});
test("a build stage is a pipeline step by its words, with a version after it but not more words", () => {
  assert.ok(stageIsStep("App 9.7.2", "App"));
  assert.ok(stageIsStep("Setup window", "Setup window"));
  assert.ok(stageIsStep("setup", "Setup"));
  assert.ok(!stageIsStep("Setup window", "Setup"));
  assert.ok(!stageIsStep("Compiling Rust: serde (3 crates)", "App"));
  assert.ok(!stageIsStep("App", ""));
});
test("the pipeline follows MYLE's build-setup.ps1 markers in order, never back", () => {
  const titles = ["App", "Setup window", "Uninstaller", "Payload", "Setup"];
  assert.equal(stepReached([], titles), -1);
  assert.equal(stepReached(["App 9.8.0", "Compiling Rust: myle (412 crates)"], titles), 0);
  assert.equal(stepReached(["App 9.8.0", "Setup window", "Bundling with Vite", "Uninstaller"], titles), 2);
  // "Setup window" must not count as the last step "Setup".
  assert.equal(stepReached(["App 9.8.0", "Setup window"], titles), 1);
  assert.equal(stepReached(["App 9.8.0", "Setup window", "Uninstaller", "Payload", "Setup", "Done: MYLE.exe (18 MB)"], titles), 4);
  assert.equal(stepReached(["Uninstaller", "App 9.8.0"], titles), 2);
});
