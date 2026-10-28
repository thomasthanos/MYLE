import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { isSidebarToggle, nextFocus, pageForShortcut, sections, shortcutOf, tooltipPosition } from "../../frontend/lib/sidebar.ts";
// The registry's order and groups (frontend/app/pages/registry.ts).
const registry = readFileSync(new URL("../../frontend/app/pages/registry.ts", import.meta.url), "utf8");
const all = [...registry.matchAll(/\{ id: "([a-z-]+)",\s+label: "[^"]+",\s+group: "([a-z]+)",([^\n]*)/g)].map(
  ([, id, group, rest]) => ({ id, group, ownerOnly: rest.includes("ownerOnly: true"), numbered: !rest.includes("numbered: false"), lazy: false }),
);
for (const page of all) {
  const entry = registry.slice(registry.indexOf(`id: "${page.id}"`)).split(/\n {2}\{ id:|\n\] as const/)[0];
  page.lazy = entry.includes("load: () => import(");
}
const everyone = all.filter((page) => !page.ownerOnly);
const keys = (key, extra = {}) => ({ key, ctrlKey: true, shiftKey: false, altKey: false, metaKey: false, ...extra });
test("the registry lists every page once, grouped", () => {
  assert.equal(all.length, 13);
  assert.equal(new Set(all.map((page) => page.id)).size, all.length);
  assert.equal(all[0].id, "install-apps", "the page the app opens on comes first");
  assert.deepEqual(
    sections(all).map((section) => section.group),
    ["apps", "data", "system", "owner"],
  );
  assert.deepEqual(all.filter((page) => page.group === "bottom").map((page) => page.id), ["settings"]);
});
test("Auto Clicker is available to everyone without changing existing numbered shortcuts", () => {
  assert.ok(everyone.some(page => page.id === "auto-clicker"));
  assert.equal(shortcutOf(everyone, "auto-clicker"), null);
  assert.equal(shortcutOf(all, "project-backups"), "Ctrl+0");
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
test("numbered shortcuts follow the sidebar order and Ctrl+comma stays available", () => {
  assert.equal(shortcutOf(everyone, "install-apps"), "Ctrl+1");
  assert.equal(pageForShortcut(everyone, keys("1")).id, "install-apps");
  assert.equal(pageForShortcut(everyone, keys(",")).id, "settings");
  // The same numbers for everyone: the owner's pages come after.
  for (const page of everyone) {
    if (page.group !== "bottom") assert.equal(shortcutOf(all, page.id), shortcutOf(everyone, page.id));
  }
  // Nine pages for everyone since Disk Analyzer: Ctrl+9 is the last of them.
  assert.equal(pageForShortcut(everyone, keys("9")).id, "system-maintenance");
  assert.equal(shortcutOf(everyone, "disk-analyzer"), "Ctrl+8");
  assert.equal(pageForShortcut(everyone, keys("0")), undefined, "only nine pages for everyone");
  assert.equal(pageForShortcut(all, keys("9")).id, "system-maintenance");
  // Not a shortcut.
  assert.equal(pageForShortcut(all, keys("1", { shiftKey: true })), undefined);
  assert.equal(pageForShortcut(all, keys("1", { ctrlKey: false })), undefined);
  assert.equal(pageForShortcut(all, keys("b")), undefined);
});
test("Ctrl+period opens Settings, including when matching its physical key", () => {
  assert.equal(pageForShortcut(everyone, keys("."))?.id, "settings");
  assert.equal(pageForShortcut(everyone, keys(".", { code: "Period" }))?.id, "settings");
  assert.equal(pageForShortcut(everyone, keys(",", { code: "Comma" }))?.id, "settings");
  assert.equal(shortcutOf(everyone, "settings"), "Ctrl+.");
});
test("Ctrl+0 opens Project Backups only when the owner page is available, and GitHub Releases has no number", () => {
  assert.equal(pageForShortcut(all, keys("0", { code: "Digit0" }))?.id, "project-backups");
  assert.equal(shortcutOf(all, "project-backups"), "Ctrl+0");
  assert.equal(pageForShortcut(everyone, keys("0", { code: "Digit0" })), undefined);
  assert.equal(shortcutOf(everyone, "project-backups"), null);
  // Ten numbers, eleven pages before Settings: the last one goes without.
  assert.equal(shortcutOf(all, "github-releases"), null);
});
test("sidebar toggle accepts the B key in Latin and Greek keyboard layouts", () => {
  for (const [key, code] of [["b", "KeyB"], ["B", "KeyB"], ["β", "KeyB"], ["Β", "KeyB"]]) {
    assert.equal(isSidebarToggle(keys(key, { code })), true, `${key} / ${code}`);
  }
  assert.equal(isSidebarToggle(keys("b")), true, "fallback when key codes are unavailable");
  assert.equal(isSidebarToggle(keys("β", { code: "KeyV" })), false);
  for (const modifier of ["shiftKey", "altKey", "metaKey"]) {
    assert.equal(isSidebarToggle(keys("b", { code: "KeyB", [modifier]: true })), false);
    assert.equal(pageForShortcut(all, keys(".", { code: "Period", [modifier]: true })), undefined);
    assert.equal(pageForShortcut(all, keys("0", { code: "Digit0", [modifier]: true })), undefined);
  }
  assert.equal(isSidebarToggle(keys("b", { code: "KeyB", ctrlKey: false })), false);
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
test("sidebar tooltip leaves space for its shadow at the bottom and points into the button", () => {
  const position = tooltipPosition(673, 76, 720);
  assert.ok(position, "the tooltip must use a viewport-aware position");
  assert.ok(position.top + 76 <= 688, "32px remain for the shadow and window edge");
  const arrowY = position.top + position.arrow;
  assert.ok(arrowY >= 653 && arrowY <= 693, "the arrow still touches the 40px button");
  assert.ok(position.arrow >= 18 && position.arrow <= 58, "the arrow avoids the rounded corners");
});
test("tooltip placement adapts to taller content and keeps middle tooltips centred", () => {
  const middle = tooltipPosition(300, 100, 720);
  assert.deepEqual(middle, { top: 250, arrow: 50 });
  const tall = tooltipPosition(660, 164, 720);
  assert.ok(tall && tall.top + 164 <= 688);
  assert.equal(tall.top + tall.arrow, 660);
  const nearTop = tooltipPosition(50, 104, 720);
  assert.ok(nearTop && nearTop.top >= 32);
});
