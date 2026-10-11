// The extension's page-world part for passkeys (passkeys.js), run in a
// stand-in for the page: `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(new URL("../../extension/passkeys.js", import.meta.url), "utf8");
const EXTENSION = "myle-passkeys/extension";

const b64 = (bytes) => Buffer.from(bytes).toString("base64url");

/** A page with the browser's own passkeys, and MYLE's part listening.
 *  `timers` (optional) stands in for the page's setTimeout/clearTimeout. */
function page(timers) {
  const posted = [];
  const window = new EventTarget();
  class CredentialsContainer {
    create(options) {
      return Promise.resolve({ native: "create", options });
    }
    get(options) {
      if (options?.mediation === "conditional") {
        return new Promise((resolve, reject) => {
          window.nativeConditional = resolve;
          options.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        });
      }
      return Promise.resolve({ native: "get", options });
    }
  }
  class PublicKeyCredential {
    static isUserVerifyingPlatformAuthenticatorAvailable() {
      return Promise.resolve(false);
    }
  }
  class AuthenticatorAttestationResponse {}
  class AuthenticatorAssertionResponse {}
  const context = vm.createContext({
    window,
    navigator: { credentials: new CredentialsContainer() },
    location: { origin: "https://example.com" },
    CredentialsContainer,
    PublicKeyCredential,
    AuthenticatorAttestationResponse,
    AuthenticatorAssertionResponse,
    EventTarget,
    AbortController,
    DOMException,
    btoa,
    atob,
    ...(timers ? { setTimeout: timers.set, clearTimeout: timers.clear } : {}),
  });
  window.top = window;
  const traces = [];
  // MYLE's debug notes go to the page as well; the questions are counted apart.
  window.postMessage = (data) => (data?.kind === "trace" ? traces.push(data) : posted.push(data));
  vm.runInContext(source, context, { filename: "passkeys.js" });
  /** MYLE's part answering the last question. */
  const answer = (reply, index = posted.length - 1) => {
    const event = new Event("message");
    Object.assign(event, { source: window, data: { source: EXTENSION, id: posted[index].id, ...reply } });
    window.dispatchEvent(event);
  };
  return { context, posted, answer, window, traces };
}

const made = {
  id: b64([1, 2, 3]),
  rawId: b64([1, 2, 3]),
  authenticatorAttachment: "platform",
  response: {
    clientDataJSON: b64(Buffer.from('{"type":"webauthn.create"}')),
    attestationObject: b64([0xa3]),
    authenticatorData: b64([9, 9]),
    publicKey: b64([4, 5, 6]),
    publicKeyAlgorithm: -7,
    transports: ["hybrid", "internal"],
  },
};

const createOptions = () => ({
  publicKey: {
    rp: { id: "example.com", name: "Example" },
    user: { id: new Uint8Array([7, 7]), name: "me@example.com", displayName: "Me" },
    challenge: new Uint8Array([1, 2, 3, 4]).buffer,
    pubKeyCredParams: [{ type: "public-key", alg: -7 }, { type: "public-key", alg: -257 }],
    excludeCredentials: [{ type: "public-key", id: new Uint8Array([5]) }],
    authenticatorSelection: { userVerification: "required" },
    extensions: { credProps: true },
  },
});

test("a new passkey is asked of MYLE and comes back as a PublicKeyCredential", async () => {
  const { context, posted, answer } = page();
  const pending = context.navigator.credentials.create(createOptions());
  assert.equal(posted.length, 1);
  assert.equal(posted[0].kind, "create");
  assert.deepEqual({ ...posted[0].options, algorithms: [...posted[0].options.algorithms], exclude: [...posted[0].options.exclude] }, {
    rpId: "example.com",
    rpName: "Example",
    userId: b64([7, 7]),
    userName: "me@example.com",
    userDisplayName: "Me",
    challenge: b64([1, 2, 3, 4]),
    algorithms: [-7, -257],
    exclude: [b64([5])],
    userVerification: "required",
    credProps: true,
  });
  answer({ result: "credential", credential: made });
  const credential = await pending;
  assert.ok(credential instanceof context.PublicKeyCredential);
  assert.ok(credential.response instanceof context.AuthenticatorAttestationResponse);
  assert.equal(credential.id, made.id);
  assert.equal(credential.type, "public-key");
  assert.deepEqual([...new Uint8Array(credential.rawId)], [1, 2, 3]);
  assert.deepEqual([...new Uint8Array(credential.response.getPublicKey())], [4, 5, 6]);
  assert.deepEqual([...new Uint8Array(credential.response.getAuthenticatorData())], [9, 9]);
  assert.equal(credential.response.getPublicKeyAlgorithm(), -7);
  assert.deepEqual([...credential.response.getTransports()], ["hybrid", "internal"]);
  assert.deepEqual(JSON.parse(JSON.stringify(credential.getClientExtensionResults())), { credProps: { rk: true } });
  assert.equal(JSON.parse(JSON.stringify(credential)).response.attestationObject, made.response.attestationObject);
});

test("signing in returns an assertion, and the user handle as bytes", async () => {
  const { context, posted, answer } = page();
  const pending = context.navigator.credentials.get({
    publicKey: { challenge: new Uint8Array([3, 3]), rpId: "example.com", allowCredentials: [{ type: "public-key", id: new Uint8Array([1, 2, 3]) }] },
  });
  assert.equal(posted[0].kind, "get");
  assert.deepEqual([...posted[0].options.allow], [b64([1, 2, 3])]);
  assert.equal(posted[0].options.userVerification, "preferred");
  answer({
    result: "credential",
    credential: { ...made, response: { clientDataJSON: made.response.clientDataJSON, authenticatorData: b64([1]), signature: b64([2]), userHandle: b64([7, 7]) } },
  });
  const credential = await pending;
  assert.ok(credential.response instanceof context.AuthenticatorAssertionResponse);
  assert.deepEqual([...new Uint8Array(credential.response.signature)], [2]);
  assert.deepEqual([...new Uint8Array(credential.response.userHandle)], [7, 7]);
  assert.deepEqual({ ...credential.getClientExtensionResults() }, {});
});

test("another device, or nothing in MYLE, leaves it to the browser", async () => {
  const { context, answer } = page();
  const pending = context.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]) } });
  answer({ result: "native" });
  assert.equal((await pending).native, "get");
  // Calls without publicKey (passwords, federated) never come to MYLE.
  assert.equal((await context.navigator.credentials.get({ password: true })).native, "get");
});

test("a refusal reaches the page as the DOMException WebAuthn names", async () => {
  const { context, answer } = page();
  const pending = context.navigator.credentials.create(createOptions());
  answer({ result: "error", name: "InvalidStateError", message: "Already there." });
  await assert.rejects(pending, (error) => error.name === "InvalidStateError" && error.message === "Already there.");
});

test("the page can withdraw its question", async () => {
  const { context, posted } = page();
  const controller = new AbortController();
  const pending = context.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]) }, signal: controller.signal });
  controller.abort();
  await assert.rejects(pending, (error) => error.name === "AbortError");
  assert.equal(posted.at(-1).kind, "abort");
  assert.equal(posted.at(-1).id, posted[0].id);
});

test("a sign-in form waits for MYLE's menu and the browser's own at once", async () => {
  // MYLE's passkey picked first: the browser's own request is withdrawn.
  let { context, posted, answer, window } = page();
  let pending = context.navigator.credentials.get({ mediation: "conditional", publicKey: { challenge: new Uint8Array([1]) } });
  assert.equal(posted[0].kind, "conditional");
  answer({ result: "credential", credential: { ...made, response: { clientDataJSON: made.response.clientDataJSON, authenticatorData: b64([1]), signature: b64([2]) } } }, 0);
  assert.equal((await pending).id, made.id);
  assert.equal(posted.length, 1, "MYLE's part answered, so it knows the form is done");

  // The browser's own picked first.
  ({ context, posted, answer, window } = page());
  pending = context.navigator.credentials.get({ mediation: "conditional", publicKey: { challenge: new Uint8Array([1]) } });
  await new Promise((resolve) => setTimeout(resolve, 0));
  window.nativeConditional({ native: "conditional" });
  assert.equal((await pending).native, "conditional");
  assert.equal(posted.at(-1).kind, "abort");
});

test("a site asking for its old U2F keys (appid) hears that a MYLE passkey is not one", async () => {
  const { context, posted, answer } = page();
  const pending = context.navigator.credentials.get({
    publicKey: { challenge: new Uint8Array([3]), rpId: "google.com", extensions: { appid: "https://www.gstatic.com/securitykey/origins.json" } },
  });
  assert.equal(posted[0].options.appid, true);
  answer({
    result: "credential",
    credential: { ...made, response: { clientDataJSON: made.response.clientDataJSON, authenticatorData: b64([1]), signature: b64([2]) } },
  });
  const credential = await pending;
  assert.deepEqual({ ...credential.getClientExtensionResults() }, { appid: false });
  assert.deepEqual(JSON.parse(JSON.stringify(credential)).clientExtensionResults, { appid: false });
});

test("an empty helper frame of the page's own origin asks MYLE through the page", async () => {
  const parent = page();
  const frame = (origin) => {
    // Each frame has its own realm, and so its own prototypes.
    class CredentialsContainer {
      get(options) {
        return Promise.resolve({ native: "frame", options });
      }
      create(options) {
        return Promise.resolve({ native: "frame", options });
      }
    }
    const window = new EventTarget();
    window.top = { origin: "https://example.com", navigator: parent.context.navigator };
    window.postMessage = () => {};
    const context = vm.createContext({
      window,
      EventTarget,
      location: { origin, href: `${origin}/` },
      self: { origin },
      navigator: { credentials: new CredentialsContainer() },
      CredentialsContainer,
      PublicKeyCredential: class {},
      Object,
    });
    vm.runInContext(source, context, { filename: "passkeys.js" });
    return context;
  };
  const same = frame("https://example.com");
  const pending = same.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]), rpId: "example.com" } });
  assert.equal(parent.posted.at(-1).kind, "get", "the page's MYLE part was asked");
  parent.answer({ result: "native" });
  assert.equal((await pending).native, "get", "and the page's answer came back");
  // Passwords and the like stay the frame's own.
  assert.equal((await same.navigator.credentials.get({ password: true })).native, "frame");

  // Another origin: the browser's own, untouched.
  const other = frame("https://evil.example");
  assert.equal((await other.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]) } })).native, "frame");
});

test("MYLE's wrappers read as the browser's own functions", () => {
  const { context } = page();
  const read = vm.runInContext("Function.prototype.toString.call(navigator.credentials.get)", context);
  assert.match(read, /^get\(options\)/, "the stand-in's own source, as the browser's would be native code");
  assert.equal(vm.runInContext("navigator.credentials.get === CredentialsContainer.prototype.get", context), true);
  assert.equal(vm.runInContext("String(Function.prototype.toString).includes('__myle')", context), false);
});


test("a sign-in asked before MYLE's part of the page started is asked again on its hello", async () => {
  const { context, posted, answer, window } = page();
  const pending = context.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]) } });
  assert.equal(posted.length, 1);
  const hello = new Event("message");
  Object.assign(hello, { source: window, data: { source: EXTENSION, kind: "hello" } });
  window.dispatchEvent(hello);
  assert.equal(posted.length, 2, "asked again");
  assert.equal(posted[1].id, posted[0].id, "as the same question, answered once");
  answer({ result: "native" });
  assert.equal((await pending).native, "get");
  // Once bridged, a question is asked just once.
  void context.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([2]) } });
  window.dispatchEvent(Object.assign(new Event("message"), { source: window, data: { source: EXTENSION, kind: "hello" } }));
  assert.equal(posted.length, 3);
});

test("a site asking for a platform authenticator hears MYLE is one", async () => {
  const { context } = page();
  assert.equal(await context.PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable(), true);
});

/** Timers that run only when the test says so. */
function fakeTimers() {
  const pending = new Map();
  let next = 0;
  return {
    pending,
    set: (run, ms) => { pending.set(++next, { run, ms }); return next; },
    clear: (id) => pending.delete(id),
    /** Runs every timer of exactly `ms`. */
    fire(ms) {
      for (const [id, timer] of [...pending]) {
        if (timer.ms !== ms) continue;
        pending.delete(id);
        timer.run();
      }
    },
  };
}

const hello = (window, acks) => {
  const event = new Event("message");
  Object.assign(event, { source: window, data: { source: EXTENSION, kind: "hello", ...(acks ? { acks: true } : {}) } });
  window.dispatchEvent(event);
};
const ack = (window, id) => {
  const event = new Event("message");
  Object.assign(event, { source: window, data: { source: EXTENSION, kind: "ack", id } });
  window.dispatchEvent(event);
};
const signIn = { publicKey: { challenge: new Uint8Array([1]), rpId: "example.com", timeout: 60_000 } };

test("a sign-in MYLE never answers ends at the page's own timeout, as the browser's would", async () => {
  const timers = fakeTimers();
  const { context, posted, window } = page(timers);
  hello(window, true);
  const pending = context.navigator.credentials.get(signIn);
  ack(window, posted[0].id);
  timers.fire(60_000);
  await assert.rejects(pending, (error) => error.name === "NotAllowedError");
  assert.equal(posted.at(-1).kind, "abort", "MYLE's prompt is closed too");
  assert.equal(timers.pending.size, 0);
});

test("the page's timeout is kept within sane bounds", async () => {
  const timers = fakeTimers();
  const { context, window } = page(timers);
  hello(window, false);
  void context.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]), timeout: 5 } }).catch(() => {});
  void context.navigator.credentials.get({ publicKey: { challenge: new Uint8Array([1]) } }).catch(() => {});
  const delays = [...timers.pending.values()].map((timer) => timer.ms).sort((a, b) => a - b);
  assert.deepEqual(delays, [30_000, 300_000]);
});

test("a question MYLE's part of the page never hears goes to the browser's own passkeys", async () => {
  const timers = fakeTimers();
  const { context, posted, window } = page(timers);
  hello(window, true);
  const pending = context.navigator.credentials.get(signIn);
  assert.equal(posted.length, 1);
  timers.fire(2000);
  const answer = await pending;
  assert.equal(answer.native, "get", "the browser's own get was called");
});

test("an older MYLE part of the page (no acks) is waited for as before", async () => {
  const timers = fakeTimers();
  const { context, posted, window, answer } = page(timers);
  hello(window, false);
  const pending = context.navigator.credentials.get(signIn);
  timers.fire(2000);
  answer({ result: "native" });
  assert.equal((await pending).native, "get");
  assert.equal(posted.filter((message) => message.kind === "abort").length, 0);
});

test("a question asked before MYLE's part of the page starts goes native if it never starts", async () => {
  const timers = fakeTimers();
  const { context } = page(timers);
  const pending = context.navigator.credentials.get(signIn);
  timers.fire(10_000);
  assert.equal((await pending).native, "get");
});

test("a page put in the back/forward cache refuses its modal sign-in but keeps a waiting form", async () => {
  const timers = fakeTimers();
  const { context, posted, window } = page(timers);
  hello(window, true);
  const modal = context.navigator.credentials.get(signIn);
  const conditional = context.navigator.credentials.get({ ...signIn, mediation: "conditional" });
  for (const message of posted) if (message.kind !== "abort") ack(window, message.id);
  const leaving = new Event("pagehide");
  leaving.persisted = true;
  window.dispatchEvent(leaving);
  await assert.rejects(modal, (error) => error.name === "NotAllowedError");
  const state = await Promise.race([conditional.then(() => "settled", () => "settled"), new Promise((resolve) => setImmediate(() => resolve("waiting")))]);
  assert.equal(state, "waiting");
});

test("a conditional create is left to the browser", async () => {
  const { context, posted } = page();
  const made = await context.navigator.credentials.create({ ...createOptions(), mediation: "conditional" });
  assert.equal(made.native, "create");
  assert.equal(posted.length, 0);
});
