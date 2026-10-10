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
async function backgroundWithTab(reply = { ok: true }) {
  const asked = [];
  const listener = { addListener() {} };
  const context = vm.createContext({
    chrome: {
      runtime: {
        getURL: () => "chrome-extension://test/",
        onMessage: listener,
        id: "test",
        sendNativeMessage: async (_host, message) => {
          asked.push(JSON.stringify(message));
          return reply;
        },
      },
      tabs: { onRemoved: listener, get: async () => ({ id: 7, active: true }) },
      webNavigation: {
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
  });
  context.globalThis = context;
  for (const file of ["psl.js", "background.js"]) {
    vm.runInContext(await readFile(new URL(file, extension), "utf8"), context, { filename: file });
  }
  return { context, asked };
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
