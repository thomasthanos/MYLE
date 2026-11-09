<script lang="ts">
  import { onMount } from "svelte";
  import Ban from "@lucide/svelte/icons/ban";
  import Check from "@lucide/svelte/icons/check";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import type { BadgeGame, LibraryGame } from "../api";
  import GameGrid from "../GameGrid.svelte";
  import GameTile from "../GameTile.svelte";
  import { farmLine, plural, searchGames, sortGames, untilText } from "../model";
  import StartButton from "../StartButton.svelte";
  import { steamAutomationState as page, type CardSort } from "../state.svelte";

  type CardGame = LibraryGame & { drops?: number };
  const SORTS: { value: CardSort; label: string }[] = [
    { value: "drops", label: "Drops remaining" },
    { value: "name", label: "Name (A to Z)" },
    { value: "playtime", label: "Playtime (high to low)" },
  ];

  const find = page.find.cards;
  const blank = (appid: number, name: string): LibraryGame => ({ appid, name, coverUrl: null, headerUrl: null, minutes: 0, achievementsUnlocked: 0, achievementsTotal: 0, achievementsOk: false, shared: false });
  const known = $derived(new Map((page.library ?? []).map((game) => [game.appid, game])));
  const withDrops = $derived((page.drops ?? []).map((game: BadgeGame): CardGame => ({ ...(known.get(game.appid) ?? blank(game.appid, game.name)), drops: game.drops, minutes: game.minutesPlayed })));
  const dropsOf = $derived(new Map(withDrops.map((game) => [game.appid, game.drops])));
  const listed = (ids: number[]): CardGame[] => ids.map((appid) => ({ ...(known.get(appid) ?? blank(appid, `Game ${appid}`)), drops: dropsOf.get(appid) }));
  const whitelist = $derived(listed(page.settings?.cardWhitelist ?? []));
  const blacklist = $derived(listed(page.settings?.cardBlacklist ?? []));
  const shown = $derived(find.tab === "drops" ? withDrops : find.tab === "whitelist" ? whitelist : blacklist);

  const games = $derived.by(() => {
    const found = searchGames(shown, find.query);
    if (find.sort === "drops") return [...found].sort((a, b) => (b.drops ?? 0) - (a.drops ?? 0) || a.name.localeCompare(b.name));
    return sortGames(found, find.sort);
  });

  const status = $derived(page.status);
  const emptyText = $derived(find.tab === "drops" ? "No games have card drops left." : find.tab === "whitelist" ? "Nothing whitelisted, so every game with drops can be farmed." : "Nothing blacklisted.");
  let now = $state(Date.now() / 1000);

  onMount(() => {
    void page.loadDrops(false);
    const clock = setInterval(() => (now = Date.now() / 1000), 15_000);
    return () => clearInterval(clock);
  });
</script>

{#snippet tabs()}
  <div class="tabs" role="tablist" aria-label="Which games">
    <button class="tab" class:on={find.tab === "drops"} type="button" role="tab" aria-selected={find.tab === "drops"} onclick={() => (find.tab = "drops")}>Games with drops<i>{withDrops.length}</i></button>
    <button class="tab" class:on={find.tab === "whitelist"} type="button" role="tab" aria-selected={find.tab === "whitelist"} onclick={() => (find.tab = "whitelist")}>Whitelisted<i>{whitelist.length}</i></button>
    <button class="tab" class:on={find.tab === "blacklist"} type="button" role="tab" aria-selected={find.tab === "blacklist"} onclick={() => (find.tab = "blacklist")}>Blacklisted<i>{blacklist.length}</i></button>
  </div>
{/snippet}

{#snippet tile(game: CardGame)}
  {@const white = page.settings?.cardWhitelist.includes(game.appid) ?? false}
  {@const black = page.settings?.cardBlacklist.includes(game.appid) ?? false}
  <GameTile {game} drops={game.drops} running={page.status?.running.includes(game.appid)}>
    {#snippet actions()}
      <button type="button" class:on={white} aria-pressed={white} aria-label={white ? `Take ${game.name} off the whitelist` : `Whitelist ${game.name}`} title={white ? "Take off the whitelist" : "Whitelist"} onclick={() => page.toggleCardList("cardWhitelist", game.appid)}><Check size={15} /></button>
      <button type="button" class:on={black} aria-pressed={black} aria-label={black ? `Take ${game.name} off the blacklist` : `Blacklist ${game.name}`} title={black ? "Take off the blacklist" : "Never farm this game"} onclick={() => page.toggleCardList("cardBlacklist", game.appid)}><Ban size={14} /></button>
    {/snippet}
  </GameTile>
{/snippet}

<GameGrid {games} bind:query={find.query} bind:sort={find.sort} sorts={SORTS} placeholder="Search" empty={emptyText} loading={find.tab === "drops" && page.dropsLoading} error={find.tab === "drops" ? page.dropsError : null} onretry={() => void page.loadDrops(true)} {tabs} {tile}>
  {#snippet controls()}
    {#if page.settings}
      <label class="auto">Run automatically<input class="switch" type="checkbox" checked={page.settings.farmOn} onchange={(event) => void page.change({ farmOn: event.currentTarget.checked })} /></label>
    {/if}
    <button class="icon-btn" type="button" aria-label="Read the card drops again" title="Read the card drops again" disabled={page.dropsLoading} onclick={() => void page.loadDrops(true)}><RefreshCw size={15} /></button>
    <StartButton feature="farm" />
  {/snippet}
  {#snippet note()}
    {#if status?.started.farm}
      <p class="note">
        {farmLine(status.farm)}{#if status.farm.nextCheckAt}. Next check {untilText(status.farm.nextCheckAt, now)}{/if}{#if status.farm.skippedRefundable > 0}. Skipping {plural(status.farm.skippedRefundable, "game")} that can still be refunded{/if}.
      </p>
    {/if}
  {/snippet}
</GameGrid>
