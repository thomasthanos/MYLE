// The setup's idea of newer and older (frontend/installer/versions.ts): `node --test "scripts/tests/*.test.mjs"`.
import assert from "node:assert/strict";
import test from "node:test";
import { compareVersions } from "../../frontend/installer/versions.ts";
test("releases are ordered by their numbers", () => {
  assert.equal(compareVersions("9.0.1", "9.0.0"), 1);
  assert.equal(compareVersions("9.0.0", "9.1.0"), -1);
  assert.equal(compareVersions("10.0.0", "9.9.9"), 1);
  assert.equal(compareVersions("9.0", "9.0.0"), 0);
  assert.equal(compareVersions("v9.0.0", "9.0.0"), 0);
});
test("a pre-release comes before its release", () => {
  assert.equal(compareVersions("9.0.0-beta.1", "9.0.0"), -1);
  assert.equal(compareVersions("9.0.0", "9.0.0-rc.1"), 1);
  assert.equal(compareVersions("9.0.1-beta.1", "9.0.0"), 1);
});
test("pre-releases follow Semantic Versioning's order", () => {
  const ordered = [
    "1.0.0-alpha",
    "1.0.0-alpha.1",
    "1.0.0-alpha.beta",
    "1.0.0-beta",
    "1.0.0-beta.2",
    "1.0.0-beta.11",
    "1.0.0-rc.1",
    "1.0.0",
  ];
  for (let i = 1; i < ordered.length; i++) {
    assert.equal(compareVersions(ordered[i - 1], ordered[i]), -1, `${ordered[i - 1]} < ${ordered[i]}`);
    assert.equal(compareVersions(ordered[i], ordered[i - 1]), 1, `${ordered[i]} > ${ordered[i - 1]}`);
  }
});
test("build metadata is not part of the order", () => {
  assert.equal(compareVersions("9.0.0+123", "9.0.0"), 0);
  assert.equal(compareVersions("9.0.0+build.1", "9.0.0+build.2"), 0);
  assert.equal(compareVersions("9.0.0-beta.1+x", "9.0.0-beta.1"), 0);
});
