import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";

const values = new Map();
globalThis.localStorage = { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
globalThis.window = { addEventListener() {} };
globalThis.__projectSyncCalls = [];
const core = `export const invoke=(...args)=>globalThis.__projectSyncInvoke(...args); export const isTauri=()=>true; export class Channel {}`;
const pages = `const page=new Proxy({}, {get:(_,key)=>(...args)=>globalThis.__projectSyncCalls.push([key,...args])});
export const nav=page,settings=page,gameSavesState=page,diskState=page,appsState=page,cleanerState=page,maintenanceState=page,githubReleases=page;`;
const hooks = registerHooks({
  resolve(specifier, context, next) {
    if (specifier === "@tauri-apps/api/core") return { url: "test:project-core", shortCircuit: true };
    if (specifier === "@tauri-apps/api/event") return { url: "test:project-event", shortCircuit: true };
    if (context.parentURL?.endsWith("/account/snapshot.ts") && specifier.includes(".svelte")) return { url: "test:project-pages", shortCircuit: true };
    if (specifier.endsWith("/confirm.svelte") || specifier.endsWith("/toast.svelte")) return { url: "test:project-notices", shortCircuit: true };
    if (specifier.startsWith(".") && context.parentURL) {
      const base = new URL(specifier, context.parentURL);
      for (const suffix of [".ts", ".svelte.ts"]) if (existsSync(fileURLToPath(base) + suffix)) return next(base.href + suffix, context);
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    const virtual = { "test:project-core": core, "test:project-pages": pages, "test:project-event": "export const listen=async()=>()=>{};", "test:project-notices": "export const confirm=async()=>true; export const toast={success(){},info(){},error(){}};" };
    if (virtual[url]) return { format: "module", source: virtual[url], shortCircuit: true };
    if (!url.endsWith(".svelte.ts")) return next(url, context);
    const js = transpileModule(readFileSync(fileURLToPath(url), "utf8"), { compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext } }).outputText;
    return { format: "module", source: compileModule(js, { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
  },
});
const snapshot = await import("../../frontend/app/account/snapshot.ts");
const accountModule = await import("../../frontend/app/account/account.svelte.ts");
const { accountApi } = await import("../../frontend/app/account/api.ts");
hooks.deregister();
const paths = { version: 1, paths: ["H:\\Projects\\MYLE"] };
let localProjects;
const cloud = (extra = {}) => ({ version: 1, updatedAt: "2026-10-27T10:00:00.000Z", storage: {}, gameSaves: null, ...extra });
test.beforeEach(() => {
  values.clear(); globalThis.__projectSyncCalls = [];
  localProjects = structuredClone(paths);
  globalThis.__projectSyncInvoke = async (command, args) => {
    if (command === "github_releases_sync_export") return structuredClone(localProjects);
    if (command === "github_releases_sync_import") { localProjects = structuredClone(args.synced); return { repos: localProjects.paths.map(path => ({ id: "local", path })) }; }
    if (command === "game_saves_sync_export") return null;
    throw new Error(`Unexpected command ${command}`);
  };
});

test("owner snapshots export GitHub project paths and announce path changes as synced", async () => {
  const result = await snapshot.collect("2026-10-27T11:00:00.000Z", true);
  assert.deepEqual(result.githubReleases, paths);
  assert.equal(snapshot.isSynced("githubReleases.projects"), true);
});

test("non-owner snapshots do not request or carry owner-only project paths", async () => {
  const result = await snapshot.collect("2026-10-27T11:00:00.000Z", false);
  assert.ok(!("githubReleases" in result));
});

test("cloud project paths restore even when no Game Saves section exists", async () => {
  await snapshot.apply(cloud({ githubReleases: paths }), true);
  assert.ok(globalThis.__projectSyncCalls.some(([name]) => name === "applySyncedProjects"));
});

test("old snapshots leave local projects untouched, while an explicit empty list is restored", async () => {
  await snapshot.apply(cloud(), true);
  assert.ok(!globalThis.__projectSyncCalls.some(([name]) => name === "applySyncedProjects"));
  await snapshot.apply(cloud({ githubReleases: { version: 1, paths: [] } }), true);
  assert.ok(globalThis.__projectSyncCalls.some(([name]) => name === "applySyncedProjects"));
});

test("export failure is surfaced instead of replacing cloud projects with an empty list", async () => {
  globalThis.__projectSyncInvoke = async command => { if (command.startsWith("github_releases")) throw new Error("Settings locked"); return null; };
  await assert.rejects(snapshot.collect("now", true), /Settings locked/);
});

test("project import failure remains retryable instead of advancing the cloud timestamp", async () => {
  assert.equal(typeof accountModule.AccountState, "function");
  const account = new accountModule.AccountState();
  account.profile = { id: "owner" }; account.owner = true;
  values.set("myle.sync.updatedAt", "2026-10-26T10:00:00.000Z");
  accountApi.pull = async () => cloud({ githubReleases: paths });
  globalThis.__projectSyncInvoke = async () => { throw new Error("A build is running"); };
  await account.sync();
  assert.match(account.error, /build is running/);
  assert.equal(values.get("myle.sync.updatedAt"), "2026-10-26T10:00:00.000Z");
  assert.equal(account.lastSyncedAt, null);
});

test("existing local project paths are uploaded once when an older cloud snapshot lacks the section", async () => {
  const account = new accountModule.AccountState();
  account.profile = { id: "owner" }; account.owner = true;
  values.set("myle.sync.updatedAt", cloud().updatedAt);
  accountApi.pull = async () => cloud();
  const pushed = []; accountApi.push = async value => pushed.push(value);
  await account.sync();
  assert.equal(pushed.length, 1);
  assert.deepEqual(pushed[0].githubReleases, paths);
  assert.ok(pushed[0].updatedAt > cloud().updatedAt);
});

test("newer local theme preferences after reinstall cannot overwrite cloud project paths", async () => {
  const account = new accountModule.AccountState();
  account.profile = { id: "owner" }; account.owner = true;
  localProjects = { version: 1, paths: [] };
  values.set("myle.sync.updatedAt", "2030-01-01T00:00:00Z");
  accountApi.pull = async () => cloud({ githubReleases: paths, githubReleasesUpdatedAt: "2026-10-27T09:00:00Z" });
  const pushed = []; accountApi.push = async value => pushed.push(value);
  await account.sync();
  assert.equal(account.error, null);
  assert.deepEqual(localProjects, paths);
  assert.deepEqual(pushed[0].githubReleases, paths);
  assert.equal(pushed[0].githubReleasesUpdatedAt, "2026-10-27T09:00:00Z");
});

test("newer cloud preferences do not discard a more recent local project edit", async () => {
  const account = new accountModule.AccountState();
  account.profile = { id: "owner" }; account.owner = true;
  values.set("myle.sync.updatedAt", "2026-10-26T10:00:00Z");
  values.set(snapshot.PROJECTS_UPDATED_AT_KEY, "2026-10-27T11:00:00Z");
  accountApi.pull = async () => cloud({ updatedAt: "2026-10-27T12:00:00Z", githubReleasesUpdatedAt: "2026-10-27T09:00:00Z", githubReleases: { version: 1, paths: [] } });
  const pushed = []; accountApi.push = async value => pushed.push(value);
  await account.sync();
  assert.equal(account.error, null);
  assert.deepEqual(pushed[0].githubReleases, paths);
  assert.ok(!globalThis.__projectSyncCalls.some(([name]) => name === "applySyncedProjects"));
});

test("non-owner account sync never imports an owner-only cloud section", async () => {
  const account = new accountModule.AccountState();
  account.profile = { id: "non-owner" }; account.owner = false;
  accountApi.pull = async () => cloud({ githubReleases: paths });
  await account.sync();
  assert.equal(account.error, null);
  assert.ok(!globalThis.__projectSyncCalls.some(([name]) => name === "applySyncedProjects"));
});

test("an old account's late cloud reply cannot import paths after the account changes", async () => {
  const account = new accountModule.AccountState();
  account.profile = { id: "old-owner" }; account.owner = true;
  let reply; accountApi.pull = () => new Promise(resolve => { reply = resolve; });
  const syncing = account.sync();
  account.profile = { id: "another-user" }; account.owner = false;
  reply(cloud({ githubReleases: paths }));
  await syncing;
  assert.ok(!globalThis.__projectSyncCalls.some(([name]) => name === "applySyncedProjects"));
  assert.equal(account.lastSyncedAt, null);
});
