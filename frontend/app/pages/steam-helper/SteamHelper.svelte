<script lang="ts">
  // What is free on Steam now, this week's picks and the deals, all on one page. The backend does the asking; see backend/src/steam_helper/.
  import { onMount, untrack } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import type { Game } from "./api";
  import GameCard from "./GameCard.svelte";
  import OfferCard from "./OfferCard.svelte";
  import { isLive, updatedAgo, type Filters } from "./model";
  import { steamHelperState as helper, type SectionState, type SeeAllSection } from "./state.svelte";

  const CARD_MIN = 165;
  const CARD_GAP = 10;
  const MODES: [keyof Filters, string][] = [["onlineCoop", "Online co-op"], ["singlePlayer", "Single-player"]];
  const TITLES: Record<SeeAllSection, string> = { picks: "This week's picks", deals: "Deals", freeToPlay: "Free to play" };

  let now = $state(Date.now() / 1000);
  let width = $state(1000);
  let sentinelVisible = $state(false);

  const columns = $derived(Math.max(1, Math.floor((width + CARD_GAP) / (CARD_MIN + CARD_GAP))));
  const offers = $derived(helper.free.items.filter((offer) => isLive(offer.endsAt, now)));
  const anyToKeep = $derived(offers.some((offer) => offer.kind === "freeToKeep"));
  const sections = $derived([helper.free, helper.picks, helper.deals, helper.freeToPlay]);
  const refreshing = $derived(sections.some((section) => section.loading));
  const updated = $derived.by(() => {
    const times = sections.map((section) => section.updatedAt).filter((time): time is number => time !== null);
    return times.length ? updatedAgo(Math.min(...times), now) : "";
  });
  const allGames = $derived(helper.seeAll === "picks" ? helper.picks.items : helper.moreGames);
  const allLoading = $derived(helper.seeAll === "picks" ? helper.picks.loading : helper.moreLoading);

  onMount(() => {
    helper.load();
    const clock = setInterval(() => (now = Date.now() / 1000), 30_000);
    return () => clearInterval(clock);
  });

  $effect(() => {
    if (sentinelVisible && helper.seeAll && helper.seeAll !== "picks" && !helper.moreLoading && !helper.moreDone && !helper.moreError) {
      untrack(() => void helper.loadMore());
    }
  });

  function watchEnd(node: HTMLElement) {
    const observer = new IntersectionObserver(([entry]) => (sentinelVisible = entry.isIntersecting), { rootMargin: "400px" });
    observer.observe(node);
    return {
      destroy() {
        observer.disconnect();
        sentinelVisible = false;
      },
    };
  }
</script>

{#snippet skeletons()}
  <div class="grid">
    {#each { length: columns } as _}<div class="skeleton shimmer"></div>{/each}
  </div>
{/snippet}

{#snippet failed(message: string)}
  <div class="empty surface">
    <strong>This section could not be loaded</strong>
    <p>{message}</p>
    <button class="btn" type="button" onclick={() => helper.load(true)}>Try again</button>
  </div>
{/snippet}

{#snippet stale()}
  <p class="note">Could not refresh, so this is the last saved list. <button class="link" type="button" onclick={() => helper.load(true)}>Try again</button></p>
{/snippet}

{#snippet gameSection(section: SectionState<Game>, id: SeeAllSection, note: string, nothing: string)}
  <section>
    <header class="head">
      <h2>{TITLES[id]}</h2>
      <small>{note}</small>
      {#if section.items.length}<button class="link see-all" type="button" onclick={() => helper.openSeeAll(id)}>See all</button>{/if}
    </header>
    {#if section.items.length}
      <div class="grid">
        {#each section.items.slice(0, columns) as game (game.appid)}
          <GameCard {game} {now} showModes={id === "picks"} />
        {/each}
      </div>
      {#if section.stale}{@render stale()}{/if}
    {:else if section.loading}
      {@render skeletons()}
    {:else if section.error}
      {@render failed(section.error)}
    {:else}
      <div class="empty surface"><strong>{nothing}</strong></div>
    {/if}
  </section>
{/snippet}

<div class="page" bind:clientWidth={width}>
  <PageHeader title="Steam Game Helper" subtitle="Deals, free games and what's worth playing this week." />

  <div class="filters" role="group" aria-label="Filter games">
    {#each MODES as [name, label] (name)}
      <button class="chip" class:active={helper.filters[name]} type="button" aria-pressed={helper.filters[name]} onclick={() => helper.setFilter(name, !helper.filters[name])}>{label}</button>
    {/each}
    <span class="divider" aria-hidden="true"></span>
    <button class="chip" class:active={helper.filters.storyOnly} type="button" aria-pressed={helper.filters.storyOnly} onclick={() => helper.setFilter("storyOnly", !helper.filters.storyOnly)}>Story only</button>
    <span class="spacer"></span>
    {#if updated}<span class="updated">{updated}</span>{/if}
    <button class="btn" type="button" disabled={refreshing} onclick={() => helper.load(true)}><RefreshCw size={14} /> Refresh</button>
  </div>

  {#if helper.seeAll}
    <section>
      <header class="head">
        <button class="btn back" type="button" onclick={() => helper.back()}><ArrowLeft size={14} /> Back</button>
        <h2>{TITLES[helper.seeAll]}</h2>
      </header>
      {#if allGames.length}
        <div class="grid">
          {#each allGames as game (game.appid)}
            <GameCard {game} {now} showModes={helper.seeAll === "picks"} />
          {/each}
        </div>
      {:else if !allLoading && !helper.moreError}
        <div class="empty surface"><strong>Nothing matches these filters</strong></div>
      {/if}
      {#if allLoading}<div class="more-gap">{@render skeletons()}</div>{/if}
      {#if helper.moreError}
        <p class="note">{helper.moreError} <button class="link" type="button" onclick={() => void helper.loadMore()}>Try again</button></p>
      {/if}
      {#if helper.seeAll !== "picks"}<div class="end" use:watchEnd></div>{/if}
    </section>
  {:else}
    <section>
      <header class="head"><h2>Free right now</h2><small>ends soon first, not affected by filters</small></header>
      {#if offers.length}
        <div class="offers">
          {#each offers as offer (offer.offerId)}<OfferCard {offer} {now} />{/each}
        </div>
        {#if !anyToKeep}<p class="note">No games are 100% off right now.</p>{/if}
        {#if helper.free.stale}{@render stale()}{/if}
      {:else if helper.free.loading}
        <div class="skeleton offer-skeleton shimmer"></div>
      {:else if helper.free.error}
        {@render failed(helper.free.error)}
      {:else}
        <div class="empty surface"><strong>Nothing is free on Steam right now</strong></div>
      {/if}
    </section>

    {@render gameSection(helper.picks, "picks", "changes every Monday", "No picks for these filters this week")}
    {@render gameSection(helper.deals, "deals", "best reviewed first", "No deals match these filters")}
    {@render gameSection(helper.freeToPlay, "freeToPlay", "always free, best reviewed first", "No free games match these filters")}
  {/if}
</div>

<style>
  .page { min-width: 0; }
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .divider { width: 1px; height: 18px; margin: 0 4px; background: var(--glass-border); }
  .spacer { flex: 1; }
  .updated { color: var(--text-3); font-size: 12px; white-space: nowrap; }
  section { margin-top: 18px; }
  .head { display: flex; align-items: baseline; gap: 8px; margin-bottom: 8px; }
  .head:has(.back) { align-items: center; }
  h2 { margin: 0; font-size: 14.5px; font-weight: 700; }
  small { color: var(--text-3); font-size: 11.5px; }
  .link { padding: 0; border: 0; background: none; color: var(--accent); font: inherit; cursor: pointer; }
  .link:hover, .link:focus-visible { text-decoration: underline; }
  .see-all { margin-left: auto; font-size: 12px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(165px, 1fr)); gap: 10px; }
  .offers { display: grid; grid-template-columns: repeat(auto-fit, minmax(250px, 1fr)); gap: 10px; }
  .skeleton { aspect-ratio: 165 / 150; border-radius: var(--radius-md); }
  .offer-skeleton { aspect-ratio: auto; height: 72px; }
  .note { margin: 8px 0 0; color: var(--text-3); font-size: 12px; }
  .more-gap { margin-top: 10px; }
  .end { height: 1px; }
  .empty { display: grid; justify-items: center; gap: 7px; padding: 28px 20px; text-align: center; }
  .empty strong { font-size: 13.5px; }
  .empty p { max-width: 440px; margin: 0; color: var(--text-3); font-size: 11.5px; }
</style>
