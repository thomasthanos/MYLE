import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_FILTERS, validFilters, timeLeft, isLive, dealOver, reviewLabel, reviewTone, appendUnique, updatedAgo, createLatest } from "../../frontend/app/pages/steam-helper/model.ts";
test("online co-op starts on, the rest off", () => assert.deepEqual(DEFAULT_FILTERS, { onlineCoop: true, singlePlayer: false, storyOnly: false }));
test("a saved filter must have the right shape", () => {
  assert.ok(validFilters(DEFAULT_FILTERS));
  assert.ok(!validFilters({ onlineCoop: "yes", singlePlayer: false, storyOnly: false }));
  assert.ok(!validFilters({ onlineCoop: true }));
  assert.ok(!validFilters(null));
});
test("time left reads short", () => {
  assert.equal(timeLeft(1000 + 3600 * 23 + 30, 1000), "23h");
  assert.equal(timeLeft(1000 + 86400 * 5 + 60, 1000), "5d");
  assert.equal(timeLeft(1000 + 1800, 1000), "30m");
  assert.equal(timeLeft(1000 + 20, 1000), "1m", "the last minute still shows");
  assert.equal(timeLeft(null, 1000), null);
});
test("a deal that has ended shows no time and counts as over", () => {
  assert.equal(timeLeft(999, 1000), null);
  assert.equal(dealOver(90, 999, 1000), true);
  assert.equal(dealOver(90, 1000, 1000), false);
  assert.equal(dealOver(90, null, 1000), false);
  assert.equal(dealOver(0, 999, 1000), false);
});
test("an offer past its end is no longer live", () => {
  assert.equal(isLive(999, 1000), false);
  assert.equal(isLive(1000, 1000), true);
  assert.equal(isLive(null, 1000), true);
});
test("a game with no reviews is not called bad", () => {
  assert.equal(reviewLabel(0, 0), "No reviews yet");
  assert.equal(reviewTone(0, 0), "none");
  assert.equal(reviewLabel(95, 10), "95% positive");
  assert.equal(reviewTone(95, 10), "good");
  assert.equal(reviewTone(75, 10), "good");
  assert.equal(reviewTone(74, 10), "mid");
  assert.equal(reviewTone(46, 10), "mid");
});
test("See all never repeats a game", () => {
  assert.deepEqual(appendUnique([{ appid: 1 }], [{ appid: 1 }, { appid: 2 }, { appid: 2 }]).map((g) => g.appid), [1, 2]);
});
test("updated time", () => {
  assert.equal(updatedAgo(880, 1000), "Updated 2 min ago");
  assert.equal(updatedAgo(990, 1000), "Updated just now");
  assert.equal(updatedAgo(1000 - 3 * 3600, 1000), "Updated 3 h ago");
  assert.equal(updatedAgo(null, 1000), "");
});
test("only the latest request counts", () => {
  const latest = createLatest();
  const first = latest.next();
  const second = latest.next();
  assert.equal(latest.isCurrent(first), false);
  assert.equal(latest.isCurrent(second), true);
});
