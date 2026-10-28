import assert from "node:assert/strict";
import test from "node:test";
import { textActions, menuPosition } from "../../frontend/lib/context-menu.ts";
const field = { editable: true, disabled: false, readOnly: false, sensitive: false, noCopy: false, selectedText: "selected", hasText: true };
test("normal fields allow editing, while an empty selection cannot be cut or copied", () => {
  assert.deepEqual(textActions(field), { copy: true, cut: true, paste: true, selectAll: true });
  assert.deepEqual(textActions({ ...field, selectedText: "", hasText: false }), { copy: false, cut: false, paste: true, selectAll: false });
});
test("readonly and disabled fields cannot be changed", () => {
  assert.deepEqual(textActions({ ...field, readOnly: true }), { copy: true, cut: false, paste: false, selectAll: true });
  assert.deepEqual(textActions({ ...field, disabled: true }), { copy: false, cut: false, paste: false, selectAll: false });
});
test("secrets keep their dedicated clipboard flow and informational boxes are not copyable", () => {
  assert.deepEqual(textActions({ ...field, sensitive: true }), { copy: false, cut: false, paste: true, selectAll: true });
  assert.deepEqual(textActions({ ...field, editable: false, noCopy: true }), { copy: false, cut: false, paste: false, selectAll: false });
  assert.deepEqual(textActions({ ...field, editable: false }), { copy: true, cut: false, paste: false, selectAll: false });
});
test("menus opened at the right and bottom edges remain inside the window", () => {
  assert.deepEqual(menuPosition(990, 690, 244, 280, 1000, 700), { left: 748, top: 412 });
  assert.deepEqual(menuPosition(-10, -5, 244, 100, 1000, 700), { left: 8, top: 8 });
  assert.deepEqual(menuPosition(200, 150, 244, 100, 1000, 700), { left: 200, top: 150 });
});
