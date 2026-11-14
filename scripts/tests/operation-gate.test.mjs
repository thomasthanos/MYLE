// The cross-page operation gate: `node --test scripts/tests/`.
import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
import { compileModule } from "svelte/compiler";
const hooks = registerHooks({
  load(url, context, next) {
    if (!url.endsWith(".svelte.ts")) return next(url, context);
    const source = transpileModule(readFileSync(fileURLToPath(url), "utf8"), { compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext } }).outputText;
    return { format: "module", source: compileModule(source, { filename: fileURLToPath(url), generate: "client" }).js.code, shortCircuit: true };
  },
});
const { operationGate: gate } = await import("../../frontend/lib/operation-gate.svelte.ts");
hooks.deregister();
const W = "windows-optimization";

test("one workflow ending does not free the gate under another of the same page", () => {
  assert.equal(gate.begin(W, "debloat"), true);
  // The page reloads and sees no Auto-Logon or firmware job of its own: it
  // drops only its own hold, the debloat run keeps the gate.
  gate.end(W, "backend");
  assert.equal(gate.owner, W);
  assert.equal(gate.lockedFor("install-apps"), true);
  assert.equal(gate.begin("install-apps"), false, "another page cannot start meanwhile");
  assert.equal(gate.begin(W, "autologon"), true);
  gate.end(W, "debloat");
  assert.equal(gate.owner, W, "still held by Auto-Logon");
  gate.end(W, "autologon");
  assert.equal(gate.owner, null);
  assert.equal(gate.lockedFor("install-apps"), false);
});

test("holding twice under one key needs one release, and a stranger's release is ignored", () => {
  assert.equal(gate.begin("spotify-hub", "backend"), true);
  assert.equal(gate.begin("spotify-hub", "backend"), true);
  gate.end(W, "backend");
  assert.equal(gate.owner, "spotify-hub");
  gate.end("spotify-hub", "backend");
  assert.equal(gate.owner, null);
  // The default key, as Install Apps uses it.
  assert.equal(gate.begin("install-apps"), true);
  gate.end("install-apps");
  assert.equal(gate.owner, null);
});

test("a tab sees another workflow of its own page", () => {
  gate.begin(W, "debloat");
  assert.equal(gate.heldBesides(W, ["autologon", "firmware", "backend"]), true, "the Tools tab waits for the debloat run");
  assert.equal(gate.heldBesides(W, ["debloat", "start-menu"]), false);
  gate.end(W, "debloat");
  gate.begin(W, "backend");
  assert.equal(gate.heldBesides(W, ["debloat", "start-menu"]), true, "debloat waits for a running Auto-Logon change");
  gate.end(W, "backend");
  assert.equal(gate.heldBesides(W, []), false);
});
