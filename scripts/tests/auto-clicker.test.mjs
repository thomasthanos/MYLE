import assert from "node:assert/strict";
import test from "node:test";
import { newProfile, newStep, intervalInMs, validateConfig, progressText, progressShare, moveStep } from "../../frontend/app/pages/auto-clicker/model.ts";
test("seconds convert exactly and fractional milliseconds are rejected", () => {
  assert.equal(intervalInMs(0.25, "seconds"), 250);
  assert.equal(intervalInMs(125, "ms"), 125);
  assert.equal(intervalInMs(0.0001, "seconds"), null);
  assert.equal(intervalInMs(NaN, "ms"), null);
});
test("sequence validation catches incomplete points, counts and intervals", () => {
  const config = newProfile().config;
  config.mode = "sequence";
  config.steps = [];
  assert.ok(validateConfig(config));
  config.steps = [newStep()];
  config.steps[0].x = -1500;
  config.steps[0].y = -250;
  assert.equal(validateConfig(config), null);
  config.steps[0].clicks = 0;
  assert.ok(validateConfig(config));
  config.steps[0].clicks = 2;
  config.steps[0].intervalMs = 0;
  assert.ok(validateConfig(config));
});
test("new profiles and steps do not share their mutable configuration", () => {
  const first = newProfile(), second = newProfile();
  assert.notEqual(first.id, second.id);
  first.config.steps[0].x = 777;
  assert.equal(second.config.steps[0].x, 0);
  assert.notEqual(newStep().id, newStep().id);
});
test("reordering steps preserves values and refuses out-of-bounds moves", () => {
  const a = newStep(), b = newStep(), c = newStep();
  assert.deepEqual(moveStep([a, b, c], 1, -1), [b, a, c]);
  assert.deepEqual(moveStep([a, b, c], 0, -1), [a, b, c]);
});
const running = { revision: 12, runId: 4, phase: "running", mode: "sequence", loop: 2, loops: 3, step: 1, steps: 2, click: 22, clicks: 50, completed: 122, total: 300, countdown: 0, hotkey: true, hotkeyError: null, error: null };
test("progress reports the current step and a finite overall fraction", () => {
  assert.equal(progressText(running), "Step 1 of 2: Click 22/50");
  assert.equal(progressShare(running), 122 / 300);
  assert.equal(progressShare({ ...running, total: null }), null);
  assert.equal(progressText({ ...running, phase: "countdown", countdown: 3 }), "Starting in 3…");
});
