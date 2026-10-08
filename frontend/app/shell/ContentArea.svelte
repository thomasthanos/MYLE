<script lang="ts">
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
  const Page = $derived(def.component);
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
        <Page />
      </div>
    {/key}
  </div>
</main>

<style>
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
