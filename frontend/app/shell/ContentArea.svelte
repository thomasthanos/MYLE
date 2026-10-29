<script lang="ts">
  import { untrack } from "svelte";
  import { nav } from "../../lib/nav.svelte";
  import { account } from "../account/account.svelte";
  import { canOpen, pages } from "../pages/registry";
  import { whenShown } from "../../lib/startup";
  import { PageModules } from "./page-modules.svelte";

  // An owner-only page someone else lands on (a link, a sign-out while it is open) gives way to the first page. Before the first answer about the account is in, the first page shows without forgetting where to go.
  const asked = $derived(pages.find((p) => p.id === nav.current));
  const allowed = $derived(!!asked && canOpen(asked, account.owner));
  const def = $derived(allowed && asked ? asked : pages[0]);
  $effect(() => {
    if (!allowed && account.accessKnown) nav.go(pages[0].id);
  });
  const modules = new PageModules();
  const loadError = $derived(modules.errors[def.id]);
  $effect(() => {
    const { id, load } = def;
    if (load) untrack(() => void modules.load(id, load));
  });
  // Parse secondary pages one at a time while idle, after the main window is
  // visible. Owner-only modules remain unavailable to other accounts.
  $effect(() => {
    const owner = account.owner;
    let cancelled = false;
    const idle = () => new Promise<void>((resolve) =>
      "requestIdleCallback" in window ? requestIdleCallback(() => resolve(), { timeout: 3000 }) : setTimeout(resolve, 1500));
    void (async () => {
      await whenShown();
      for (const page of pages) {
        if (cancelled) return;
        if (!page.load || !canOpen(page, owner) || modules.loaded[page.id]) continue;
        await idle();
        if (cancelled) return;
        await modules.load(page.id, page.load);
      }
    })();
    return () => { cancelled = true; };
  });
  const Page = $derived(def.component ?? modules.loaded[def.id]);
  let scroller = $state<HTMLDivElement>();
  // Ignore the scroll events caused by swapping pages.
  let restoring = false;

  // Runs after the new page is in the DOM: put it back where it was left.
  $effect(() => {
    const page = nav.current;
    if (!scroller || !Page) return;
    restoring = true;
    scroller.scrollTop = nav.scrollOf(page);
    requestAnimationFrame(() => (restoring = false));
  });

  function onScroll() {
    if (!restoring && scroller && Page) nav.rememberScroll(nav.current, scroller.scrollTop);
  }
</script>

<main class="content glass">
  <div class="scroller" class:fill={def.fill} bind:this={scroller} onscroll={onScroll}>
    {#key nav.current}
      <div class="page" class:fill={def.fill}>
        {#if Page}
          <Page />
        {:else if loadError}
          <div class="page-state" role="alert">
            <p>This page could not be loaded: {loadError}</p>
            {#if def.load}<button class="btn small" onclick={() => modules.load(def.id, def.load!)}>Try again</button>{/if}
          </div>
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

  /* A page that fills the height: its own parts scroll; the page scrolls only when the window is too short for them. */
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
    background: var(--scroll-thumb) padding-box;
  }

  .scroller::-webkit-scrollbar-thumb:hover {
    background-color: var(--scroll-thumb-hover);
  }
</style>
