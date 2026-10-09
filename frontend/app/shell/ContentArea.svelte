<script lang="ts">
  import type { Component } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";
  import { nav } from "../../lib/nav.svelte";
  import { settings } from "../../lib/settings.svelte";
  import { account } from "../account/account.svelte";
  import { canOpen, pages } from "../pages/registry";

  // An owner-only page someone else lands on (a link, a sign-out while it
  // is open) gives way to the first page. Before the first answer about the
  // account is in, the first page shows without forgetting where to go.
  const asked = $derived(pages.find((p) => p.id === nav.current));
  const allowed = $derived(!!asked && canOpen(asked, account.owner));
  const def = $derived(allowed && asked ? asked : pages[0]);
  $effect(() => {
    if (!allowed && account.accessKnown) nav.go(pages[0].id);
  });
  // The owner-only pages load the first time they are needed: fetched while
  // idle once the owner is known, so they still open at once for the owner,
  // and never fetched for anyone else.
  let loaded = $state.raw<Record<string, Component>>({});
  let loadError = $state<string | null>(null);
  function load(id: string, module: NonNullable<(typeof pages)[number]["load"]>) {
    if (loaded[id]) return;
    module()
      .then((m) => {
        loaded = { ...loaded, [id]: m.default };
        loadError = null;
      })
      .catch((error) => {
        if (nav.current === id) loadError = error instanceof Error ? error.message : String(error);
      });
  }
  $effect(() => {
    if (!def.load) return;
    loadError = null;
    load(def.id, def.load);
  });
  $effect(() => {
    if (!account.owner) return;
    const idle = (work: () => void) =>
      "requestIdleCallback" in window ? requestIdleCallback(work, { timeout: 3000 }) : setTimeout(work, 1500);
    for (const page of pages) if (page.load && canOpen(page, true)) idle(() => load(page.id, page.load!));
  });
  const Page = $derived(def.component ?? loaded[def.id]);
  // A short slide-in; none at all in the lighter mode or with reduced motion,
  // so a slow machine spends its first frames on the page, not the animation.
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const enter = $derived(!settings.glass || reducedMotion ? 0 : 160);
  let scroller = $state<HTMLDivElement>();
  // Ignore the scroll events caused by swapping pages.
  let restoring = false;

  // Runs after the new page is in the DOM: put it back where it was left.
  $effect(() => {
    const page = nav.current;
    if (!scroller) return;
    restoring = true;
    scroller.scrollTop = nav.scrollOf(page);
    requestAnimationFrame(() => (restoring = false));
  });

  function onScroll() {
    if (!restoring && scroller) nav.rememberScroll(nav.current, scroller.scrollTop);
  }
</script>

<main class="content glass">
  <div class="scroller" class:fill={def.fill} bind:this={scroller} onscroll={onScroll}>
    {#key nav.current}
      <div class="page" class:fill={def.fill} in:fly={{ y: 8, duration: enter, easing: cubicOut }}>
        {#if Page}
          <Page />
        {:else if loadError}
          <p class="page-state" role="alert">This page could not be loaded: {loadError}</p>
        {:else}
          <p class="page-state" aria-busy="true">Loading {def.label}…</p>
        {/if}
      </div>
    {/key}
  </div>
</main>

<style>
  .page-state {
    margin: 48px auto;
    color: var(--text-3);
    font-size: 13px;
  }

  .content {
    grid-area: main;
    min-width: 0;
    min-height: 0;
    border-radius: var(--shell-panel-radius);
  }

  /* Scrolling lives in a child so the glass rim is never clipped or scrolled. */
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
    padding: 28px 32px;
    border-radius: inherit;
    contain: strict;
  }

  /* A page that fills the height: its own parts scroll; the page scrolls
     only when the window is too short for them. */
  .page.fill {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 480px;
  }

  @media (max-height: 860px) {
    .scroller.fill {
      padding-block: 18px;
    }

    .page.fill > :global(header) {
      margin-bottom: 14px;
    }
  }

  /* Keep the scrollbar clear of the panel's rounded top and bottom edges. */
  .scroller::-webkit-scrollbar {
    width: 10px;
    height: 10px;
  }

  .scroller::-webkit-scrollbar-track {
    margin-block: 16px;
    background: transparent;
  }

  .scroller::-webkit-scrollbar-thumb {
    border: 3px solid transparent;
    border-radius: 999px;
    background: rgb(210 220 245 / 0.13) padding-box;
  }

  .scroller::-webkit-scrollbar-thumb:hover {
    background-color: rgb(210 220 245 / 0.24);
  }
</style>
