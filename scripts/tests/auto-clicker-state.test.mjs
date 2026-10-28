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
    if (!url.endsWith(".svelte.ts")) return next(url, context);
    const source = transpileModule(readFileSync(fileURLToPath(url), "utf8"), { compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext } }).outputText;
    return { format: "module", source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
  },
});
const { AutoClickerState } = await import("../../frontend/app/pages/auto-clicker/state.svelte.ts");
const { newProfile } = await import("../../frontend/app/pages/auto-clicker/model.ts");
hooks.deregister();
const ready = { revision: 1, runId: 0, phase: "ready", mode: "simple", loop: 0, loops: null, step: 0, steps: 0, click: 0, clicks: 0, completed: 0, total: null, countdown: 0, hotkey: true, hotkeyError: null, error: null };
const deferred = () => { let resolve; const promise = new Promise(r => resolve = r); return { promise, resolve }; };
function setup(overrides = {}) {
  const p = newProfile(), writes = [], configs = [];
  let listener;
  const api = { load: async () => ({ version: 1, activeId: p.id, items: [p] }), status: async () => ready,
    save: async doc => { writes.push(structuredClone(doc)); }, configure: async config => { configs.push(config); },
    listen: async cb => { listener = cb; return () => {}; }, start: async () => ({ ...ready, revision: 2, runId: 1, phase: "countdown", countdown: 3 }),
    stop: async () => ({ ...ready, revision: 4, runId: 1, phase: "stopped" }), pick: async () => null, ...overrides };
  return { s: new AutoClickerState(api), writes, configs, event: s => listener(s) };
}
test("late status response cannot rewind a newer running event", async () => {
  const pending = deferred();
  const { s, event } = setup({ status: () => pending.promise });
  const loading = s.load();
  await Promise.resolve();
  event({ ...ready, revision: 9, runId: 2, phase: "running", completed: 22 });
  pending.resolve(ready);
  await loading;
  assert.equal(s.status.completed, 22);
  assert.equal(s.busy, true);
});
test("invalid edits disarm F6 and do not overwrite the last valid saved profiles", async () => {
  const { s, configs, writes } = setup(); await s.load();
  s.edit({ intervalMs: 0 });
  await s.save();
  await s.configured();
  assert.equal(configs.at(-1), null);
  assert.equal(writes.length, 0);
  assert.ok(s.validation);
  s.edit({ intervalMs: 250 });
  await s.save();
  assert.equal(writes.at(-1).items[0].config.intervalMs, 250);
});
test("profiles can be created renamed removed and never lose the last profile", async () => {
  const { s, writes } = setup(); await s.load();
  const original = s.active.id;
  s.addProfile(); s.renameProfile("Second");
  assert.equal(s.profiles.items.length, 2);
  assert.equal(s.active.name, "Second");
  s.removeProfile();
  assert.equal(s.active.id, original);
  s.removeProfile();
  assert.equal(s.profiles.items.length, 1);
  await s.save();
  assert.equal(writes.at(-1).activeId, original);
});
test("cancelled location capture ignores a late coordinate response", async () => {
  const pending = deferred();
  const { s } = setup({ pick: () => pending.promise }); await s.load();
  const step = s.active.config.steps[0];
  const picking = s.pick(step.id);
  await s.stop();
  pending.resolve({ x: 700, y: -300 });
  await picking;
  assert.equal(s.active.config.steps[0].x, 0);
  assert.equal(s.pickingId, null);
});
test("failed profile loading never writes fresh defaults over the original file", async () => {
  const { s, writes } = setup({ load: async () => { throw new Error("corrupt profiles"); } });
  await s.load(); s.addProfile(); await s.save();
  assert.equal(s.profiles, null);
  assert.equal(writes.length, 0);
  assert.match(s.error, /corrupt profiles/);
});
test("Stop cancels a Start still waiting for configuration without restarting later", async () => {
  const configure = deferred();
  let starts = 0;
  const { s } = setup({ configure: () => configure.promise, start: async () => { starts++; return { ...ready, revision: 5, phase: "countdown" }; } });
  await s.load();
  const starting = s.start();
  await s.stop();
  configure.resolve();
  await starting;
  assert.equal(starts, 0);
  assert.equal(s.status.phase, "stopped");
  assert.equal(s.busy, false);
});
test("Stop is repeated after a late Start acknowledgement", async () => {
  const pending = deferred();
  let stops = 0;
  const { s } = setup({ start: () => pending.promise, stop: async () => ({ ...ready, revision: ++stops + 10, phase: "stopped" }) });
  await s.load(); await s.configured();
  const starting = s.start();
  await Promise.resolve(); await Promise.resolve();
  await s.stop();
  pending.resolve({ ...ready, revision: 10, runId: 1, phase: "countdown" });
  await starting;
  assert.equal(s.status.phase, "stopped");
  assert.equal(stops, 2, "the in-flight start must be stopped after it acknowledges");
});
test("picker cancellation addresses the same native request even before its status event", async () => {
  const pending = deferred();
  let requested, cancelled;
  const { s } = setup({ pick: id => { requested = id; return pending.promise; }, stop: async id => { cancelled = id; return ready; } });
  await s.load();
  const picking = s.pick(s.active.config.steps[0].id);
  await s.stop();
  pending.resolve(null);
  await picking;
  assert.match(requested, /^[0-9a-f-]{36}$/);
  assert.equal(cancelled, requested);
  assert.equal(s.busy, false);
});
