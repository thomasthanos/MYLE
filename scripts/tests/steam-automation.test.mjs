import assert from "node:assert/strict";
import test from "node:test";
import { waitingText, euros, validMinimum, pickable, untilText, farmLine, plural } from "../../frontend/app/pages/steam-automation/model.ts";
test("the banner says why things are waiting", () => {
  assert.equal(waitingText({ kind: "realGame", appid: 9 }), "Paused while you play. Everything picks up again when you close your game.");
  assert.equal(waitingText({ kind: "noSteam", appid: null }), "Steam is not running.");
  assert.equal(waitingText({ kind: "otherAccount", appid: null }), "Steam and Steam Community are signed in to different accounts, so nothing is running.");
  assert.equal(waitingText({ kind: "paused", appid: null }), "Paused. Nothing runs until you resume.");
  assert.equal(waitingText(null), "");
});
test("counts read properly", () => {
  assert.equal(plural(1, "game"), "1 game");
  assert.equal(plural(2, "game"), "2 games");
  assert.equal(plural(0, "game"), "0 games");
});
test("cents read as euros", () => {
  assert.equal(euros(184), "1,84€");
  assert.equal(euros(5), "0,05€");
  assert.equal(euros(100), "1,00€");
});
test("a minimum price must be sensible", () => {
  assert.equal(validMinimum(5), true);
  assert.equal(validMinimum(0), false);
  assert.equal(validMinimum(1.5), false);
  assert.equal(validMinimum(NaN), false);
});
const games = [{ appid: 1, name: "One", achievementsOk: true }, { appid: 2, name: "Two", achievementsOk: false }];
test("the achievements picker hides games that cannot be chosen", () => {
  assert.deepEqual(pickable(games, "achievements", "").map((g) => g.appid), [1]);
});
test("the idler can pick any game", () => {
  assert.deepEqual(pickable(games, "idle", "").map((g) => g.appid), [1, 2]);
});
test("the picker searches by name, ignoring case", () => {
  assert.deepEqual(pickable(games, "idle", "tw").map((g) => g.appid), [2]);
});
test("times ahead read simply", () => {
  assert.equal(untilText(1000 + 24 * 60, 1000), "in 24 min");
  assert.equal(untilText(1000 + 3 * 3600, 1000), "in 3 h");
  assert.equal(untilText(1000, 1000), "soon");
  assert.equal(untilText(null, 1000), "");
});
test("card farming says what it is doing", () => {
  assert.equal(farmLine({ phase: "building", gamesLeft: 3, dropsLeft: 9 }), "9 cards left to drop, across 3 games");
  assert.equal(farmLine({ phase: "solo", gamesLeft: 1, dropsLeft: 1 }), "1 card left to drop, in 1 game");
  assert.equal(farmLine({ phase: "done", gamesLeft: 0, dropsLeft: 0 }), "No cards left to drop");
});
