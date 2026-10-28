import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { transpileModule, ScriptTarget, ModuleKind } from "typescript";
const hooks = registerHooks({
  load(url, context, next) {
    if (!url.endsWith("/health-model.ts")) return next(url, context);
    return { format: "module", shortCircuit: true, source: transpileModule(readFileSync(fileURLToPath(url), "utf8"), {
      compilerOptions: { target: ScriptTarget.ESNext, module: ModuleKind.ESNext },
    }).outputText };
  },
});
const { filterFindings, cleanupPlan, compatibleActions, selectResults } = await import("../../frontend/app/pages/system-cleaner/health-model.ts");
hooks.deregister();
const finding = (id, module, actions, extra = {}) => ({ id, module, title: id, detail: "", path: null, size: null, enabled: null, confidence: null, flags: [], actions, cleanupAction: null, recommended: false, reviewReason: null, ...extra });
const rows = [
  finding("cache", "redundant", ["recycle"], { cleanupAction: "recycle", recommended: true, size: 200 }),
  finding("registry", "broken", ["deleteKey", "forceClean"], { cleanupAction: "deleteKey", path: "HKCU\\Example", size: 0 }),
  finding("folder", "leftovers", ["recycle"], { cleanupAction: "recycle", size: 100 }),
  finding("startup", "hibernation", ["disable", "enable"]),
  finding("service", "hibernation", ["manual"]),
  finding("info", "hibernation", []),
];
test("mixed cleanup retains each item's supported action without deleting startup controls", () => {
  assert.deepEqual(cleanupPlan(rows, new Set(rows.map(r => r.id)), {}).map(r => [r.id, r.action]), [["cache", "recycle"], ["registry", "deleteKey"], ["folder", "recycle"]]);
  assert.equal(cleanupPlan(rows, new Set(["registry"]), { registry: "forceClean" })[0].action, "forceClean");
  assert.equal(cleanupPlan(rows, new Set(["registry"]), { registry: "disable" })[0].action, "deleteKey");
});
test("filtering and select all retain choices outside the visible results", () => {
  const visible = filterFindings(rows, { category: "cleanup", query: "hkcu", filter: "all", sort: "name", failures: {} });
  assert.deepEqual(visible.map(r => r.id), ["registry"]);
  assert.deepEqual([...selectResults(new Set(["cache"]), visible)], ["cache", "registry"]);
  assert.equal(selectResults(new Set(), rows).has("info"), false);
});
test("recommended/review/failed and size sort match the list being shown", () => {
  const options = { category: "cleanup", query: "", filter: "recommended", sort: "size", failures: { folder: "locked" } };
  assert.deepEqual(filterFindings(rows, options).map(r => r.id), ["cache"]);
  assert.deepEqual(filterFindings(rows, { ...options, filter: "failed" }).map(r => r.id), ["folder"]);
  assert.deepEqual(filterFindings(rows, { ...options, filter: "review" }).map(r => r.id), ["folder", "registry"]);
});
test("mixed controls show compatible action counts instead of hiding all buttons", () => {
  assert.deepEqual(compatibleActions(rows.slice(3)), [{ action: "disable", count: 1 }, { action: "enable", count: 1 }, { action: "manual", count: 1 }]);
});
