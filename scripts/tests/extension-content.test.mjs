// Pieces of the extension's page script (content.js) that need no page: `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
// A Windows checkout may have CRLF line endings: read it as LF.
const source = (await readFile(new URL("../../extension/content.js", import.meta.url), "utf8")).replace(/\r\n/g, "\n");
test("an RP mismatch leaves related-origin validation to the browser without asking MYLE to sign", async () => {
  const start = source.indexOf("  async function passkeysHere(");
  const end = source.indexOf("\n  }\n", start);
  const answers = [];
  const calls = [];
  const scope = vm.createContext({
    prompted: "request", siteName: "example.com", debug() {},
    send: async (message) => { calls.push(message); return { ok: false, error: "rpMismatch" }; },
    answerPage: (id, answer) => answers.push({ id, ...answer }),
    refuse: (id, error) => answers.push({ id, result: "error", error }),
  });
  const passkeysHere = vm.runInContext(`(${source.slice(start, end + 4).trim()})`, scope);
  assert.equal(await passkeysHere("request", { rpId: "related.example", allow: [] }, () => assert.fail("must not retry")), null);
  assert.deepEqual(answers, [{ id: "request", result: "native" }]);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].type, "passkeyList");
  assert.equal(calls[0].rpId, "related.example");
  assert.equal(scope.prompted, null);
});
/** One function of content.js, by name, run on its own. */
function pick(name) {
  const start = source.indexOf(`  function ${name}(`);
  assert.ok(start >= 0, `${name} is in content.js`);
  const end = source.indexOf("\n  }\n", start);
  return vm.runInNewContext(`(${source.slice(start, end + 4).trim()})`);
}
function unlockWait(context) {
  const start = source.indexOf("  async function carryOnWhenUnlocked(");
  assert.ok(start >= 0);
  const end = source.indexOf("\n  }\n", start);
  const scope = vm.createContext({
    prompted: "request", promptToken: {}, UNLOCK_WAIT: 180, problems: {}, debug() {}, ...context,
  });
  const wait = vm.runInContext(`(${source.slice(start, end + 4).trim()})`, scope);
  wait.scope = scope;
  return wait;
}
for (const [label, reply] of [
  ["refused", { ok: false, error: "badRequest" }],
  ["disconnected", { ok: false, error: "noHost" }],
  ["missing", undefined],
  ["missing state", { ok: true }],
]) {
  test(`a ${label} unlock reply stops polling and explains the connection failure`, async () => {
    let calls = 0;
    const wait = unlockWait({
      send: async () => {
        if (++calls > 1) throw new Error("the failed reply was polled again");
        return reply;
      },
    });
    const shown = { textContent: "" };
    await wait("request", shown, () => assert.fail("a failed check must not sign in"));
    assert.match(shown.textContent, /extension|connection/i);
    assert.equal(calls, 1);
  });
}
test("a valid unlocked reply resumes the waiting sign-in once", async () => {
  const wait = unlockWait({ send: async () => ({ ok: true, state: "unlocked", unlocked: true }) });
  const shown = { textContent: "" };
  let retries = 0;
  await wait("request", shown, () => retries++);
  assert.equal(retries, 1);
  assert.match(shown.textContent, /Signing you in/);
});
test("a 2FA key written on a setup page is told from other text", () => {
  const keyIn = pick("keyIn");
  // As sites write them: groups of four, either case, or one block.
  assert.equal(keyIn("JBSW Y3DP EHPK 3PXP JBSW Y3DP EHPK 3PXP"), "JBSW Y3DP EHPK 3PXP JBSW Y3DP EHPK 3PXP");
  assert.equal(keyIn("  hxdm vjec jjws rb3h wizr 4ifu gftm xboz "), "hxdm vjec jjws rb3h wizr 4ifu gftm xboz");
  assert.equal(keyIn("HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ"), "HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ");
  // The QR code's link, wherever it is written.
  assert.equal(
    keyIn('Or open otpauth://totp/Site:me?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=Site in your app'),
    "otpauth://totp/Site:me?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=Site",
  );
  // Not keys: words, mixed case, 0/1/8/9, too short.
  assert.equal(keyIn("AUTHENTICATIONREQUIRED"), null);
  assert.equal(keyIn("Scan this QR code with your app"), null);
  assert.equal(keyIn("HxDmVjEcJjWsRb3HwIzR4IfU"), null);
  assert.equal(keyIn("ORDER-1234-5678-9012-3456"), null);
  assert.equal(keyIn("JBSW Y3DP"), null);
});
test("a prompt drawn again ends the earlier wait, so the sign-in is carried on once", async () => {
  let release;
  const wait = unlockWait({
    send: () => new Promise((resolve) => { release = resolve; }),
  });
  const shown = { textContent: "" };
  let retries = 0;
  const waiting = wait("request", shown, () => retries++);
  // "Try again" draws the prompt again while the first wait is with MYLE.
  wait.scope.promptToken = {};
  release({ ok: true, state: "unlocked", unlocked: true });
  await waiting;
  assert.equal(retries, 0, "only the wait of the prompt on screen carries the sign-in on");
});
test("a copy left behind by an extension update answers instead of throwing", async () => {
  const removed = [];
  const send = vm.runInNewContext(`(${source.slice(source.indexOf("  function send("), source.indexOf("\n  }\n", source.indexOf("  function send(")) + 4).trim()})`, {
    ext: { runtime: { sendMessage() { throw new Error("Extension context invalidated."); } } },
    lost: () => ({ ok: false, error: "noHost" }),
    orphaned: () => removed.push(true),
    Promise,
  });
  assert.equal((await send({ type: "logins" })).error, "reloaded");
  assert.equal(removed.length, 1, "a fresh copy may take the page over");
});
test("a rejected message is a missing host while the extension lives, and reloaded once it is gone", async () => {
  const start = source.indexOf("  const lost = ");
  const line = source.slice(start, source.indexOf("\n", start));
  const alive = vm.runInNewContext(`${line}; lost`, { ext: { runtime: { id: "x" } } });
  const gone = vm.runInNewContext(`${line}; lost`, { ext: { runtime: {} } });
  assert.equal(alive().error, "noHost");
  assert.equal(gone().error, "reloaded");
});
test("the 2FA key prompt closes only itself", () => {
  const settleKey = vm.runInNewContext(`(${source.slice(source.indexOf("  function settleKey("), source.indexOf("\n  }\n", source.indexOf("  function settleKey(")) + 4).trim()})`, {
    get prompted() { return "passkey-request"; },
    closePrompt: () => assert.fail("a passkey prompt that replaced the key prompt must stay"),
    setTimeout: (run) => run(),
  });
  settleKey({ textContent: "" }, "Saved.", "totp-1");
});
