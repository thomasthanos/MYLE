// Getting the browser extension from the app (BrowserFilling.svelte and
// Settings.svelte): `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = async (path) => (await readFile(new URL(`../../frontend/app/pages/${path}`, import.meta.url), "utf8")).replace(/\r\n/g, "\n");
const filling = await read("password-manager/BrowserFilling.svelte");
const settings = await read("settings/Settings.svelte");

test("the Chrome Web Store button is always in view, not inside the folder steps", () => {
  const button = filling.indexOf("Install from Chrome Web Store");
  const steps = filling.indexOf("{#if showSteps}");
  assert.ok(button > 0, "the button exists");
  assert.ok(steps > 0);
  assert.ok(button < steps, "the store button comes before (outside) the collapsible folder steps");
  assert.match(filling, /const storeUrl = EXTENSION_URL;/);
});

test("a store page that cannot be opened is said, never swallowed", () => {
  assert.equal(/openUrl\([^)]*\)\.catch\(\(\) => \{\}\)/.test(filling), false);
  assert.match(filling, /toast\.error\(`Could not open the Chrome Web Store/);
});

test("the browser-contact listener is removed even when the dialog closes first", () => {
  assert.match(filling, /\.then\(\(unlisten\) => \(closed \? unlisten\(\) : \(stop = unlisten\)\)\)/);
});

test("Settings offers the extension from the Chrome Web Store too, and finds it by search", () => {
  assert.match(settings, /Install from Chrome Web Store/);
  assert.match(settings, /import \{ EXTENSION_URL \} from "..\/..\/..\/lib\/extension";/);
  assert.match(settings, /extension: "browser extension chrome web store/);
});

test("the store page is the official listing of MYLE Passwords, put in at build time", async () => {
  const lib = await readFile(new URL("../../frontend/lib/extension.ts", import.meta.url), "utf8");
  assert.match(lib, /import\.meta\.env\.VITE_EXTENSION_URL/);
  const pkg = JSON.parse(await readFile(new URL("../../package.json", import.meta.url), "utf8"));
  assert.equal(pkg.myle.extensionUrl, "https://chromewebstore.google.com/detail/mifjffbnaeeljjfboiglbcoaokgdilca");
});
