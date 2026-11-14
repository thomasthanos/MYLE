import assert from "node:assert/strict";
import test from "node:test";
import { autoUpdateCards } from "../../frontend/app/pages/install-apps/updates.ts";
const update = (id, name, available = "2.0.0", version = "1.0.0") => ({ id, name, version, available });
test("an update MYLE does not list becomes a card", () => {
  const cards = autoUpdateCards([update("GitHub.cli", "GitHub CLI")], ["Spotify.Spotify"]);
  assert.deepEqual(cards, [{ id: "GitHub.cli", name: "GitHub CLI" }]);
});
test("apps MYLE already lists keep their own card", () => {
  const cards = autoUpdateCards(
    [update("Spotify.Spotify", "Spotify"), update("Discord.Discord", "Discord")],
    ["spotify.spotify", "Discord.Discord"],
  );
  assert.deepEqual(cards, []);
});
test("each package id is carded once, whatever its casing", () => {
  const cards = autoUpdateCards(
    [
      update("Microsoft.DotNet.Runtime.8", "Microsoft .NET Runtime 8"),
      update("microsoft.dotnet.runtime.8", "Microsoft .NET Runtime 8"),
    ],
    [],
  );
  assert.equal(cards.length, 1);
  assert.equal(cards[0].id, "Microsoft.DotNet.Runtime.8");
});
test("the search box filters update cards like the other cards", () => {
  const updates = [
    update("GitHub.cli", "GitHub CLI"),
    update("Microsoft.DotNet.Runtime.8", "Microsoft .NET Runtime 8"),
    update("Spotify.Spotify", "Spotify"),
  ];
  assert.deepEqual(autoUpdateCards(updates, [], "cli").map((card) => card.id), ["GitHub.cli"]);
  assert.deepEqual(autoUpdateCards(updates, [], "SPOTIFY").map((card) => card.id), ["Spotify.Spotify"]);
  assert.deepEqual(autoUpdateCards(updates, [], "dotnet").map((card) => card.id), ["Microsoft.DotNet.Runtime.8"]);
  assert.equal(autoUpdateCards(updates, [], "").length, 3);
  assert.equal(autoUpdateCards(updates, [], "nothing").length, 0);
});
test("twenty updates with two known apps leave eighteen automatic cards", () => {
  const updates = Array.from({ length: 20 }, (_, index) => update(`Vendor.App${index}`, `App ${index}`));
  updates[0] = update("Spotify.Spotify", "Spotify");
  updates[1] = update("Discord.Discord", "Discord");
  const cards = autoUpdateCards(updates, ["Spotify.Spotify", "Discord.Discord"]);
  assert.equal(cards.length, 18);
  assert.ok(cards.every((card) => card.id.startsWith("Vendor.")));
});
