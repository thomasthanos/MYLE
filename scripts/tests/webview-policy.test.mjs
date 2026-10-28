import assert from "node:assert/strict";
import test from "node:test";
import { blockedWebviewShortcut } from "../../frontend/lib/webview-policy.ts";
const key = (value, options = {}) => ({ key: value, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...options });
test("release blocks DevTools shortcuts and browser actions", () => {
  for (const shortcut of [key("F12"), key("F5"), key("F7"), key("r", { ctrlKey: true }), key("u", { ctrlKey: true }), ...["I", "J", "C", "K"].map(value => key(value, { ctrlKey: true, shiftKey: true }))]) {
    assert.equal(blockedWebviewShortcut(shortcut, false), true, JSON.stringify(shortcut));
  }
});
test("ordinary editing, app navigation and the custom keyboard menu stay available", () => {
  for (const value of ["c", "v", "x", "a", "z", "y", "b"]) assert.equal(blockedWebviewShortcut(key(value, { ctrlKey: true }), false), false);
  assert.equal(blockedWebviewShortcut(key("F10", { shiftKey: true }), false), false);
  assert.equal(blockedWebviewShortcut(key("ContextMenu"), false), false);
});
test("development builds retain DevTools and reload shortcuts", () => {
  assert.equal(blockedWebviewShortcut(key("F12"), true), false);
  assert.equal(blockedWebviewShortcut(key("I", { ctrlKey: true, shiftKey: true }), true), false);
  assert.equal(blockedWebviewShortcut(key("F5"), true), false);
});
