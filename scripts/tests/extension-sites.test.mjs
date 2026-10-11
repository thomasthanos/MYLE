// The extension's idea of one site (background.js), run in a stand-in for the
// browser: `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const extension = new URL("../../extension/", import.meta.url);

async function background() {
  const listener = { addListener() {} };
  const context = vm.createContext({
    chrome: {
      runtime: { getURL: () => "chrome-extension://test/", onMessage: listener, id: "test" },
      tabs: { onRemoved: listener },
    },
    crypto: globalThis.crypto,
    URL,
    setTimeout,
    clearTimeout,
  });
  context.globalThis = context;
  for (const file of ["psl.js", "background.js"]) {
    vm.runInContext(await readFile(new URL(file, extension), "utf8"), context, { filename: file });
  }
  return context;
}

test("hosts of one owner are one site", async () => {
  const { sameSite } = await background();
  assert.equal(sameSite("https://login.example.com/a", "https://example.com/b"), true);
  assert.equal(sameSite("https://accounts.example.com/", "https://www.example.com/"), true);
  assert.equal(sameSite("https://a.b.example.co.uk/", "https://example.co.uk/"), true);
  assert.equal(sameSite("http://localhost:3000/", "http://localhost:5173/"), true);
});

test("names on a shared suffix are not one site", async () => {
  const { sameSite } = await background();
  assert.equal(sameSite("https://attacker.github.io/", "https://github.io/"), false);
  assert.equal(sameSite("https://attacker.github.io/", "https://victim.github.io/"), false);
  assert.equal(sameSite("https://evil.blogspot.com/", "https://blogspot.com/"), false);
  assert.equal(sameSite("https://example.co.uk/", "https://co.uk/"), false);
  assert.equal(sameSite("https://a.example.com/", "https://example.org/"), false);
  assert.equal(sameSite("https://127.0.0.1/", "https://0.0.1/"), false);
});

test("wildcard and exception rules are followed", async () => {
  const { siteOf } = await background();
  assert.equal(siteOf("foo.bar.ck"), "foo.bar.ck");
  assert.equal(siteOf("a.www.ck"), "www.ck");
  assert.equal(siteOf("shop.city.kawasaki.jp"), "city.kawasaki.jp");
  assert.equal(siteOf("xn--80ak6aa92e.xn--p1ai"), "xn--80ak6aa92e.xn--p1ai");
});

test("why the browser could not reach MYLE is told apart", async () => {
  const { hostProblem } = await background();
  // Chromium's messages, then Firefox's.
  assert.equal(hostProblem("Specified native messaging host not found."), "hostMissing");
  assert.equal(hostProblem("Access to the specified native messaging host is forbidden."), "hostForbidden");
  assert.equal(hostProblem("Native host has exited."), "hostExited");
  assert.equal(hostProblem("Error when communicating with the native messaging host."), "hostExited");
  assert.equal(hostProblem("No such native application com.thomasthanos.myle"), "hostMissing");
  assert.equal(hostProblem("An unexpected error occurred"), "hostExited");
  assert.equal(hostProblem("Something new"), "noHost");
});

/** background.js with a tab of two frames, recording what reaches MYLE. Our
 *  script in each frame answers the popup's census with a message of its own,
 *  as content.js does; `frames` says what each one sees. */
async function backgroundWithTab(reply = { ok: true }, scheduling = {}, frames = defaultFrames(), storage = undefined) {
  const asked = [];
  const applied = [];
  const ports = [];
  const listener = { addListener() {} };
  const tab = { id: 7, active: true, url: "https://example.com/login" };
  let context;
  context = vm.createContext({
    chrome: {
      runtime: {
        getURL: () => "chrome-extension://test/",
        onMessage: listener,
        onConnect: { addListener: (connect) => ports.push(connect) },
        id: "test",
        sendNativeMessage: async (_host, message) => {
          asked.push(JSON.stringify(message));
          return typeof reply === "function" ? reply(message) : reply;
        },
      },
      storage,
      tabs: {
        onRemoved: listener,
        get: async () => tab,
        sendMessage: async (tabId, message, options) => {
          if (message.type === "census") {
            for (const frame of frames) {
              await context.handle({ type: "present", round: message.round, ...frame.sees }, {
                id: "test", url: frame.url, frameId: frame.frameId, documentId: frame.documentId,
                documentLifecycle: "active", tab: { ...tab, id: tabId },
              });
            }
            return undefined;
          }
          if (message.type === "apply") {
            applied.push({ message, options });
            return { applied: true };
          }
          throw new Error("no receiver");
        },
      },
    },
    crypto: globalThis.crypto,
    URL,
    TextEncoder,
    setTimeout,
    clearTimeout,
    ...scheduling,
  });
  context.globalThis = context;
  for (const file of ["psl.js", "background.js"]) {
    vm.runInContext(await readFile(new URL(file, extension), "utf8"), context, { filename: file });
  }
  /** A passkey port from `sender`, as content.js opens one. */
  const connect = (sender) => {
    const port = {
      name: "myle-passkey", sender, posted: [], closed: false,
      messageListeners: [], disconnectListeners: new Set(),
      onMessage: { addListener: (fn) => port.messageListeners.push(fn) },
      onDisconnect: { addListener: (fn) => port.disconnectListeners.add(fn), removeListener: (fn) => port.disconnectListeners.delete(fn) },
      postMessage: (answer) => port.posted.push(answer),
      disconnect: () => { port.closed = true; },
      send: (message) => port.messageListeners.forEach((fn) => fn(message)),
      // The page went away: the browser closes the port.
      gone: () => [...port.disconnectListeners].forEach((fn) => fn()),
    };
    ports.forEach((onConnect) => onConnect(port));
    return port;
  };
  return { context, asked, applied, connect, tab };
}

function defaultFrames() {
  return [
    { frameId: 0, url: "https://example.com/login", documentId: "top-doc", sees: { fields: true, password: true } },
    { frameId: 3, url: "https://auth.other.com/frame", documentId: "frame-doc", sees: { fields: true, password: true } },
  ];
}

const popup = { url: "chrome-extension://test/popup.html", id: "test" };

test("the popup's Copy asks MYLE with the frame's own address, and gets no text back", async () => {
  const { context, asked } = await backgroundWithTab();
  const answer = await context.handle({ type: "copyTab", tabId: 7, frameId: 3, id: "abc", field: "password" }, popup);
  assert.equal(JSON.stringify(answer), JSON.stringify({ ok: true }));
  assert.deepEqual(asked, [JSON.stringify({ type: "copy", id: "abc", url: "https://auth.other.com/frame", field: "password", extVersion: "" })]);
});

test("Copy refuses other fields, a frame that is gone, and pages asking for it", async () => {
  const { context, asked } = await backgroundWithTab();
  const bad = await context.handle({ type: "copyTab", tabId: 7, frameId: 0, id: "abc", field: "notes" }, popup);
  assert.equal(bad.error, "badRequest");
  const gone = await context.handle({ type: "copyTab", tabId: 7, frameId: 9, id: "abc", field: "username" }, popup);
  assert.equal(gone.error, "pageChanged");
  const page = { url: "https://example.com/login", id: "test", tab: { id: 7, active: true }, frameId: 0 };
  const fromPage = await context.handle({ type: "copyTab", tabId: 7, frameId: 0, id: "abc", field: "password" }, page);
  assert.equal(fromPage.ok, false);
  assert.deepEqual(asked, []);
});

test("an older MYLE that does not know Copy is told apart", async () => {
  const { context } = await backgroundWithTab({ ok: false, error: "badRequest" });
  const answer = await context.handle({ type: "copyTab", tabId: 7, frameId: 0, id: "abc", field: "totp" }, popup);
  assert.equal(answer.error, "oldApp");
});

// A page's prompt waits on this one; a copy of MYLE the extension cannot read
// must not leave it polling for two minutes and then blaming the vault.
const page = { url: "https://accounts.google.com/signin", id: "test", tab: { id: 7, active: true }, frameId: 0 };

test("a status reply with no vault state ends the wait at once, not in two minutes", async () => {
  // What MYLE answered in the field: a status, but no "state" it understands.
  const { context, asked } = await backgroundWithTab({ ok: true, enabled: true, vault: "unlocked" });
  const answer = await Promise.race([
    context.handle({ type: "waitUnlocked", seconds: 30 }, page),
    new Promise((resolve) => setTimeout(() => resolve({ timedOut: true }), 2500)),
  ]);
  assert.equal(answer.timedOut, undefined, "it gave up instead of polling");
  assert.equal(answer.unlocked, false);
  assert.equal(answer.state, "noState");
  assert.equal(asked.length, 1, "one status is enough to know");
});

test("an open vault carries the page on", async () => {
  const { context } = await backgroundWithTab({ ok: true, enabled: true, state: "unlocked" });
  const answer = await context.handle({ type: "waitUnlocked", seconds: 30 }, page);
  assert.equal(answer.unlocked, true);
  assert.equal(answer.state, "unlocked");
});

test("browser filling off in MYLE is said, not waited out", async () => {
  const { context } = await backgroundWithTab({ ok: false, error: "disabled" });
  const answer = await Promise.race([
    context.handle({ type: "waitUnlocked", seconds: 30 }, page),
    new Promise((resolve) => setTimeout(() => resolve({ timedOut: true }), 2500)),
  ]);
  assert.equal(answer.timedOut, undefined);
  assert.equal(answer.state, "disabled");
  assert.equal(answer.unlocked, false);
});

for (const state of ["locked", "unlocked"]) {
  test(`native status with browser filling off and ${state} vault ends the wait`, async () => {
    const { context, asked } = await backgroundWithTab({ ok: true, enabled: false, state });
    const answer = await context.untilUnlocked(() => false, 1);
    assert.equal(answer.unlocked, false);
    assert.equal(answer.state, "disabled");
    assert.equal(asked.length, 1);
  });
}

for (const enabled of [true, false]) {
  test(`the native new vault state with filling ${enabled ? "on" : "off"} is reported as noVault without polling`, async () => {
    const { context, asked } = await backgroundWithTab({ ok: true, enabled, state: "new" });
    const answer = await context.untilUnlocked(() => false, 1);
    assert.equal(answer.state, "noVault");
    assert.equal(asked.length, 1);
  });
}

test("withdrawing an unlock wait while its status is in flight prevents resuming", async () => {
  let release;
  const { context } = await backgroundWithTab(() => new Promise((resolve) => { release = resolve; }));
  const waiting = context.handle({ type: "waitUnlocked", seconds: 2 }, page);
  await context.handle({ type: "stopWaiting" }, page);
  release({ ok: true, enabled: true, state: "unlocked" });
  assert.equal((await waiting).unlocked, false);
});

async function pendingOffer(reply) {
  const { context, asked } = await backgroundWithTab(reply);
  const pending = {
    "pending:7": { nonce: "offer", url: page.url, username: "me", password: "secret", at: Date.now(), shownAt: Date.now() },
  };
  context.chrome.storage = {
    session: {
      get: async (key) => ({ [key]: pending[key] }),
      remove: async (keys) => { for (const key of Array.isArray(keys) ? keys : [keys]) delete pending[key]; },
    },
  };
  return { context, asked, pending };
}

test("dismissing a locked Save prevents the deferred save after unlock", async () => {
  let releaseStatus;
  let statusStarted;
  const started = new Promise((resolve) => { statusStarted = resolve; });
  let saves = 0;
  const { context } = await pendingOffer((message) => {
    if (message.type === "save") return ++saves === 1 ? { ok: false, error: "locked" } : { ok: true };
    if (message.type === "status") {
      statusStarted();
      return new Promise((resolve) => { releaseStatus = resolve; });
    }
    if (message.type === "known") return { ok: true, known: false };
    return { ok: true };
  });
  const saving = context.handle({ type: "save", nonce: "offer" }, page);
  await started;
  assert.equal((await context.handle({ type: "dismiss", nonce: "offer" }, page)).ok, true);
  releaseStatus({ ok: true, enabled: true, state: "unlocked" });
  assert.equal((await saving).error, "expired");
  assert.equal(saves, 1, "only the initial locked attempt reached MYLE");
});

test("dismissing while the unlocked Save checks the existing login prevents its retry", async () => {
  let releaseKnown;
  let knownStarted;
  const started = new Promise((resolve) => { knownStarted = resolve; });
  let saves = 0;
  const { context } = await pendingOffer((message) => {
    if (message.type === "save") return ++saves === 1 ? { ok: false, error: "locked" } : { ok: true };
    if (message.type === "status") return { ok: true, enabled: true, state: "unlocked" };
    if (message.type === "known") {
      knownStarted();
      return new Promise((resolve) => { releaseKnown = resolve; });
    }
    return { ok: true };
  });
  const saving = context.handle({ type: "save", nonce: "offer" }, page);
  await started;
  await context.handle({ type: "dismiss", nonce: "offer" }, page);
  releaseKnown({ ok: true, known: false });
  assert.equal((await saving).error, "expired");
  assert.equal(saves, 1);
});

test("a locked Save still completes once when its offer is retained", async () => {
  let saves = 0;
  const { context, pending } = await pendingOffer((message) => {
    if (message.type === "save") return ++saves === 1 ? { ok: false, error: "locked" } : { ok: true, updated: true };
    if (message.type === "status") return { ok: true, enabled: true, state: "unlocked" };
    if (message.type === "known") return { ok: true, known: false };
    return { ok: true };
  });
  const answer = await context.handle({ type: "save", nonce: "offer" }, page);
  assert.equal(answer.ok, true);
  assert.equal(answer.updated, true);
  assert.equal(saves, 2);
  assert.equal(pending["pending:7"], undefined);
});

test("native replies release their deadline timer, including transport failures", async () => {
  for (const reply of [() => ({ ok: true }), () => { throw new Error("host exited"); }]) {
    const pending = new Set();
    const { context } = await backgroundWithTab(reply, {
      setTimeout(callback) { const token = { callback }; pending.add(token); return token; },
      clearTimeout(token) { pending.delete(token); },
    });
    await context.ask({ type: "save" });
    assert.equal(pending.size, 0);
  }
});

test("a native call that never answers still times out and releases its deadline", async () => {
  let fire;
  const pending = new Set();
  const { context } = await backgroundWithTab(() => new Promise(() => {}), {
    setTimeout(callback, milliseconds) {
      assert.equal(milliseconds, 30_000);
      const token = {};
      pending.add(token);
      fire = callback;
      return token;
    },
    clearTimeout(token) { pending.delete(token); },
  });
  const answer = context.ask({ type: "status" });
  fire();
  assert.equal((await answer).error, "noHost");
  assert.equal(pending.size, 0);
});

test("passkey cancellation targets only the requesting tab's native operation", async () => {
  const releases = [];
  const { context, asked } = await backgroundWithTab((message) => {
    if (message.type === "passkeyGet") return new Promise((resolve) => { releases.push(resolve); });
    return { ok: true };
  });
  const request = { type: "passkeyGet", requestId: "request", challenge: "Y2g", credentialId: "a" };
  const first = context.handle(request, page);
  const otherPage = { ...page, tab: { id: 8, active: true } };
  const other = context.handle(request, otherPage);
  await context.handle({ type: "passkeyCancel", requestId: "request" }, page);
  const messages = asked.map((item) => JSON.parse(item));
  assert.equal(messages[0].operationId?.length, 36);
  assert.notEqual(messages[0].operationId, messages[1].operationId);
  assert.equal(messages[2].type, "passkeyCancel");
  assert.equal(messages[2].operationId, messages[0].operationId);
  assert.equal(messages[2].url, page.url);
  for (const release of releases) release({ ok: false, error: "cancelled" });
  await Promise.all([first, other]);
});

test("passkey verification gets a long deadline and its timeout sends native cancellation", async () => {
  let fire;
  const { context, asked } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise(() => {}) : { ok: true }, {
    setTimeout(callback, milliseconds) {
      if (milliseconds === 150_000) fire = callback;
      return { callback };
    },
    clearTimeout() {},
  });
  const waiting = context.handle({ type: "passkeyGet", requestId: "request", challenge: "Y2g", credentialId: "a" }, page);
  assert.equal(typeof fire, "function", "verification is allowed longer than ordinary lookups");
  fire();
  assert.equal((await waiting).error, "noHost");
  const messages = asked.map((item) => JSON.parse(item));
  assert.equal(messages[1].type, "passkeyCancel");
  assert.equal(messages[1].operationId, messages[0].operationId);
});

test("a replacement document cannot cancel the previous document's passkey operation", async () => {
  let release;
  const { context, asked } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise((resolve) => { release = resolve; }) : { ok: true });
  const original = { ...page, documentId: "original" };
  const replaced = { ...page, documentId: "replacement" };
  const waiting = context.handle({ type: "passkeyGet", requestId: "request", challenge: "Y2g", credentialId: "a" }, original);
  await context.handle({ type: "passkeyCancel", requestId: "request" }, replaced);
  assert.equal(asked.length, 1, "only the requesting document owns its operation");
  await context.handle({ type: "passkeyCancel", requestId: "request" }, original);
  assert.equal(JSON.parse(asked[1]).type, "passkeyCancel");
  release({ ok: false, error: "cancelled" });
  await waiting;
});


test("closing a passkey prompt propagates cancellation for its request", async () => {
  const content = (await readFile(new URL("content.js", extension), "utf8")).replace(/\r\n/g, "\n");
  const start = content.indexOf("  function closePrompt() {");
  const end = content.indexOf("\n  }", start) + 4;
  const sent = [];
  const close = vm.runInNewContext(`(${content.slice(start, end).trim()})`, {
    prompted: "request", bar: {}, hide() {}, send: (message) => sent.push(message),
    displacedOffer: null, offerAfterPrompt: false, offerSave() {},
  });
  close();
  assert.equal(sent.some((message) => message.type === "passkeyCancel" && message.requestId === "request"), true);
});

test("a save offer the passkey prompt covered comes back when the prompt closes", async () => {
  const content = (await readFile(new URL("content.js", extension), "utf8")).replace(/\r\n/g, "\n");
  const start = content.indexOf("  function closePrompt() {");
  const end = content.indexOf("\n  }", start) + 4;
  let shown = 0;
  const context = vm.createContext({
    prompted: "request", bar: {}, hide() {}, send() {},
    displacedOffer: () => shown++, offerAfterPrompt: false, offerSave: () => assert.fail("not asked again"),
  });
  vm.runInContext(`(${content.slice(start, end).trim()})()`, context);
  assert.equal(shown, 1);
  assert.equal(context.displacedOffer, null);
});

test("a page abort of a conditional request also cancels native verification", async () => {
  const content = (await readFile(new URL("content.js", extension), "utf8")).replace(/\r\n/g, "\n");
  const start = content.indexOf('    addEventListener("message", (event) => {', content.indexOf("  if (window === window.top)"));
  const end = content.indexOf("\n    });", start) + 8;
  const sent = [];
  let listener;
  const window = {};
  vm.runInNewContext(content.slice(start, end), {
    window, FROM_PAGE: "myle-passkeys/page", heard: new Set(),
    waitingPasskey: { id: "request" }, prompted: null,
    addEventListener: (_kind, handler) => { listener = handler; },
    send: (message) => sent.push(message), closePrompt() {},
  });
  listener({ source: window, data: { source: "myle-passkeys/page", id: "request", kind: "abort" } });
  assert.equal(sent[0].type, "passkeyCancel");
  assert.equal(sent[0].requestId, "request");
});

test("a question from the page is acknowledged, so the page knows MYLE heard it", async () => {
  const content = (await readFile(new URL("content.js", extension), "utf8")).replace(/\r\n/g, "\n");
  const start = content.indexOf('    addEventListener("message", (event) => {', content.indexOf("  if (window === window.top)"));
  const end = content.indexOf("\n    });", start) + 8;
  const posted = [];
  let listener;
  const window = { postMessage: (data) => posted.push(data) };
  vm.runInNewContext(content.slice(start, end), {
    window, location: { origin: "https://example.com" }, FROM_PAGE: "myle-passkeys/page", TO_PAGE: "myle-passkeys/extension",
    heard: new Set(), waitingPasskey: null, prompted: null, debug() {},
    addEventListener: (_kind, handler) => { listener = handler; },
    send() {}, closePrompt() {}, answerPage() {},
  });
  listener({ source: window, data: { source: "myle-passkeys/page", id: "q1", kind: "conditional", options: {} } });
  assert.equal(JSON.stringify(posted), JSON.stringify([{ source: "myle-passkeys/extension", kind: "ack", id: "q1" }]));
});

async function pagehide() {
  const content = (await readFile(new URL("content.js", extension), "utf8")).replace(/\r\n/g, "\n");
  const start = content.indexOf('  addEventListener("pagehide", (event) => {');
  assert.ok(start >= 0);
  const end = content.indexOf("\n  });", start) + 7;
  const sent = [];
  let leaving;
  const context = vm.createContext({
    prompted: "prompt", waitingPasskey: { id: "conditional" }, bar: {},
    hidePanel() {}, hide() {}, send: (message) => sent.push(message),
    addEventListener: (_kind, handler) => { leaving = handler; },
  });
  vm.runInContext(content.slice(start, end), context);
  return { context, sent, leaving: (event) => leaving(event) };
}

test("the actual pagehide handler cancels prompted and conditional passkey requests", async () => {
  const { context, sent, leaving } = await pagehide();
  leaving({ persisted: false });
  assert.deepEqual(sent.filter((message) => message.type === "passkeyCancel").map((message) => message.requestId).sort(), ["conditional", "prompt"]);
  assert.equal(context.prompted, null);
  assert.equal(context.waitingPasskey, null);
});

test("a page kept in the back/forward cache keeps its waiting sign-in form", async () => {
  const { context, sent, leaving } = await pagehide();
  leaving({ persisted: true });
  assert.deepEqual(sent.filter((message) => message.type === "passkeyCancel").map((message) => message.requestId), ["prompt"]);
  assert.equal(context.waitingPasskey.id, "conditional");
});

// --- The frames of a tab, without webNavigation -------------------------------------

test("the census takes each frame's address from the browser, never from what it says", async () => {
  const frames = [
    ...defaultFrames(),
    // A frame claiming another site's address in its answer.
    { frameId: 5, url: "https://ads.example.net/", documentId: "ad", sees: { fields: true, url: "https://bank.com/" } },
  ];
  const { context, asked } = await backgroundWithTab({ ok: true, logins: [] }, {}, frames);
  const answer = await context.handle({ type: "tabLogins", tabId: 7 }, popup);
  assert.equal(answer.ok, true);
  const urls = asked.map((message) => JSON.parse(message).url);
  assert.deepEqual(urls, ["https://example.com/login", "https://auth.other.com/frame", "https://ads.example.net/"]);
});

test("a census answer for another tab or an old round is ignored", async () => {
  const { context } = await backgroundWithTab();
  // No census is being taken: nothing to add to.
  const stray = await context.handle({ type: "present", round: "made-up", fields: true },
    { id: "test", url: "https://example.com/", frameId: 0, tab: { id: 7, active: true } });
  assert.equal(stray.ok, true);
});

test("Fill goes to the very document the vault was asked about", async () => {
  const { context, asked, applied } = await backgroundWithTab({ ok: true, username: "me", password: "pw" });
  const answer = await context.handle({ type: "fillTab", tabId: 7, frameId: 3, id: "abc" }, popup);
  assert.equal(answer.applied, true);
  assert.equal(JSON.parse(asked[0]).url, "https://auth.other.com/frame");
  assert.equal(JSON.stringify(applied[0].options), JSON.stringify({ documentId: "frame-doc" }));
  assert.equal(applied[0].message.origin, "https://auth.other.com");
});

test("without document ids (older Firefox) Fill checks the frame again and sends to its number", async () => {
  const frames = defaultFrames().map(({ documentId, ...frame }) => frame);
  const { context, applied } = await backgroundWithTab({ ok: true, username: "me", password: "pw" }, {}, frames);
  const answer = await context.handle({ type: "fillTab", tabId: 7, frameId: 0, id: "abc" }, popup);
  assert.equal(answer.applied, true);
  assert.equal(JSON.stringify(applied[0].options), JSON.stringify({ frameId: 0 }));
});

test("a page whose script is not running (opened before an update) says reload, and asks MYLE nothing", async () => {
  const { context, asked } = await backgroundWithTab({ ok: true }, {}, []);
  const answer = await context.handle({ type: "fillTab", tabId: 7, frameId: 0, id: "abc" }, popup);
  assert.equal(answer.error, "noContent");
  assert.deepEqual(asked, []);
});

test("one login in the page and in a sign-in frame of the same site is one row, for the better frame", async () => {
  const frames = [
    { frameId: 0, url: "https://example.com/", documentId: "top", sees: { fields: true } },
    { frameId: 4, url: "https://login.example.com/frame", documentId: "login", sees: { fields: true, password: true, focused: true } },
  ];
  const { context } = await backgroundWithTab({ ok: true, logins: [{ id: "a", title: "Example" }] }, {}, frames);
  const answer = await context.handle({ type: "tabLogins", tabId: 7 }, popup);
  assert.equal(answer.logins.length, 1);
  assert.equal(answer.logins[0].frameId, 4);
  assert.equal(answer.logins[0].embedded, false);
});

test("a frame of another site inside the page is told apart by the tab's own address", async () => {
  const { context, asked } = await backgroundWithTab({ ok: true, logins: [] });
  const sender = (tabUrl) => ({ id: "test", url: "https://auth.other.com/frame", frameId: 3, tab: { id: 7, active: true, url: tabUrl } });
  const embedded = await context.handle({ type: "logins" }, sender("https://example.com/login"));
  assert.equal(embedded.error, "embedded");
  assert.equal(embedded.top, "example.com");
  const unknown = await context.handle({ type: "logins" }, sender(undefined));
  assert.equal(unknown.error, "embedded", "no address for the page: treated as another site");
  const sameSite = await context.handle({ type: "logins" }, sender("https://www.other.com/"));
  assert.equal(sameSite.ok, true);
  assert.equal(asked.filter((message) => JSON.parse(message).type === "logins").length, 1);
});

// --- Passkeys on a port --------------------------------------------------------------

const passkeyGet = { type: "passkeyGet", requestId: "request", challenge: "Y2g", credentialId: "a" };

test("a passkey request on a port is answered there, then the port closes", async () => {
  const { connect } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? { ok: true, credential: { id: "x" } } : { ok: true });
  const port = connect({ ...page, documentId: "doc" });
  port.send(passkeyGet);
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(port.posted[0].credential.id, "x");
  assert.equal(port.closed, true);
});

test("the page leaving (its port closed) cancels its passkey operation in MYLE", async () => {
  let release;
  const { connect, asked } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise((resolve) => { release = resolve; }) : { ok: true });
  const port = connect({ ...page, documentId: "doc" });
  port.send(passkeyGet);
  await new Promise((resolve) => setTimeout(resolve, 10));
  port.gone();
  const messages = asked.map((message) => JSON.parse(message));
  assert.equal(messages[1].type, "passkeyCancel");
  assert.equal(messages[1].operationId, messages[0].operationId);
  assert.equal(messages[1].url, page.url);
  release({ ok: false, error: "cancelled" });
});

test("a port closed after its answer cancels nothing", async () => {
  const { connect, asked } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? { ok: false, error: "notVerified" } : { ok: true });
  const port = connect({ ...page, documentId: "doc" });
  port.send(passkeyGet);
  await new Promise((resolve) => setTimeout(resolve, 10));
  port.gone();
  assert.equal(asked.length, 1);
});

test("a passkey port from the popup, or from another extension, is refused", async () => {
  const { connect, asked } = await backgroundWithTab();
  const fromPopup = connect(popup);
  const fromOther = connect({ ...page, id: "other" });
  assert.equal(fromPopup.closed, true);
  assert.equal(fromOther.closed, true);
  assert.equal(asked.length, 0);
});

test("a cancel from a tab the user moved away from still reaches MYLE", async () => {
  let release;
  const { context, asked } = await backgroundWithTab((message) =>
    message.type === "passkeyCreate" ? new Promise((resolve) => { release = resolve; }) : { ok: true });
  const create = { type: "passkeyCreate", requestId: "request", challenge: "Y2g", userId: "dQ" };
  const waiting = context.handle(create, page);
  const away = { ...page, tab: { id: 7, active: false }, documentLifecycle: "pending_deletion" };
  await context.handle({ type: "passkeyCancel", requestId: "request" }, away);
  const messages = asked.map((message) => JSON.parse(message));
  assert.equal(messages[1]?.type, "passkeyCancel");
  assert.equal(messages[1].operationId, messages[0].operationId);
  release({ ok: false, error: "cancelled" });
  await waiting;
});

test("the same request asked twice while MYLE works on it is not a failure", async () => {
  let release;
  const { context } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise((resolve) => { release = resolve; }) : { ok: true });
  const first = context.handle(passkeyGet, page);
  const second = await context.handle(passkeyGet, page);
  assert.equal(second.inProgress, true);
  release({ ok: false, error: "cancelled" });
  await first;
});

test("operations left by a worker that stopped are cancelled when it starts again", async () => {
  const stored = { "passkeyOp:left": { id: "left", url: "https://accounts.google.com/signin" }, other: 1 };
  const storage = {
    session: {
      get: async () => ({ ...stored }),
      set: async (values) => Object.assign(stored, values),
      remove: async (key) => { delete stored[key]; },
    },
  };
  const { asked } = await backgroundWithTab({ ok: true }, {}, defaultFrames(), storage);
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(asked.map((message) => JSON.parse(message)), [
    { type: "passkeyCancel", url: "https://accounts.google.com/signin", operationId: "left", extVersion: "" },
  ]);
  assert.equal(stored["passkeyOp:left"], undefined);
  assert.equal(stored.other, 1);
});

// --- Limits, retries, salt --------------------------------------------------------------

test("only the page itself scans pictures for a 2FA code, a few times a minute", async () => {
  const { context, asked } = await backgroundWithTab({ ok: false, error: "noQr" });
  const pixels = { type: "qrPixels", width: 1, height: 1, pixels: "AA" };
  const frame = { ...page, url: "https://ads.example.net/", frameId: 2 };
  assert.equal((await context.handle(pixels, frame)).error, "badRequest");
  for (let i = 0; i < 10; i++) await context.handle(pixels, page);
  assert.equal((await context.handle(pixels, page)).error, "busy");
  assert.equal(asked.length, 10);
});

test("a read is asked once more when MYLE is starting; a save never is", async () => {
  let calls = 0;
  const { context } = await backgroundWithTab((message) => {
    calls++;
    if (message.type === "save") return { ok: false, error: "notRunning" };
    return calls === 1 ? { ok: false, error: "notRunning" } : { ok: true, state: "unlocked", enabled: true };
  });
  const status = await context.ask({ type: "status" });
  assert.equal(status.ok, true);
  assert.equal(calls, 2);
  calls = 0;
  const save = await context.ask({ type: "save" });
  assert.equal(save.error, "notRunning");
  assert.equal(calls, 1);
});

test("passkey sites made at the same time share one salt", async () => {
  const local = {};
  let saltWrites = 0;
  const storage = {
    local: {
      get: async (key) => { await new Promise((resolve) => setTimeout(resolve, 1)); return { [key]: local[key] }; },
      set: async (values) => { if ("passkeySalt" in values) saltWrites++; Object.assign(local, values); },
    },
  };
  const { context } = await backgroundWithTab({ ok: true }, {}, defaultFrames(), storage);
  const [a, b] = await Promise.all([context.passkeySiteKey("example.com"), context.passkeySiteKey("example.com")]);
  assert.equal(a, b);
  assert.equal(saltWrites, 1);
});

test("asking to open MYLE twice within two seconds says nothing new happened", async () => {
  const { context, asked } = await backgroundWithTab();
  assert.equal((await context.handle({ type: "open" }, popup)).throttled, undefined);
  assert.equal((await context.handle({ type: "open" }, popup)).throttled, true);
  assert.equal(asked.length, 1);
});
