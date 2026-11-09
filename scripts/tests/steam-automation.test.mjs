import assert from "node:assert/strict";
import test from "node:test";
import { waitingText, euros, validMinimum, untilText, farmPanel, plural, playtimeText, achievementsText, sortGames, searchGames, toggleInList, addAll, canQueueIdle, duplicateIds, listable, filterItems, sortItems, parseEuros, priceText, requestedPrice, runningSource, listEach, kindLabel, sellable, freshLowest } from "../../frontend/app/pages/steam-automation/model.ts";
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
test("playtime reads as hours", () => {
  assert.equal(playtimeText(90_840), "1,514h");
  assert.equal(playtimeText(2_520), "42h");
  assert.equal(playtimeText(144), "2.4h");
  assert.equal(playtimeText(1), "0.1h");
  assert.equal(playtimeText(0), "");
});
test("achievement progress reads as a count, or nothing without any", () => {
  assert.equal(achievementsText({ achievementsUnlocked: 37, achievementsTotal: 63 }), "37/63");
  assert.equal(achievementsText({ achievementsUnlocked: 0, achievementsTotal: 0 }), "");
});
const game = (appid, name, minutes, achievementsUnlocked, achievementsTotal, achievementsOk = true) => ({ appid, name, minutes, achievementsUnlocked, achievementsTotal, achievementsOk });
const library = [game(1, "Beta", 10, 1, 2), game(2, "alpha", 500, 0, 10), game(3, "Gamma", 0, 0, 0, false), game(4, "Delta", 10, 9, 9)];
test("games sort by playtime, most first, then by name", () => {
  assert.deepEqual(sortGames(library, "playtime").map((g) => g.appid), [2, 1, 4, 3]);
});
test("games sort by name, ignoring case", () => {
  assert.deepEqual(sortGames(library, "name").map((g) => g.appid), [2, 1, 4, 3]);
});
test("games sort by how complete their achievements are, none last", () => {
  assert.deepEqual(sortGames(library, "achievements").map((g) => g.appid), [4, 1, 2, 3]);
});
test("sorting leaves the original list alone", () => {
  const before = library.map((g) => g.appid);
  sortGames(library, "name");
  assert.deepEqual(library.map((g) => g.appid), before);
});
test("the search looks at names, ignoring case", () => {
  assert.deepEqual(searchGames(library, " ALP").map((g) => g.appid), [2]);
  assert.equal(searchGames(library, "").length, 4);
});
test("a list gains what it does not have and loses what it has", () => {
  assert.deepEqual(toggleInList([1, 2], 3), [1, 2, 3]);
  assert.deepEqual(toggleInList([1, 2], 1), [2]);
});
test("add all takes only games that can be unlocked and keeps the queue's order", () => {
  assert.deepEqual(addAll([4], library), [4, 1, 2]);
});
test("the idler's queue stops at thirty-two", () => {
  const full = Array.from({ length: 32 }, (_, i) => i);
  assert.equal(canQueueIdle(full, 99), false);
  assert.equal(canQueueIdle(full, 5), true, "one already there can still be taken off");
  assert.equal(canQueueIdle([], 99), true);
});
const item = (assetId, hashName, kind = "card", marketable = true, name = hashName, game = "G") => ({ assetId, hashName, kind, marketable, name, game, iconUrl: null, amount: 1 });
const items = [item("1", "A"), item("2", "A"), item("3", "A"), item("4", "B", "foil"), item("5", "C", "background"), item("6", "D", "emoticon"), item("7", "E", "booster"), item("8", "F", "card", false), item("9", "F", "card", false)];
const none = () => new Set();
const marks = (over = {}) => ({ locked: none(), onSale: none(), duplicates: duplicateIds(items, none()), ...over });
test("duplicates keep one of each card and count only sellable cards", () => {
  assert.deepEqual([...duplicateIds(items, none())], ["2", "3"]);
});
test("a copy already on sale can never be the one kept", () => {
  assert.deepEqual([...duplicateIds([item("1", "A"), item("2", "A")], new Set(["1"]))], []);
  assert.deepEqual([...duplicateIds([item("1", "A"), item("2", "A"), item("3", "A")], new Set(["2"]))], ["3"]);
});
test("only marketable items not locked or on sale can be listed", () => {
  const found = listable(items, new Set(["1"]), new Set(["2"])).map((i) => i.assetId);
  assert.deepEqual(found, ["3", "4", "5", "6", "7"]);
});
const ids = (list) => list.map((i) => i.assetId);
test("each filter shows what it says", () => {
  assert.deepEqual(ids(filterItems(items, "all", "", marks())).length, 9);
  assert.deepEqual(ids(filterItems(items, "card", "", marks())), ["1", "2", "3", "8", "9"]);
  assert.deepEqual(ids(filterItems(items, "foil", "", marks())), ["4"]);
  assert.deepEqual(ids(filterItems(items, "background", "", marks())), ["5"]);
  assert.deepEqual(ids(filterItems(items, "emoticon", "", marks())), ["6"]);
  assert.deepEqual(ids(filterItems(items, "booster", "", marks())), ["7"]);
  assert.deepEqual(ids(filterItems(items, "duplicates", "", marks())), ["2", "3"]);
  assert.deepEqual(ids(filterItems(items, "locked", "", marks({ locked: new Set(["5"]) }))), ["5"]);
  assert.deepEqual(ids(filterItems(items, "onSale", "", marks({ onSale: new Set(["7"]) }))), ["7"]);
});
test("the inventory search looks at the item and its game", () => {
  const list = [item("1", "x", "card", true, "Ciri", "The Witcher"), item("2", "y", "card", true, "Cole", "Beyond")];
  assert.deepEqual(ids(filterItems(list, "all", "witch", marks())), ["1"]);
  assert.deepEqual(ids(filterItems(list, "all", "COLE", marks())), ["2"]);
});
test("items sort by name, game or kind", () => {
  const list = [item("1", "x", "foil", true, "B", "Z"), item("2", "y", "card", true, "A", "Y")];
  assert.deepEqual(ids(sortItems(list, "name")), ["2", "1"]);
  assert.deepEqual(ids(sortItems(list, "game")), ["2", "1"]);
  assert.deepEqual(ids(sortItems(list, "kind")), ["2", "1"]);
});
test("a typed price reads in euros, with a comma or a point", () => {
  assert.equal(parseEuros("0,31"), 31);
  assert.equal(parseEuros("1.5"), 150);
  assert.equal(parseEuros("2"), 200);
  assert.equal(parseEuros(" 0,05€ "), 5);
  assert.equal(parseEuros("0"), null);
  assert.equal(parseEuros("abc"), null);
  assert.equal(parseEuros("1,234"), null);
  assert.equal(parseEuros(""), null);
});
test("a mistyped price is never mistaken for an empty field", () => {
  assert.equal(requestedPrice("0,31"), 31);
  assert.equal(requestedPrice(""), null);
  assert.equal(requestedPrice("   "), null);
  assert.equal(requestedPrice(undefined), null);
  assert.equal(requestedPrice("0,3x"), "invalid");
  assert.equal(requestedPrice("0"), "invalid");
});
test("a price shows without the euro sign in its field", () => {
  assert.equal(priceText(31), "0,31");
  assert.equal(priceText(200), "2,00");
});
test("what cannot be sold is not shown", () => {
  assert.deepEqual(ids(sellable(items)), ["1", "2", "3", "4", "5", "6", "7"]);
});
test("a looked-up price is not used once it is old", () => {
  assert.equal(freshLowest({ cents: 31, at: 1000 }, 1000 + 599)?.cents, 31);
  assert.equal(freshLowest({ cents: 31, at: 1000 }, 1000 + 600), null);
  assert.equal(freshLowest(undefined, 1000), null);
});
test("each kind of item has a plain name", () => {
  assert.equal(kindLabel("card"), "Trading card");
  assert.equal(kindLabel("foil"), "Foil card");
  assert.equal(kindLabel("booster"), "Booster pack");
  assert.equal(kindLabel("other"), "Item");
});
const status = (over = {}) => ({ manual: [], started: { farm: false, idle: false, achievements: false }, farm: { phase: "done" }, ...over });
test("a running game says what started it", () => {
  assert.equal(runningSource(7, status({ manual: [7] }), { idleGames: [] }), "hand");
  assert.equal(runningSource(7, status({ started: { farm: false, idle: true, achievements: false } }), { idleGames: [7] }), "idler");
  assert.equal(runningSource(7, status({ started: { farm: true, idle: false, achievements: false }, farm: { phase: "solo" } }), { idleGames: [] }), "farming");
});
test("a game both queued and being farmed counts as farming until the cards are done", () => {
  assert.equal(runningSource(7, status({ started: { farm: true, idle: true, achievements: false }, farm: { phase: "solo" } }), { idleGames: [7] }), "farming");
});
test("a bulk run goes one at a time and adds up what happened", async () => {
  const seen = [];
  const steps = [];
  const answers = { a: { listed: true, waitingConfirmation: true }, b: { listed: false, waitingConfirmation: false }, c: { listed: true, waitingConfirmation: false } };
  const result = await listEach(["a", "b", "c"], async (id) => (seen.push(id), answers[id]), (step) => steps.push(step.done), () => false);
  assert.deepEqual(seen, ["a", "b", "c"]);
  assert.deepEqual(steps, [1, 2, 3]);
  assert.deepEqual(result, { done: 3, listed: 2, waiting: 1, skipped: 1, error: null, stopped: false });
});
test("a bulk run can be stopped between items", async () => {
  const seen = [];
  let stop = false;
  const result = await listEach(["a", "b"], async (id) => { seen.push(id); stop = true; return { listed: true, waitingConfirmation: false }; }, () => {}, () => stop);
  assert.deepEqual(seen, ["a"]);
  assert.equal(result.stopped, true);
});
test("a bulk run ends at the first failure and says why", async () => {
  const seen = [];
  const result = await listEach(["a", "b"], async (id) => { seen.push(id); throw "Your Steam sign-in has expired. Sign in again."; }, () => {}, () => false);
  assert.deepEqual(seen, ["a"]);
  assert.equal(result.error, "Your Steam sign-in has expired. Sign in again.");
});
test("times ahead read simply", () => {
  assert.equal(untilText(1000 + 24 * 60, 1000), "in 24 min");
  assert.equal(untilText(1000 + 3 * 3600, 1000), "in 3 h");
  assert.equal(untilText(1000, 1000), "soon");
  assert.equal(untilText(null, 1000), "");
});
const farm = (phase, gamesLeft = 2, dropsLeft = 6) => ({ phase, gamesLeft, dropsLeft });
test("before starting, card farming says what would happen", () => {
  assert.equal(farmPanel(false, farm("done"), 6, 2, null).title, "6 cards to collect from 2 games");
  assert.equal(farmPanel(false, farm("done"), 1, 1, null).title, "1 card to collect from 1 game");
  assert.equal(farmPanel(false, farm("done"), 0, 0, null).title, "No cards left to collect");
});
test("while running, card farming says what it is doing now", () => {
  assert.equal(farmPanel(true, farm("reading"), 6, 2, null).title, "Checking your games for cards");
  assert.equal(farmPanel(true, farm("signedOut"), 6, 2, null).title, "Waiting for you to sign in to Steam");
  assert.equal(farmPanel(true, farm("building"), 6, 2, null).title, "Warming up 2 games");
  assert.equal(farmPanel(true, farm("solo", 1, 5), 5, 1, "Detroit").title, "Farming Detroit");
  assert.equal(farmPanel(true, farm("solo", 1, 5), 5, 1, "Detroit").detail, "5 cards left to collect.");
  assert.equal(farmPanel(true, farm("done", 0, 0), 0, 0, null).title, "All your cards have dropped");
});
