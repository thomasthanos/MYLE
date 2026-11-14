<script lang="ts">
  // What is free on Steam now, this week's picks and the deals, all on one page. The backend does the asking; see backend/src/steam_helper/.
  import { onMount, untrack } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import Users from "@lucide/svelte/icons/users";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import type { Game } from "./api";
  import GameCard from "./GameCard.svelte";
  import OfferCard from "./OfferCard.svelte";
  import { isLive, updatedAgo } from "./model";
  import { steamHelperState as helper, type SectionState, type SeeAllSection } from "./state.svelte";

  const CARD_MIN = 165;
  const CARD_GAP = 10;
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
    <span class="group-label"><SlidersHorizontal size={12} /> Mode:</span>
    <button
      class="chip filter-btn coop"
      class:active={helper.filters.onlineCoop}
      type="button"
      aria-pressed={helper.filters.onlineCoop}
      title={helper.filters.onlineCoop ? "Online co-op is active (click to turn off)" : "Show co-op games to play with friends"}
      onclick={() => helper.setFilter("onlineCoop", !helper.filters.onlineCoop)}
    >
      <span class="icon-wrap"><Users size={13} /></span>
      <span class="btn-text">Online co-op</span>
      <span class="state-pill">{helper.filters.onlineCoop ? "ON" : "OFF"}</span>
    </button>

    <button
      class="chip filter-btn solo"
      class:active={helper.filters.singlePlayer}
      type="button"
      aria-pressed={helper.filters.singlePlayer}
      title={helper.filters.singlePlayer ? "Single-player co-op is active (click to turn off)" : "Show single-player & co-op games"}
      onclick={() => helper.setFilter("singlePlayer", !helper.filters.singlePlayer)}
    >
      <span class="icon-wrap"><UserPlus size={13} /></span>
      <span class="btn-text">Single-player co-op</span>
      <span class="state-pill">{helper.filters.singlePlayer ? "ON" : "OFF"}</span>
    </button>

    <span class="divider" aria-hidden="true"></span>

    <span class="group-label">Focus:</span>
    <button
      class="chip filter-btn story"
      class:active={helper.filters.storyOnly}
      type="button"
      aria-pressed={helper.filters.storyOnly}
      title={helper.filters.storyOnly ? "Story mode is active (click to turn off)" : "Show story mode / campaign games (combine with Online co-op for story games with friends!)"}
      onclick={() => helper.setFilter("storyOnly", !helper.filters.storyOnly)}
    >
      <span class="icon-wrap"><BookOpen size={13} /></span>
      <span class="btn-text">Story only</span>
      <span class="state-pill">{helper.filters.storyOnly ? "ON" : "OFF"}</span>
    </button>

    <span class="spacer"></span>
    {#if updated}<span class="updated">{updated}</span>{/if}
    <button class="btn refresh-btn" type="button" disabled={refreshing} onclick={() => helper.load(true)}>
      <RefreshCw size={13} class={refreshing ? "spin" : ""} /> Refresh
    </button>
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
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 4px 0 6px; }
  .group-label { display: inline-flex; align-items: center; gap: 5px; color: var(--text-3); font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.6px; user-select: none; }
  
  /* Filter Toggle Buttons */
  .filter-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px 0 11px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    background: linear-gradient(180deg, #242838 0%, #161824 100%);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.12), 0 2px 5px rgba(0, 0, 0, 0.45);
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    user-select: none;
    transition: transform 0.15s var(--ease-out),
                border-color 0.15s var(--ease-out),
                box-shadow 0.15s var(--ease-out),
                background 0.15s var(--ease-out),
                color 0.15s var(--ease-out);
  }

  .filter-btn:hover {
    transform: translateY(-1.5px);
    border-color: rgba(255, 255, 255, 0.22);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.2), 0 5px 12px rgba(0, 0, 0, 0.55);
    color: var(--text-1);
  }

  .filter-btn:active {
    transform: translateY(0.5px);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.45);
  }

  .icon-wrap {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-3);
    transition: color 0.15s var(--ease-out);
  }

  .state-pill {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 30px;
    height: 18px;
    padding: 0 6px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.5px;
    background: rgba(0, 0, 0, 0.45);
    color: var(--text-3);
    border: 1px solid rgba(255, 255, 255, 0.08);
    transition: all 0.15s var(--ease-out);
  }

  /* CO-OP: Electric Cyan/Blue Theme */
  .filter-btn.coop:hover {
    border-color: rgba(56, 189, 248, 0.45);
  }
  .filter-btn.coop:hover .icon-wrap {
    color: #38bdf8;
  }
  .filter-btn.coop.active {
    background: linear-gradient(135deg, rgba(2, 132, 199, 0.35) 0%, rgba(3, 105, 161, 0.25) 100%);
    border-color: #38bdf8;
    color: #ffffff;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25), 0 0 14px rgba(56, 189, 248, 0.32);
  }
  .filter-btn.coop.active .icon-wrap {
    color: #38bdf8;
  }
  .filter-btn.coop.active .state-pill {
    background: #0284c7;
    color: #ffffff;
    border-color: #38bdf8;
    box-shadow: 0 0 8px rgba(56, 189, 248, 0.55);
  }

  /* SOLO: Purple/Violet Theme */
  .filter-btn.solo:hover {
    border-color: rgba(168, 85, 247, 0.45);
  }
  .filter-btn.solo:hover .icon-wrap {
    color: #c084fc;
  }
  .filter-btn.solo.active {
    background: linear-gradient(135deg, rgba(124, 58, 237, 0.35) 0%, rgba(109, 40, 217, 0.25) 100%);
    border-color: #a855f7;
    color: #ffffff;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25), 0 0 14px rgba(168, 85, 247, 0.32);
  }
  .filter-btn.solo.active .icon-wrap {
    color: #c084fc;
  }
  .filter-btn.solo.active .state-pill {
    background: #7c3aed;
    color: #ffffff;
    border-color: #c084fc;
    box-shadow: 0 0 8px rgba(168, 85, 247, 0.55);
  }

  /* STORY: Golden Amber Theme */
  .filter-btn.story:hover {
    border-color: rgba(245, 158, 11, 0.45);
  }
  .filter-btn.story:hover .icon-wrap {
    color: #fbbf24;
  }
  .filter-btn.story.active {
    background: linear-gradient(135deg, rgba(217, 119, 6, 0.35) 0%, rgba(180, 83, 9, 0.25) 100%);
    border-color: #f59e0b;
    color: #ffffff;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25), 0 0 14px rgba(245, 158, 11, 0.32);
  }
  .filter-btn.story.active .icon-wrap {
    color: #fbbf24;
  }
  .filter-btn.story.active .state-pill {
    background: #d97706;
    color: #ffffff;
    border-color: #fbbf24;
    box-shadow: 0 0 8px rgba(245, 158, 11, 0.55);
  }

  .divider { width: 1px; height: 20px; margin: 0 4px; background: rgba(255, 255, 255, 0.12); }
  :global(.spin) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
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
  .offers { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 10px; }
  .skeleton { aspect-ratio: 165 / 165; border-radius: var(--radius-md); }
  .offer-skeleton { aspect-ratio: auto; height: 64px; border-radius: var(--radius-md); }
  .note { margin: 8px 0 0; color: var(--text-3); font-size: 12px; }
  .more-gap { margin-top: 10px; }
  .end { height: 1px; }
  .empty { display: grid; justify-items: center; gap: 7px; padding: 28px 20px; text-align: center; }
  .empty strong { font-size: 13.5px; }
  .empty p { max-width: 440px; margin: 0; color: var(--text-3); font-size: 11.5px; }
</style>
