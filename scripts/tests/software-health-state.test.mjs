import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";
const hooks = registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith(".") && context.parentURL) {
      const base = new URL(specifier, context.parentURL);
      for (const suffix of [".ts", ".svelte.ts"]) if (existsSync(fileURLToPath(base) + suffix)) return next(base.href + suffix, context);
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (!url.endsWith(".ts")) return next(url, context);
    const source = transpileModule(readFileSync(fileURLToPath(url), "utf8"), { compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext } }).outputText;
    return { format: "module", source: url.endsWith(".svelte.ts") ? compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code : source, shortCircuit: true };
  },
});
const { HealthState, healthApi } = await import("../../frontend/app/pages/system-cleaner/health.svelte.ts");
const { confirmState } = await import("../../frontend/lib/confirm.svelte.ts");
const { toast } = await import("../../frontend/lib/toast.svelte.ts");
hooks.deregister();
const row = (id, recommended = true) => ({ id, module: "redundant", title: id, detail: "cache", path: "C:\\cache\\" + id, size: 50, enabled: null, confidence: 90, flags: [], actions: ["recycle"], cleanupAction: "recycle", recommended, reviewReason: null });
const report = (findings) => ({ module: "redundant", findings, error: null });
let state, calls, rows;
test.beforeEach(() => {
  state = new HealthState(); calls = []; rows = [row("a"), row("b", false)];
  healthApi.scan = async (modules, onEvent) => { const r = report(rows); onEvent({ event: "result", data: { report: r } }); return [r]; };
  healthApi.restoreList = async () => [];
  healthApi.batch = async (items, onEvent) => { calls.push(items); return items.map(i => ({ id: i.id, ok: true, message: null })); };
});
test.afterEach(() => { if (confirmState.current) confirmState.answer(false); while (toast.visible.length) toast.dismiss(toast.visible[0].id); });
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
test("fresh scans select recommended while subsequent scans preserve deliberate deselection", async () => {
  await state.scan(); assert.deepEqual([...state.selected], ["a"]);
  state.toggle("a"); await state.scan(); assert.equal(state.selected.size, 0);
});
test("cancelling the batch preview performs no native changes and releases the operation", async () => {
  await state.scan(); const work = state.applyBatch([{ id: "a", action: "recycle" }]);
  await tick(); assert.equal(state.busy, true); confirmState.answer(false); await work;
  assert.equal(calls.length, 0); assert.equal(state.busy, false); assert.equal(state.selected.has("a"), true);
});
test("partial batch failure keeps failed items selected and retry clears stale errors", async () => {
  await state.scan(); state.toggle("b");
  healthApi.batch = async (items) => { calls.push(items); return items.map(i => ({ id: i.id, ok: i.id === "a", message: i.id === "b" ? "Close the app" : null })); };
  const work = state.applyBatch([{ id: "a", action: "recycle" }, { id: "b", action: "recycle" }]);
  await tick(); confirmState.answer(true); await work;
  assert.deepEqual([...state.selected], ["b"]); assert.equal(state.failures.b, "Close the app"); assert.equal(state.failures.a, undefined);
  healthApi.batch = async (items) => items.map(i => ({ id: i.id, ok: true, message: null }));
  const retry = state.retryFailed(); await tick(); confirmState.answer(true); await retry;
  assert.equal(state.failures.b, undefined); assert.equal(state.selected.size, 0);
});
test("scan and restore cannot race an open batch preview", async () => {
  await state.scan(); let scans = 0, restores = 0;
  healthApi.scan = async () => { scans++; return []; }; healthApi.undo = async () => { restores++; };
  const work = state.applyBatch([{ id: "a", action: "recycle" }]); await tick();
  await state.scan(); await state.undo({ id: "undo", title: "old", module: "redundant", at: 0, undo: { kind: "text" } });
  assert.equal(scans, 0); assert.equal(restores, 0); confirmState.answer(false); await work;
});
test("a lost batch reply refreshes affected categories and keeps unresolved selections", async () => {
  await state.scan(); let rescans = 0, histories = 0;
  healthApi.scan = async () => { rescans++; return [report(rows)]; };
  healthApi.restoreList = async () => { histories++; return []; };
  healthApi.batch = async () => { throw new Error("Reply lost"); };
  const work = state.applyBatch([{ id: "a", action: "recycle" }]);
  await tick(); confirmState.answer(true); await work;
  assert.equal(rescans, 1); assert.equal(histories, 1);
  assert.equal(state.selected.has("a"), true); assert.equal(state.lastResults[0].ok, false);
});
test("retry includes failures from earlier batches as well as the latest batch", async () => {
  await state.scan();
  healthApi.batch = async items => items.map(i => ({ id: i.id, ok: false, message: "Locked" }));
  for (const id of ["a", "b"]) {
    const work = state.applyBatch([{ id, action: "recycle" }]); await tick(); confirmState.answer(true); await work;
  }
  healthApi.batch = async items => { calls.push(items); return items.map(i => ({ id: i.id, ok: true, message: null })); };
  const retry = state.retryFailed(); await tick(); confirmState.answer(true); await retry;
  assert.deepEqual(calls[0].map(i => i.id).sort(), ["a", "b"]);
});
test("a transient module scan error preserves deliberate and failed selections until recovery", async () => {
  await state.scan(); state.toggle("b"); state.failures.b = "Locked";
  healthApi.scan = async () => [{ module: "redundant", findings: [], error: "Cannot read cache" }];
  await state.scan(); assert.deepEqual([...state.selected].sort(), ["a", "b"]);
  healthApi.scan = async () => [report(rows)];
  await state.scan(); assert.deepEqual(state.picked.map(f => f.id).sort(), ["a", "b"]);
  assert.equal(state.failures.b, "Locked");
});
