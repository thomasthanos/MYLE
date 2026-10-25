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
const exposeState = `
globalThis.pinView = {
  get offer() { return offer; },
  get manual() { return manual; },
  get notificationsDisabled() { return notificationsDisabled; },
  openNotifications: () => openNotificationSettings(),
  pin, dismiss,
};`;
const code = transpile(compileModule(transpile(source) + exposeState, { filename: "taskbar-choice.svelte.js", generate: "client" }).js.code, ModuleKind.CommonJS);
const require = createRequire(import.meta.url);
const tick = () => new Promise(setImmediate);

async function harness({ pending = true, supported = true, requestAfterSetup = false, focused = true, pinned = true, pinResults, pinError } = {}) {
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
      if (command === "taskbar_pin") {
        if (pinError) throw pinError;
        return pinResults?.shift() ?? { status: pinned ? "pinned" : "notPinned" };
      }
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
  assert.equal(h.pin.offer?.pending, true);
  assert.equal(h.notices[0].kind, "info");
  h.focus(true); await tick();
  assert.equal(h.calls.filter((call) => call === "taskbar_pin").length, 1);
  await h.pin.pin();
  assert.equal(h.calls.filter((call) => call === "taskbar_pin").length, 2);
});

test("Windows denying confirmation keeps the choice pending and shows the manual route", async () => {
  const h = await harness({ pinResults: [{ status: "notAllowed", reason: "unknown" }] });
  await h.pin.pin();
  assert.equal(h.pin.offer?.pending, true);
  assert.equal(h.pin.manual, true);
  assert.equal(h.notices.some((notice) => notice.kind === "success"), false);
  assert.equal(h.calls.includes("taskbar_pin_dismiss"), false);
});

test("a blocked setup request waits for an explicit retry after Windows settings change", async () => {
  const h = await harness({ requestAfterSetup: true, pinResults: [{ status: "notAllowed", reason: "unknown" }, { status: "pinned" }] });
  h.focus(false); h.focus(true); await tick();
  assert.equal(h.calls.filter((call) => call === "taskbar_pin").length, 1);
  assert.equal(h.pin.manual, true);
  await h.pin.pin();
  assert.equal(h.calls.filter((call) => call === "taskbar_pin").length, 2);
  assert.equal(h.pin.offer, null);
  assert.equal(h.notices.at(-1).kind, "success");
});

test("disabled pin consent notifications expose the settings action without recording a pin", async () => {
  const h = await harness({ pinResults: [{ status: "notAllowed", reason: "pinConsentNotificationsDisabled" }] });
  await h.pin.pin();
  assert.equal(h.pin.notificationsDisabled, true);
  await h.pin.openNotifications();
  assert.equal(h.calls.at(-1), "taskbar_open_notification_settings");
  assert.equal(h.pin.offer?.pending, true);
  assert.equal(h.notices.some((notice) => notice.kind === "success"), false);
});

test("an unidentified Windows denial does not claim notifications are disabled", async () => {
  const h = await harness({ pinResults: [{ status: "notAllowed", reason: "unknown" }] });
  await h.pin.pin();
  assert.equal(h.pin.manual, true);
  assert.equal(h.pin.notificationsDisabled, false);
});

test("a retry that reaches Windows confirmation removes the old notification warning", async () => {
  const h = await harness({ pinResults: [{ status: "notAllowed", reason: "pinConsentNotificationsDisabled" }, { status: "notPinned" }] });
  await h.pin.pin();
  assert.equal(h.pin.notificationsDisabled, true);
  await h.pin.pin();
  assert.equal(h.pin.notificationsDisabled, false);
  assert.equal(h.pin.manual, false);
  assert.equal(h.pin.offer?.pending, true);
  assert.equal(h.notices.at(-1).kind, "info");
});

test("an unexpected pin error keeps the suggestion available without reporting success", async () => {
  const h = await harness({ pinError: "The MYLE window is not open." });
  await h.pin.pin();
  assert.equal(h.pin.offer?.pending, true);
  assert.equal(h.notices.at(-1).kind, "error");
  assert.equal(h.notices.some((notice) => notice.kind === "success"), false);
});

test("dismissing records the decision and closes the suggestion", async () => {
  const h = await harness();
  await h.pin.dismiss();
  assert.equal(h.calls.at(-1), "taskbar_pin_dismiss");
  assert.equal(h.pin.offer, null);
});
