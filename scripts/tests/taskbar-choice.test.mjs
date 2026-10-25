import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { compileModule } from "svelte/compiler";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";

// Exercise the component's real lifecycle and callbacks, without invoking an
// OS pin dialog or needing a DOM for its presentation-only markup.
const component = readFileSync(new URL("../../frontend/lib/components/TaskbarPin.svelte", import.meta.url), "utf8");
const source = component.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1].replace(/^\s*import[^;]*;\r?\n/gm, "");
const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, { compilerOptions: { target: ScriptTarget.ESNext, module } }).outputText;
const code = transpile(compileModule(transpile(source) + "\nglobalThis.pinView = { get offer() { return offer; }, pin, dismiss };", { filename: "taskbar-choice.svelte.js", generate: "client" }).js.code, ModuleKind.CommonJS);
const require = createRequire(import.meta.url);
const tick = () => new Promise(setImmediate);

async function harness({ pending = true, supported = true, requestAfterSetup = false, focused = true, pinned = true } = {}) {
  let mount, onFocus;
  const calls = [], notices = [];
  const context = vm.createContext({
    require, exports: {}, isTauri: () => true,
    onMount: (callback) => { mount = callback; },
    getCurrentWindow: () => ({
      onFocusChanged: (callback) => { onFocus = callback; return Promise.resolve(() => {}); },
      isFocused: async () => focused,
    }),
    invoke: async (command) => {
      calls.push(command);
      if (command === "taskbar_pin_offer") return { pending, supported, requestAfterSetup };
      if (command === "taskbar_pin") return pinned;
    },
    toast: Object.fromEntries(["success", "error", "info"].map((kind) => [kind, (text) => notices.push({ kind, text })])),
  });
  vm.runInContext(code, context);
  const unmount = mount();
  await tick();
  return { pin: context.pinView, calls, notices, focus: (value) => onFocus({ payload: value }), unmount };
}

test("regular launches and updates never request a pin automatically", async () => {
  const h = await harness();
  h.focus(true); await tick();
  assert.deepEqual(h.calls, ["taskbar_pin_offer"]);
  assert.equal(h.pin.offer.pending, true);
});

test("a checked setup taskbar choice requests Windows confirmation once after focus", async () => {
  const h = await harness({ requestAfterSetup: true, focused: false });
  assert.deepEqual(h.calls, ["taskbar_pin_offer"]);
  h.focus(true); h.focus(true); await tick();
  assert.deepEqual(h.calls, ["taskbar_pin_offer", "taskbar_pin"]);
  assert.equal(h.pin.offer, null);
  assert.equal(h.notices[0].kind, "success");
});

test("a setup launch that is already focused also opens the confirmation", async () => {
  const h = await harness({ requestAfterSetup: true });
  assert.deepEqual(h.calls, ["taskbar_pin_offer", "taskbar_pin"]);
});

test("an opted-out or already handled request never pins", async () => {
  const h = await harness({ pending: false, requestAfterSetup: true });
  h.focus(true); await tick();
  assert.deepEqual(h.calls, ["taskbar_pin_offer"]);
});

test("unsupported Windows keeps the manual instructions without invoking the API", async () => {
  const h = await harness({ supported: false, requestAfterSetup: true });
  h.focus(true); await tick();
  assert.deepEqual(h.calls, ["taskbar_pin_offer"]);
  assert.equal(h.pin.offer.supported, false);
});

test("a refused pin is not reported as success and can be retried explicitly", async () => {
  const h = await harness({ requestAfterSetup: true, pinned: false });
  assert.equal(h.pin.offer.pending, true);
  assert.equal(h.notices[0].kind, "info");
  h.focus(true); await tick();
  assert.equal(h.calls.filter((call) => call === "taskbar_pin").length, 1);
  await h.pin.pin();
  assert.equal(h.calls.filter((call) => call === "taskbar_pin").length, 2);
});

test("dismissing records the decision and closes the suggestion", async () => {
  const h = await harness();
  await h.pin.dismiss();
  assert.equal(h.calls.at(-1), "taskbar_pin_dismiss");
  assert.equal(h.pin.offer, null);
});
