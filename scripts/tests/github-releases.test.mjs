import assert from "node:assert/strict";
import test from "node:test";
import { stageIsStep, stepReached } from "../../frontend/app/pages/github-releases/pipeline.ts";

test("a build stage is a pipeline step by its words, with a version after it but not more words", () => {
  assert.ok(stageIsStep("App 9.7.2", "App"));
  assert.ok(stageIsStep("Setup window", "Setup window"));
  assert.ok(stageIsStep("setup", "Setup"));
  assert.ok(!stageIsStep("Setup window", "Setup"));
  assert.ok(!stageIsStep("Compiling Rust: serde (3 crates)", "App"));
  assert.ok(!stageIsStep("App", ""));
});

test("the pipeline follows MYLE's build-setup.ps1 markers in order, never back", () => {
  const titles = ["App", "Setup window", "Uninstaller", "Payload", "Setup"];
  assert.equal(stepReached([], titles), -1);
  assert.equal(stepReached(["App 9.8.0", "Compiling Rust: myle (412 crates)"], titles), 0);
  assert.equal(stepReached(["App 9.8.0", "Setup window", "Bundling with Vite", "Uninstaller"], titles), 2);
  // "Setup window" must not count as the last step "Setup".
  assert.equal(stepReached(["App 9.8.0", "Setup window"], titles), 1);
  assert.equal(stepReached(["App 9.8.0", "Setup window", "Uninstaller", "Payload", "Setup", "Done: MYLE.exe (18 MB)"], titles), 4);
  assert.equal(stepReached(["Uninstaller", "App 9.8.0"], titles), 2);
});
