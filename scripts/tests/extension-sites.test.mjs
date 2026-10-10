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

/** background.js with a tab of two frames, recording what reaches MYLE. */
async function backgroundWithTab(reply = { ok: true }, scheduling = {}) {
  const asked = [];
  const navigationListeners = [];
  const listener = { addListener() {} };
  const context = vm.createContext({
    chrome: {
      runtime: {
        getURL: () => "chrome-extension://test/",
        onMessage: listener,
        id: "test",
        sendNativeMessage: async (_host, message) => {
          asked.push(JSON.stringify(message));
          return typeof reply === "function" ? reply(message) : reply;
        },
      },
      tabs: { onRemoved: listener, get: async () => ({ id: 7, active: true }) },
      webNavigation: {
        onCommitted: { addListener: (listener) => navigationListeners.push(listener) },
        getAllFrames: async () => [
          { frameId: 0, url: "https://example.com/login" },
          { frameId: 3, url: "https://auth.other.com/frame" },
        ],
      },
    },
    crypto: globalThis.crypto,
    URL,
    setTimeout,
    clearTimeout,
    ...scheduling,
  });
  context.globalThis = context;
  for (const file of ["psl.js", "background.js"]) {
    vm.runInContext(await readFile(new URL(file, extension), "utf8"), context, { filename: file });
  }
  return { context, asked, navigate: (details) => navigationListeners.forEach((listener) => listener(details)) };
}

const popup = { url: "chrome-extension://test/popup.html", id: "test" };

test("the popup's Copy asks MYLE with the frame's own address, and gets no text back", async () => {
  const { context, asked } = await backgroundWithTab();
  const answer = await context.handle({ type: "copyTab", tabId: 7, frameId: 3, id: "abc", field: "password" }, popup);
  assert.equal(JSON.stringify(answer), JSON.stringify({ ok: true }));
  assert.deepEqual(asked, [JSON.stringify({ type: "copy", id: "abc", url: "https://auth.other.com/frame", field: "password" })]);
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
    await context.ask({ type: "status" });
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
  });
  close();
  assert.equal(sent.some((message) => message.type === "passkeyCancel" && message.requestId === "request"), true);
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

test("the actual pagehide handler cancels prompted and conditional passkey requests", async () => {
  const content = (await readFile(new URL("content.js", extension), "utf8")).replace(/\r\n/g, "\n");
  const start = content.indexOf('  addEventListener("pagehide", () => {');
  const end = content.indexOf("\n  });", start) + 7;
  const sent = [];
  let leaving;
  const context = vm.createContext({
    prompted: "prompt", waitingPasskey: { id: "conditional" }, bar: {},
    hidePanel() {}, hide() {}, send: (message) => sent.push(message),
    addEventListener: (_kind, handler) => { leaving = handler; },
  });
  vm.runInContext(content.slice(start, end), context);
  leaving();
  assert.deepEqual(sent.filter((message) => message.type === "passkeyCancel").map((message) => message.requestId).sort(), ["conditional", "prompt"]);
  assert.equal(context.prompted, null);
  assert.equal(context.waitingPasskey, null);
});

test("a committed navigation cancels only departed document operations", async () => {
  const releases = [];
  const { context, asked, navigate } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise((resolve) => { releases.push(resolve); }) : { ok: true });
  const request = { type: "passkeyGet", requestId: "request", challenge: "Y2g", credentialId: "a" };
  const pending = [
    context.handle(request, { ...page, documentId: "departed" }),
    context.handle(request, { ...page, documentId: "current" }),
    context.handle(request, { ...page, tab: { id: 8, active: true }, documentId: "other-tab" }),
  ];
  navigate({ tabId: 7, frameId: 3, documentId: "subframe" });
  assert.equal(asked.length, 3, "a child frame's navigation cannot cancel top-frame operations");
  navigate({ tabId: 7, frameId: 0, documentId: "current", url: page.url });
  const messages = asked.map((message) => JSON.parse(message));
  assert.equal(messages.length, 4);
  assert.equal(messages[3].type, "passkeyCancel");
  assert.equal(messages[3].operationId, messages[0].operationId);
  for (const release of releases) release({ ok: false, error: "cancelled" });
  await Promise.all(pending);
});

test("same-document URL changes still allow cancellation when documentId is unavailable", async () => {
  let release;
  const { context, asked } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise((resolve) => { release = resolve; }) : { ok: true });
  const pending = context.handle({ type: "passkeyGet", requestId: "request", challenge: "Y2g", credentialId: "a" }, page);
  try {
    await context.handle({ type: "passkeyCancel", requestId: "request" }, { ...page, url: "https://accounts.google.com/next#passkey" });
    const messages = asked.map((message) => JSON.parse(message));
    assert.equal(messages[1]?.type, "passkeyCancel");
    assert.equal(messages[1].operationId, messages[0].operationId);
    assert.equal(messages[1].url, page.url, "native cancellation keeps the original authorized URL");
  } finally {
    release({ ok: false, error: "cancelled" });
    await pending;
  }
});

test("a delayed navigation event without document IDs preserves operations started afterward", async () => {
  const releases = [];
  let now = 100;
  const { context, asked, navigate } = await backgroundWithTab((message) =>
    message.type === "passkeyGet" ? new Promise((resolve) => { releases.push(resolve); }) : { ok: true }, {
    Date: class extends Date { static now() { return now; } },
  });
  const request = { type: "passkeyGet", challenge: "Y2g", credentialId: "a" };
  const old = context.handle({ ...request, requestId: "old" }, page);
  now = 200;
  const current = context.handle({ ...request, requestId: "current" }, page);
  try {
    navigate({ tabId: 7, frameId: 0, timeStamp: 150, url: page.url });
    const messages = asked.map((message) => JSON.parse(message));
    assert.equal(messages.length, 3);
    assert.equal(messages[2].operationId, messages[0].operationId);
  } finally {
    for (const release of releases) release({ ok: false, error: "cancelled" });
    await Promise.all([old, current]);
  }
});
