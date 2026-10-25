import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { ModuleKind, ScriptTarget, transpileModule } from "typescript";
import { appendOutput, outputText, scanProgress } from "../../frontend/app/pages/system-maintenance/output.ts";

const source = readFileSync(new URL("../../frontend/app/pages/system-maintenance/state.svelte.ts", import.meta.url), "utf8");
// Exercise the real console batching, including replacements within one frame.
import { compileModule } from "svelte/compiler";
const transpile = (text, module = ModuleKind.ESNext) => transpileModule(text, {
  compilerOptions: { target: ScriptTarget.ESNext, module },
}).outputText;
const code = transpile(compileModule(transpile(source.replace(/^import[\s\S]*?;\r?\n/gm, "")) + "\nglobalThis.state = maintenanceState;", {
  filename: "maintenance-test.svelte.js", generate: "client",
}).js.code, ModuleKind.CommonJS);
const require = createRequire(import.meta.url);
const card = {
  id: "system-repair", title: "System File & Image Repair", admin: true, console: true,
  actions: [{ id: "dism", label: "Run DISM", confirm: null }],
};
function harness(events) {
  const context = vm.createContext({
    require, exports: {}, setTimeout, SvelteSet: Set, appendOutput,
    readJson: (_key, fallback) => fallback,
    toast: { error() {}, info() {}, success() {} },
    requestAnimationFrame: () => 1,
    maintenanceApi: { run: async (_id, _elevated, send) => {
      for (const data of events) send({ event: "line", data });
      return { result: "done", note: null };
    } },
  });
  vm.runInContext(code, context);
  return context.state;
}
const line = (text, replace = false) => ({ text, replace });
const bar = (percent) => `[==========================${percent}%===             ]`;

test("DISM newline progress stays on one row and keeps its final diagnosis", async () => {
  const state = harness([line("Deployment Image Servicing"), ...Array.from({ length: 261 }, () => line(bar(63.4))), line(bar(100)), line("The restore operation completed successfully.")]);
  await state.run(card, card.actions[0]);
  assert.deepEqual(Array.from(state.consoles[card.id].lines), ["Deployment Image Servicing", bar(100), "The restore operation completed successfully."]);
});

test("a progress line split across reads merges with the preceding newline update", async () => {
  const state = harness([line(bar(63.4)), line("[==========================64."), line(bar(64.1), true), line("Error: 0x800f081f"), line("The source files could not be found.")]);
  await state.run(card, card.actions[0]);
  assert.deepEqual(Array.from(state.consoles[card.id].lines), [bar(64.1), "Error: 0x800f081f", "The source files could not be found."]);
});

test("localized SFC percentages collapse while stage changes and repeated diagnostics remain", async () => {
  const state = harness([line("Επαλήθευση 20% ολοκληρώθηκε."), line("Επαλήθευση 60% ολοκληρώθηκε."), line("Επαλήθευση 100% ολοκληρώθηκε."), line("Stage 2: 10%"), line("Error"), line("Error")]);
  await state.run(card, card.actions[0]);
  assert.deepEqual(Array.from(state.consoles[card.id].lines), ["Επαλήθευση 100% ολοκληρώθηκε.", "Stage 2: 10%", "Error", "Error"]);
});

test("ordinary carriage-return replacements still replace their own row", async () => {
  const state = harness([line("Header"), line("Starting"), line("Finished", true), line("Summary")]);
  await state.run(card, card.actions[0]);
  assert.deepEqual(Array.from(state.consoles[card.id].lines), ["Header", "Finished", "Summary"]);
});

test("rendering and copying replace the terminal bar with readable progress", () => {
  assert.equal(outputText(["Header", bar(63.4), "Result"]), "Header\nProgress: 63.4%\nResult");
  assert.equal(scanProgress("Επαλήθευση 63,4% ολοκληρώθηκε.").percent, 63.4);
  assert.equal(scanProgress("Stage 1: 50%").signature, "Stage 1: %");
  assert.equal(scanProgress("Stage 2: 50%").signature, "Stage 2: %");
  assert.equal(scanProgress("Invalid 150%"), null);
  assert.equal(scanProgress("Invalid 1000%"), null);
  assert.equal(scanProgress("Invalid -5%"), null);
  assert.equal(scanProgress("Stage: 20%; Total: 10%"), null);
});
