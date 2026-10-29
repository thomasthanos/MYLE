import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { ScriptTarget, ModuleKind, transpileModule } from "typescript";

const source = readFileSync(new URL("../../frontend/lib/startup.ts", import.meta.url), "utf8")
  .replace(/^import[\s\S]*?;\r?\n/gm, "").replace(/^export /gm, "");
const executable = transpileModule(source + "\nglobalThis.startup = { whenShown };", {
  compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext },
}).outputText;

function harness({ visible = false, delayedRegistration = false } = {}) {
  const listeners = new Set(), timers = new Map();
  let nextTimer = 0, register;
  const registration = new Promise(resolve => { register = resolve; });
  const context = vm.createContext({
    isTauri: () => true,
    getCurrentWindow: () => ({ isVisible: async () => visible }),
    listen: (_event, callback) => {
      listeners.add(callback);
      const unlisten = () => listeners.delete(callback);
      return delayedRegistration ? registration.then(() => unlisten) : Promise.resolve(unlisten);
    },
    setTimeout: callback => { timers.set(++nextTimer, callback); return nextTimer; },
    clearTimeout: timer => timers.delete(timer),
  });
  vm.runInContext(executable, context);
  return {
    whenShown: context.startup.whenShown, listeners, timers, register,
    show: () => { for (const listener of [...listeners]) listener(); },
    expire: () => { for (const [id, callback] of [...timers]) { timers.delete(id); callback(); } },
  };
}

test("an already visible window releases its startup listener and fallback timer", async () => {
  const h = harness({ visible: true });
  await h.whenShown();
  assert.equal(h.listeners.size, 0);
  assert.equal(h.timers.size, 0);
});

test("a shown event releases a listener that finishes registering after the event", async () => {
  const h = harness({ delayedRegistration: true });
  const shown = h.whenShown();
  h.show();
  await shown;
  assert.equal(h.timers.size, 0);
  h.register();
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(h.listeners.size, 0);
});

test("the startup timeout releases the listener and later callers reuse the settled result", async () => {
  const h = harness();
  const shown = h.whenShown();
  await Promise.resolve();
  h.expire();
  await shown;
  assert.equal(h.listeners.size, 0);
  assert.equal(h.timers.size, 0);
  assert.equal(h.whenShown(), shown);
});
