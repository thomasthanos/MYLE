// The feeds of a MYLE Passwords release: `node --test scripts/tests/`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { APP_ID, feeds, MIN_IOS } from "../mobile-feeds.mjs";

const release = {
  version: "1.2.3",
  tag: "mobile-v1.2.3",
  base: "https://downloads.thomast.uk",
  repo: "thomasthanos/MYLE",
  apk: { size: 100, sha256: "aa" },
  ipa: { size: 200, sha256: "bb" },
  date: new Date("2026-10-04T12:00:00.123Z"),
};

test("Android reads the version and the APK where update.rs looks", async () => {
  const { latest } = feeds(release);
  assert.equal(latest.version, "1.2.3");
  assert.equal(latest.android.url, "https://downloads.thomast.uk/MYLE-Passwords-1.2.3.apk");
  assert.equal(latest.pubDate, "2026-10-04T12:00:00Z");
  // The app's own feed address is next to the files the feed names.
  const update = await readFile(new URL("../../backend/mobile/src/update.rs", import.meta.url), "utf8");
  assert.match(update, /pub const FEED: &str = "https:\/\/downloads\.thomast\.uk\/mobile-latest\.json";/);
});

test("SideStore gets the IPA with the app's identity", async () => {
  const { sidestore } = feeds(release);
  const [app] = sidestore.apps;
  const conf = JSON.parse(await readFile(new URL("../../backend/mobile/tauri.conf.json", import.meta.url), "utf8"));
  assert.equal(app.bundleIdentifier, APP_ID);
  assert.equal(conf.identifier, APP_ID);
  assert.equal(conf.bundle.iOS.minimumSystemVersion, MIN_IOS);
  assert.deepEqual(app.versions[0], {
    version: "1.2.3",
    buildVersion: "1.2.3",
    date: "2026-10-04",
    localizedDescription: "What's new: https://github.com/thomasthanos/MYLE/releases/tag/mobile-v1.2.3",
    versionDescription: "What's new: https://github.com/thomasthanos/MYLE/releases/tag/mobile-v1.2.3",
    downloadURL: "https://downloads.thomast.uk/MYLE-Passwords-1.2.3.ipa",
    size: 200,
    sha256: "bb",
    minOSVersion: MIN_IOS,
  });
});

test("the app's version is the same in its two files", async () => {
  const conf = JSON.parse(await readFile(new URL("../../backend/mobile/tauri.conf.json", import.meta.url), "utf8"));
  const cargo = await readFile(new URL("../../backend/mobile/Cargo.toml", import.meta.url), "utf8");
  assert.equal(cargo.match(/^version = "(.+)"$/m)?.[1], conf.version);
});

test("iOS Info.plist template registers the deep link scheme", async () => {
  const plist = await readFile(new URL("../../backend/mobile/Info.ios.plist", import.meta.url), "utf8");
  assert.match(plist, /<key>CFBundleURLTypes<\/key>/);
  assert.match(plist, /<string>uk\.thomast\.myle\.passwords<\/string>/);
});


test("SideStore shows the release notes as plain text when there are some", async () => {
  const { feeds: make, plainNotes } = await import("../mobile-feeds.mjs");
  const text = plainNotes("### Fixed\n- **Discord** sign-in uses a `private` sheet ([details](https://x)).");
  assert.equal(text, "Fixed\n• Discord sign-in uses a private sheet (details).");
  const { sidestore } = make({ version: "1.0.0", tag: "v1.0.0", base: "https://d", repo: "o/r", apk: { size: 1, sha256: "a" }, ipa: { size: 2, sha256: "b" }, whatsNew: text });
  assert.equal(sidestore.apps[0].versions[0].versionDescription, text);
  assert.equal(sidestore.apps[0].versions[0].localizedDescription, text);
});
