// Pieces of the extension's page script (content.js) that need no page:
// `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

// A Windows checkout may have CRLF line endings: read it as LF.
const source = (await readFile(new URL("../../extension/content.js", import.meta.url), "utf8")).replace(/\r\n/g, "\n");

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
  return vm.runInNewContext(`(${source.slice(start, end + 4).trim()})`, {
    prompted: "request", UNLOCK_WAIT: 180, problems: {}, debug() {}, ...context,
  });
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
