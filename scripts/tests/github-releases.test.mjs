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
const { ReleaseSession, failureKey } = await import("../../frontend/app/pages/github-releases/state.svelte.ts");
hooks.deregister();

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
