import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(new URL("../../extension/passkeys.js", import.meta.url), "utf8");

// Run the real routing code in separate page/frame realms. The browser boundary
// returns its exact request so fallback context, options and signal are observable.
function page({ top, ancestor, policy, load = true } = {}) {
  const posted = [];
  const window = new EventTarget();
  class CredentialsContainer {
    create(options) { return this.native("create", options); }
    get(options) { return this.native("get", options); }
    native(kind, options) {
      if (options?.signal?.aborted) return Promise.reject(options.signal.reason);
      if (options?.mediation === "conditional") {
        window.nativeOptions = options;
        return new Promise((resolve, reject) => {
          window.nativeResolve = resolve;
          options.signal?.addEventListener("abort", () => reject(options.signal.reason));
        });
      }
      return Promise.resolve({ native: kind, window, options });
    }
  }
  class HTMLIFrameElement {
    constructor(win) { this.win = win; }
    get contentWindow() { return this.win; }
  }
  window.top = top?.window ?? window;
  window.parent = ancestor ?? window.top;
  window.origin = "https://example.com";
  window.document = { permissionsPolicy: policy };
  window.postMessage = (message) => { if (message.kind !== "trace") posted.push(message); };
  const context = vm.createContext({
    window, document: window.document, self: window, EventTarget,
    location: { origin: window.origin, href: `${window.origin}/` },
    navigator: { credentials: new CredentialsContainer() }, CredentialsContainer, HTMLIFrameElement,
    PublicKeyCredential: class {}, AbortController, DOMException, btoa, atob,
  });
  window.navigator = context.navigator;
  window.CredentialsContainer = CredentialsContainer;
  if (load) vm.runInContext(source, context);
  return { window, context, posted };
}

const createOptions = () => ({ publicKey: {
  challenge: new Uint8Array([1]), rp: { id: "example.com", name: "Example" },
  user: { id: new Uint8Array([2]), name: "user", displayName: "User" },
  pubKeyCredParams: [{ type: "public-key", alg: -7 }],
} });

test("a cross-platform creation stays with the original browser and signal", async () => {
  const { context, posted, window } = page();
  const options = createOptions();
  options.publicKey.authenticatorSelection = { authenticatorAttachment: "cross-platform" };
  options.signal = new AbortController().signal;
  const pending = context.navigator.credentials.create(options);
  assert.equal(posted.length, 0, "MYLE must not offer its platform authenticator");
  const result = await pending;
  assert.equal(result.native, "create");
  assert.equal(result.window, window);
  assert.equal(result.options, options);
  assert.equal(result.options.signal, options.signal);
});

for (const [kind, extensions] of [
  ["create", { prf: {} }], ["get", { prf: { eval: { first: new Uint8Array([1]) } } }],
  ["create", { largeBlob: { support: "required" } }], ["get", { largeBlob: { read: true } }],
  ["create", { uvm: true }], ["get", { uvm: true }],
  ["create", { credentialProtectionPolicy: "userVerificationRequired", enforceCredentialProtectionPolicy: true }],
  ["create", { appidExclude: "https://example.com/keys" }], ["get", { futureExtension: {} }],
]) {
  test(`${kind} with unsupported ${Object.keys(extensions).join("/")} stays native`, async () => {
    const { context, posted, window } = page();
    const options = createOptions();
    options.publicKey.extensions = extensions;
    const pending = context.navigator.credentials[kind](options);
    assert.equal(posted.length, 0);
    const result = await pending;
    assert.equal(result.native, kind);
    assert.equal(result.window, window);
    assert.equal(result.options, options);
  });
}

test("unsupported extensions in conditional get start only the browser request", async () => {
  const { context, posted, window } = page();
  const options = { ...createOptions(), mediation: "conditional", signal: new AbortController().signal };
  options.publicKey.extensions = { prf: {} };
  const pending = context.navigator.credentials.get(options);
  assert.equal(posted.length, 0);
  assert.equal(window.nativeOptions, options);
  window.nativeResolve({ native: "conditional" });
  assert.equal((await pending).native, "conditional");
});

for (const route of ["delegated", "adopted"]) {
  for (const [label, setup] of [
    ["denied policy", { policy: { allowsFeature: () => false } }],
    ["unknown policy", { policy: null }],
    ["throwing policy", { policy: { allowsFeature: () => { throw new Error("unavailable"); } } }],
    ["cross-origin ancestor", { policy: { allowsFeature: () => true }, ancestor: { origin: "https://other.example" } }],
    ["inaccessible ancestor", { policy: { allowsFeature: () => true }, ancestor: Object.defineProperty({}, "origin", { get() { throw new Error("SecurityError"); } }) }],
  ]) {
    for (const kind of ["get", "create"]) {
      test(`a ${route} frame with ${label} keeps ${kind} in its original context`, async () => {
        const top = page();
        const frame = page({ ...setup, top, load: route !== "adopted" });
        if (route === "adopted") void new top.context.HTMLIFrameElement(frame.window).contentWindow;
        const options = createOptions();
        const pending = frame.context.navigator.credentials[kind](options);
        assert.equal(top.posted.length, 0);
        const result = await pending;
        assert.equal(result.window, frame.window);
        assert.equal(result.options, options);
      });
    }
  }
}

test("a denied top-level policy leaves validation to the original browser", async () => {
  const { context, posted } = page({ policy: { allowsFeature: () => false } });
  for (const kind of ["get", "create"]) {
    const pending = context.navigator.credentials[kind](createOptions());
    assert.equal(posted.length, 0);
    assert.equal((await pending).native, kind);
  }
});

test("the cross-platform attachment constraint follows Web IDL string conversion", async () => {
  const { context, posted } = page();
  const options = createOptions();
  options.publicKey.authenticatorSelection = { authenticatorAttachment: new String("cross-platform") };
  const pending = context.navigator.credentials.create(options);
  assert.equal(posted.length, 0);
  assert.equal((await pending).native, "create");
});

for (const kind of ["get", "create"]) {
  test(`native ${kind} fallback preserves an already-aborted signal and reason`, async () => {
    const { context, posted } = page();
    const controller = new AbortController();
    const reason = new DOMException("Cancelled by the page", "AbortError");
    controller.abort(reason);
    const options = createOptions();
    options.signal = controller.signal;
    options.publicKey.extensions = { prf: {} };
    await assert.rejects(context.navigator.credentials[kind](options), (error) => error === reason);
    assert.equal(posted.length, 0);
  });
}

test("native conditional fallback preserves later cancellation", async () => {
  const { context, posted, window } = page();
  const controller = new AbortController();
  const options = { ...createOptions(), mediation: "conditional", signal: controller.signal };
  options.publicKey.extensions = { largeBlob: { read: true } };
  const pending = context.navigator.credentials.get(options);
  const reason = new DOMException("Cancelled by the page", "AbortError");
  assert.equal(window.nativeOptions.signal, controller.signal);
  controller.abort(reason);
  await assert.rejects(pending, (error) => error === reason);
  assert.equal(posted.length, 0);
});

function answer(page, reply) {
  const message = page.posted.findLast((message) => message.kind !== "abort");
  const event = new Event("message");
  Object.assign(event, { source: page.window, data: { source: "myle-passkeys/extension", id: message.id, ...reply } });
  page.window.dispatchEvent(event);
}

test("an allowed nested same-origin frame can delegate and rechecks policy at call time", async () => {
  let allowGet = false;
  const top = page();
  const ancestor = page({ top });
  const frame = page({ top, ancestor: ancestor.window, policy: {
    allowsFeature: (feature) => feature === "publickey-credentials-create" || allowGet,
  } });
  const options = createOptions();
  assert.equal((await frame.context.navigator.credentials.get(options)).window, frame.window);
  const creating = frame.context.navigator.credentials.create(options);
  assert.equal(top.posted.at(-1).kind, "create");
  answer(top, { result: "native" });
  await creating;
  allowGet = true;
  const signingIn = frame.context.navigator.credentials.get(options);
  assert.equal(top.posted.at(-1).kind, "get");
  answer(top, { result: "native" });
  await signingIn;
  allowGet = false;
  assert.equal((await frame.context.navigator.credentials.get(options)).window, frame.window);
});

test("an adopted same-origin frame supports the legacy featurePolicy API", async () => {
  const top = page();
  const frame = page({ top, load: false });
  frame.window.document.featurePolicy = { allowsFeature: (feature) => feature === "publickey-credentials-get" };
  void new top.context.HTMLIFrameElement(frame.window).contentWindow;
  const signingIn = frame.context.navigator.credentials.get(createOptions());
  assert.equal(top.posted.at(-1).kind, "get");
  answer(top, { result: "native" });
  await signingIn;
  assert.equal((await frame.context.navigator.credentials.create(createOptions())).window, frame.window);
});

for (const route of ["delegated", "adopted"]) {
  test(`a ${route} allowed frame keeps unsupported requirements with its original browser`, async () => {
    const top = page();
    const frame = page({ top, load: route !== "adopted", policy: { allowsFeature: () => true } });
    if (route === "adopted") void new top.context.HTMLIFrameElement(frame.window).contentWindow;
    const options = createOptions();
    options.publicKey.authenticatorSelection = { authenticatorAttachment: "cross-platform" };
    assert.equal((await frame.context.navigator.credentials.create(options)).window, frame.window);
    options.publicKey.extensions = { prf: {} };
    assert.equal((await frame.context.navigator.credentials.get(options)).window, frame.window);
    assert.equal(top.posted.length, 0);
  });
}

test("non-enumerable and inherited unsupported dictionary inputs stay native", async () => {
  for (const extensions of [
    Object.defineProperty({}, "uvm", { value: true }),
    Object.create(Object.defineProperty({}, "prf", { value: {} })),
  ]) {
    const { context, posted } = page();
    const options = createOptions();
    options.publicKey.extensions = extensions;
    assert.equal((await context.navigator.credentials.get(options)).native, "get");
    assert.equal(posted.length, 0);
  }
});

test("supported conditional get cancels the MYLE question when the browser wins", async () => {
  const top = page();
  const pending = top.context.navigator.credentials.get({ ...createOptions(), mediation: "conditional" });
  assert.equal(top.posted[0].kind, "conditional");
  const browserSignal = top.window.nativeOptions.signal;
  top.window.nativeResolve({ native: "conditional" });
  assert.equal((await pending).native, "conditional");
  assert.equal(browserSignal.aborted, true);
  assert.equal(top.posted.at(-1).kind, "abort");
  assert.equal(top.posted.at(-1).id, top.posted[0].id);
  // An answer arriving after cancellation cannot resurrect the MYLE request.
  answer(top, { result: "error", name: "NotAllowedError" });
  assert.equal(top.posted.filter((message) => message.kind === "abort").length, 1);
});

test("supported conditional get carries outer cancellation to both racers", async () => {
  const top = page();
  const controller = new AbortController();
  const pending = top.context.navigator.credentials.get({ ...createOptions(), mediation: "conditional", signal: controller.signal });
  const browserSignal = top.window.nativeOptions.signal;
  const reason = new DOMException("Cancelled by the page", "AbortError");
  controller.abort(reason);
  await assert.rejects(pending, (error) => error === reason);
  assert.equal(browserSignal.aborted, true);
  assert.equal(browserSignal.reason, reason);
  assert.equal(top.posted.at(-1).kind, "abort");
});

test("explicit platform attachment remains available through MYLE", async () => {
  const top = page();
  const options = createOptions();
  options.publicKey.authenticatorSelection = { authenticatorAttachment: "platform" };
  const pending = top.context.navigator.credentials.create(options);
  assert.equal(top.posted[0].kind, "create");
  answer(top, { result: "native" });
  assert.equal((await pending).native, "create");
});
