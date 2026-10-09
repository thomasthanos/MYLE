import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { nextFocus, pageForShortcut, sections, shortcutOf } from "../../frontend/lib/sidebar.ts";

// The registry's order and groups (frontend/app/pages/registry.ts).
const registry = readFileSync(new URL("../../frontend/app/pages/registry.ts", import.meta.url), "utf8");
const all = [...registry.matchAll(/\{ id: "([a-z-]+)",\s+label: "[^"]+",\s+group: "([a-z]+)",([^\n]*)/g)].map(
  ([, id, group, rest]) => ({ id, group, ownerOnly: rest.includes("ownerOnly: true"), lazy: false }),
);
for (const page of all) {
  const entry = registry.slice(registry.indexOf(`id: "${page.id}"`)).split(/\n  \{ id:|\n\] as const/)[0];
  page.lazy = entry.includes("load: () => import(");
}
const everyone = all.filter((page) => !page.ownerOnly);
const keys = (key, extra = {}) => ({ key, ctrlKey: true, shiftKey: false, altKey: false, metaKey: false, ...extra });

test("the registry lists every page once, grouped", () => {
  assert.equal(all.length, 11);
  assert.equal(new Set(all.map((page) => page.id)).size, all.length);
  assert.equal(all[0].id, "install-apps", "the page the app opens on comes first");
  assert.deepEqual(
    sections(all).map((section) => section.group),
    ["apps", "data", "system", "owner"],
  );
  assert.deepEqual(all.filter((page) => page.group === "bottom").map((page) => page.id), ["settings"]);
});

test("the owner-only pages are their own group, loaded only when opened", () => {
  const owner = all.filter((page) => page.ownerOnly);
  assert.deepEqual(owner.map((page) => page.id).sort(), ["github-releases", "project-backups"]);
  for (const page of owner) {
    assert.equal(page.group, "owner", page.id);
    assert.ok(page.lazy, `${page.id} is not loaded lazily`);
  }
  assert.ok(all.filter((page) => page.group === "owner").every((page) => page.ownerOnly));
  // Nothing imports them eagerly.
  assert.ok(!/^import (GithubReleases|ProjectBackups) from/m.test(registry));
  // Everyone else never sees the group.
  assert.ok(!sections(everyone).some((section) => section.group === "owner"));
});

test("Ctrl+1 to Ctrl+9 follow the order shown, Ctrl+, opens Settings", () => {
  assert.equal(shortcutOf(everyone, "install-apps"), "Ctrl+1");
  assert.equal(shortcutOf(everyone, "settings"), "Ctrl+,");
  assert.equal(pageForShortcut(everyone, keys("1")).id, "install-apps");
  assert.equal(pageForShortcut(everyone, keys(",")).id, "settings");
  // The same numbers for everyone: the owner's pages come after.
  for (const page of everyone) {
    if (page.group !== "bottom") assert.equal(shortcutOf(all, page.id), shortcutOf(everyone, page.id));
  }
  assert.equal(pageForShortcut(everyone, keys("9")), undefined, "only eight pages for everyone");
  assert.equal(pageForShortcut(all, keys("9")).id, "project-backups");
  assert.equal(shortcutOf(all, "github-releases"), null, "a tenth page has no number");
  // Not a shortcut.
  assert.equal(pageForShortcut(all, keys("1", { shiftKey: true })), undefined);
  assert.equal(pageForShortcut(all, keys("1", { ctrlKey: false })), undefined);
  assert.equal(pageForShortcut(all, keys("0")), undefined);
  assert.equal(pageForShortcut(all, keys("b")), undefined);
});

test("arrow keys move through the sidebar and wrap", () => {
  assert.equal(nextFocus(0, 5, "ArrowDown"), 1);
  assert.equal(nextFocus(4, 5, "ArrowDown"), 0);
  assert.equal(nextFocus(0, 5, "ArrowUp"), 4);
  assert.equal(nextFocus(-1, 5, "ArrowDown"), 0);
  assert.equal(nextFocus(-1, 5, "ArrowUp"), 4);
  assert.equal(nextFocus(2, 5, "Home"), 0);
  assert.equal(nextFocus(2, 5, "End"), 4);
  assert.equal(nextFocus(2, 5, "Enter"), null);
  assert.equal(nextFocus(0, 0, "ArrowDown"), null);
});
