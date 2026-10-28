import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
const source = (await readFile(new URL("../../extension/content.js", import.meta.url), "utf8")).replace(/\r\n/g, "\n");
function namingPage() {
  const start = source.indexOf("  // --- Passkey naming");
  const end = source.indexOf("  const busyLine", start);
  assert.ok(start >= 0 && end > start, "the naming helper is present");
  const dialogs = [];
  const inputs = [];
  const writes = [];
  const listeners = new Map();
  const timers = new Map();
  const roots = [];
  let observer;
  let nextTimer = 1;
  class Input {
    constructor(extra = {}) {
      Object.assign(this, { type: "text", value: "random-device-123", name: "passkeyName", id: "", autocomplete: "off", isConnected: true, disabled: false, readOnly: false, shown: true, attrs: {}, labels: [] }, extra);
    }
    getAttribute(key) { return this.attrs[key] ?? null; }
  }
  const document = {
    documentElement: {},
    getElementById() { return null; },
    addEventListener(kind, fn) { listeners.set(kind, fn); },
    removeEventListener(kind, fn) { if (listeners.get(kind) === fn) listeners.delete(kind); },
  };
  const scope = vm.createContext({
    document, HTMLInputElement: Input, shadowRoots: () => roots,
    deepAll: (selector) => selector === "input" ? inputs : dialogs,
    usable: (field) => field instanceof Input && field.isConnected && field.shown && !field.disabled && !field.readOnly,
    visible: (element) => element.shown !== false,
    getAttr: (element, key) => element.getAttribute(key),
    setValue: (field, value) => { writes.push([field, value]); field.value = value; },
    MutationObserver: class {
      constructor(fn) { this.changed = fn; this.connected = false; observer = this; }
      observe(root) { this.connected = true; (this.observed ??= new Set()).add(root); }
      disconnect() { this.connected = false; }
    },
    setTimeout: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms }); return id; },
    clearTimeout: (id) => timers.delete(id),
    addEventListener: (kind, fn) => listeners.set(kind, fn),
  });
  vm.runInContext(source.slice(start, end), scope);
  function dialog(title, fields = [new Input()], extra = {}) {
    const value = { shown: true, attrs: {},
      getAttribute(key) { return this.attrs[key] ?? null; },
      querySelector(selector) { return selector.includes("heading") ? { textContent: title } : null; },
      querySelectorAll() { return fields; },
      ...extra,
    };
    dialogs.push(value);
    inputs.push(...fields);
    return fields;
  }
  const capture = () => scope.capturePasskeyNaming();
  const startNaming = (before = capture()) => scope.startPasskeyNaming("new-credential", before);
  function changed() {
    observer.changed();
    for (const [id, timer] of [...timers]) {
      if (timer.ms < 1000) { timers.delete(id); timer.fn(); }
    }
  }
  return { scope, document, roots, inputs, dialogs, writes, listeners, timers, Input, dialog, capture, startNaming, changed, get observer() { return observer; } };
}
for (const title of ["Name your new passkey", "Ονομάστε το νέο κλειδί πρόσβασης", "Όνομα νέου κλειδιού πρόσβασης"]) {
  test(`a new naming field is filled with MYLE: ${title}`, () => {
    const page = namingPage();
    page.startNaming();
    const [field] = page.dialog(title);
    page.changed();
    assert.deepEqual(page.writes, [[field, "MYLE"]]);
    assert.equal(page.observer.connected, false, "one fill finishes the helper");
    assert.equal(page.timers.size, 0);
    field.value = "My own name";
    page.changed();
    assert.equal(field.value, "My own name", "a later user choice is preserved");
  });
}
test("existing fields, account inputs and unidentified rename dialogs are left alone", () => {
  const page = namingPage();
  page.dialog("Name your passkey");
  page.startNaming();
  page.dialog("Sign in", [new page.Input({ name: "username", autocomplete: "username" })]);
  page.dialog("Name your passkey", [new page.Input({ type: "email" })]);
  page.dialog("Name your passkey", [new page.Input({ name: "username", autocomplete: "username" })]);
  page.dialog("Rename passkey");
  page.changed();
  assert.equal(page.writes.length, 0);
});
test("an explicitly identified rename dialog must belong to the newly created credential", () => {
  const page = namingPage();
  page.startNaming();
  const [old] = page.dialog("Rename passkey", undefined, { attrs: { "data-credential-id": "other-credential" } });
  const [fresh] = page.dialog("Rename passkey", undefined, { attrs: { "data-credential-id": "new-credential" } });
  page.changed();
  assert.equal(old.value, "random-device-123");
  assert.deepEqual(page.writes, [[fresh, "MYLE"]]);
});
test("generic naming titles require the new credential's identity", () => {
  for (const title of ["Passkey name", "Security key nickname", "Name your passkey", "Choose a new name for your passkey", "Enter a new security key nickname"]) {
    const unidentified = namingPage();
    unidentified.startNaming();
    unidentified.dialog(title);
    unidentified.changed();
    assert.equal(unidentified.writes.length, 0, title);
  }
  const page = namingPage();
  page.startNaming();
  const [fresh] = page.dialog("Passkey name", undefined, { attrs: { "data-credential-id": "new-credential" } });
  page.changed();
  assert.deepEqual(page.writes, [[fresh, "MYLE"]]);
});
test("trusted edits retargeted through a shadow host are preserved", () => {
  const page = namingPage();
  page.startNaming();
  const [field] = page.dialog("Name your new passkey");
  field.value = "My custom name";
  page.listeners.get("input")({ isTrusted: true, target: {}, composedPath: () => [field, {}] });
  page.changed();
  assert.equal(page.writes.length, 0);
  assert.equal(field.value, "My custom name");
});
test("shadow roots are observed locally and their listeners are removed", () => {
  const page = namingPage();
  const rootListeners = new Map();
  const root = {
    addEventListener: (kind, fn) => rootListeners.set(kind, fn),
    removeEventListener: (kind) => rootListeners.delete(kind),
  };
  page.startNaming();
  page.roots.push(root);
  page.changed();
  assert.ok(page.observer.observed.has(root));
  const [field] = page.dialog("Name your new passkey");
  rootListeners.get("input")({ isTrusted: true, target: field });
  page.changed();
  assert.equal(page.writes.length, 0);
  page.listeners.get("pagehide")();
  assert.equal(rootListeners.size, 0);
});
test("hidden, ambiguous and user-edited inputs are never overwritten", () => {
  const page = namingPage();
  page.startNaming();
  page.dialog("Name your new passkey", [new page.Input({ shown: false })]);
  page.dialog("Name your new passkey", [new page.Input(), new page.Input()]);
  const [edited] = page.dialog("Name your new passkey");
  page.listeners.get("input")({ isTrusted: true, target: edited });
  page.changed();
  assert.equal(page.writes.length, 0);
});
test("expiration, navigation and a replacement creation disconnect the helper", () => {
  const page = namingPage();
  page.startNaming();
  const oldObserver = page.observer;
  page.startNaming();
  assert.equal(oldObserver.connected, false);
  assert.equal(page.timers.size, 1);
  page.listeners.get("pagehide")();
  assert.equal(page.observer.connected, false);
  assert.equal(page.timers.size, 0);
  page.startNaming();
  [...page.timers.values()][0].fn();
  assert.equal(page.observer.connected, false);
  assert.equal(page.listeners.has("input"), false);
});
test("a page inspection failure cannot prevent passkey creation", () => {
  const page = namingPage();
  page.scope.deepAll = () => { throw new Error("page went away"); };
  assert.equal(page.capture(), null);
  assert.doesNotThrow(() => page.startNaming(null));
  assert.equal(page.timers.size, 0);
});
test("only a successful MYLE creation starts naming; the credential response is unchanged", async () => {
  const start = source.indexOf("  async function offerNewPasskey(");
  const end = source.indexOf("\n  }\n", start) + 4;
  const named = [];
  const answers = [];
  const before = new Set();
  let buttons;
  let result = { ok: true, credential: { id: "fresh", rawId: "random-cryptographic-id" } };
  const scope = vm.createContext({
    prompted: "request", busyLine: "", passkeysHere: async () => ({ rpId: "google.com" }),
    stopPasskeyNaming() {}, capturePasskeyNaming: () => before,
    startPasskeyNaming: (...args) => named.push(args),
    prompt: (_id, _title, _line, values) => { buttons = values; },
    cancel() {}, anotherDevice() {}, closePrompt() {}, failed() {},
    passkeyCall: async () => result,
    answerPage: (_id, answer) => answers.push(answer),
  });
  const offer = vm.runInContext(`(${source.slice(start, end).trim()})`, scope);
  await offer("request", {});
  await buttons[2][2]({}, {});
  assert.deepEqual(named, [["fresh", before]]);
  assert.equal(answers[0].credential, result.credential);
  result = { ok: false, error: "notSupported" };
  await offer("request", {});
  await buttons[2][2]({}, {});
  assert.equal(named.length, 1, "native fallback cannot rename a credential");
  scope.prompted = null;
  result = { ok: true, credential: { id: "cancelled" } };
  await offer("request", {});
  await buttons[2][2]({}, {});
  assert.equal(named.length, 1, "a cancelled prompt cannot arm naming");
});
