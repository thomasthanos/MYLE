import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";

const hooks = registerHooks({
  load(url, context, next) {
    if (!url.endsWith("/page-modules.svelte.ts")) return next(url, context);
    const source = transpileModule(readFileSync(fileURLToPath(url), "utf8"), {
      compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext },
    }).outputText;
    return { format: "module", source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
  },
});
const { PageModules } = await import("../../frontend/app/shell/page-modules.svelte.ts");
hooks.deregister();

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const PageA = () => {};
const PageB = () => {};

test("navigation shares a pending preload and reuses the loaded component", async () => {
  const modules = new PageModules();
  const answer = deferred();
  let requests = 0;
  const load = () => { requests++; return answer.promise; };
  const preload = modules.load("a", load);
  const navigation = modules.load("a", load);
  assert.equal(preload, navigation, "one in-flight load per page");
  answer.resolve({ default: PageA });
  await navigation;
  assert.equal(requests, 1);
  assert.equal(modules.loaded.a, PageA);
  await modules.load("a", load);
  assert.equal(requests, 1, "a loaded page must not request its module again");
});

test("a late successful preload does not erase another page's load error", async () => {
  const modules = new PageModules();
  const answer = deferred();
  const oldPage = modules.load("a", () => answer.promise);
  await modules.load("b", async () => { throw new Error("Page B unavailable"); });
  assert.equal(modules.errors.b, "Page B unavailable");
  answer.resolve({ default: PageA });
  await oldPage;
  assert.equal(modules.errors.b, "Page B unavailable");
  assert.equal(modules.loaded.a, PageA);
});

test("a failed module can be retried without keeping its previous error", async () => {
  const modules = new PageModules();
  await modules.load("b", async () => { throw "Offline"; });
  assert.equal(modules.errors.b, "Offline");
  const answer = deferred();
  const retry = modules.load("b", () => answer.promise);
  assert.equal(modules.errors.b, undefined, "retry shows loading instead of the old error");
  answer.resolve({ default: PageB });
  await retry;
  assert.equal(modules.loaded.b, PageB);
  assert.equal(modules.errors.b, undefined);
});

test("synchronous loader failures are reported and release the pending request", async () => {
  const modules = new PageModules();
  await modules.load("a", () => { throw new Error("Import failed"); });
  assert.equal(modules.errors.a, "Import failed");
  await modules.load("a", async () => ({ default: PageA }));
  assert.equal(modules.loaded.a, PageA);
  assert.equal(modules.errors.a, undefined);
});
