// The extension's two manifests (Chrome, Firefox): what the extension may do. `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
const manifest = async (path) => JSON.parse(await readFile(new URL(`../../extension/${path}`, import.meta.url), "utf8"));
const chrome = await manifest("manifest.json");
const firefox = await manifest("firefox/manifest.json");
test("the extension asks only for what it needs, in both browsers", () => {
  for (const each of [chrome, firefox]) {
    assert.deepEqual(each.permissions, ["nativeMessaging", "activeTab", "storage"]);
    assert.equal(each.permissions.includes("webNavigation"), false);
    assert.equal(each.host_permissions, undefined);
  }
});
test("both manifests carry the same version, scripts and toolbar button", () => {
  for (const key of ["version", "permissions", "content_scripts", "action", "commands"]) {
    assert.equal(JSON.stringify(chrome[key]), JSON.stringify(firefox[key]), key);
  }
});
test("the page scripts still run where filling and passkeys need them", () => {
  const [passkeys, content] = chrome.content_scripts;
  assert.deepEqual(passkeys.js, ["passkeys.js"]);
  assert.equal(passkeys.world, "MAIN");
  assert.equal(passkeys.run_at, "document_start");
  assert.deepEqual(content.js, ["content.js"]);
  assert.equal(content.all_frames, true);
});
