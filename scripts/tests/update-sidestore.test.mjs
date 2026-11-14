import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { feeds } from "../mobile-feeds.mjs";
import { updatedSource } from "../update-sidestore.mjs";

const repository = JSON.parse(readFileSync(new URL("../../docs/sidestore.json", import.meta.url), "utf8"));
const file = { size: 6202538, sha256: "a".repeat(64) };

function generated() {
  return feeds({
    version: "10.0.9",
    tag: "mobile-v10.0.9",
    base: "https://downloads.thomast.uk",
    repo: "thomasthanos/MYLE",
    whatsNew: "MYLE Passwords 10.0.9",
    apk: file,
    ipa: file,
  }).sidestore;
}

test("the repository's source takes the published version: its R2 address, size and SHA-256", () => {
  const updated = updatedSource(repository, generated());
  const [version] = updated.apps[0].versions;
  assert.equal(updated.apps[0].versions.length, 1);
  assert.equal(version.version, "10.0.9");
  assert.equal(version.downloadURL, "https://downloads.thomast.uk/MYLE-Passwords-10.0.9.ipa");
  assert.equal(version.sha256, file.sha256);
  assert.equal(version.size, file.size);
  assert.deepEqual(version, generated().apps[0].versions[0]);
});

test("the rest of the repository's source is left as it is", () => {
  const updated = updatedSource(repository, generated());
  assert.equal(updated.identifier, repository.identifier);
  assert.equal(updated.apps[0].bundleIdentifier, repository.apps[0].bundleIdentifier);
  assert.equal(updated.apps[0].localizedDescription, repository.apps[0].localizedDescription);
});

test("a generated source without a checksum or an https address is refused", () => {
  const broken = generated();
  delete broken.apps[0].versions[0].sha256;
  assert.throws(() => updatedSource(repository, broken), /SHA-256/);
  const plain = generated();
  plain.apps[0].versions[0].downloadURL = "http://downloads.thomast.uk/x.ipa";
  assert.throws(() => updatedSource(repository, plain));
  assert.throws(() => updatedSource(repository, { apps: [] }));
});
