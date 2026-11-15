<script lang="ts">
  import { untrack } from "svelte";
  import { nav } from "../../lib/nav.svelte";
  import { account } from "../account/account.svelte";
  import { canOpen, pages } from "../pages/registry";
  import { whenShown } from "../../lib/startup";
  import type { PageModules } from "./page-modules.svelte";
  import PageBoundary from "../../lib/components/PageBoundary.svelte";
  import PageHeader from "../../lib/components/PageHeader.svelte";
  import PageLoading from "./PageLoading.svelte";

  let { modules }: { modules: PageModules } = $props();

  // An owner-only page someone else lands on (a link, a sign-out while it is open) gives way to the first page. Before the first answer about the account is in, the first page shows without forgetting where to go.
  const asked = $derived(pages.find((p) => p.id === nav.current));
  const allowed = $derived(!!asked && canOpen(asked, account.owner));
  const def = $derived(allowed && asked ? asked : pages[0]);
  $effect(() => {
    if (!allowed && account.accessKnown) nav.go(pages[0].id);
  });
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

  // Show the loading state from the top, then restore the page's own position.
  $effect(() => {
    const page = def.id;
    if (!scroller) return;
    restoring = true;
    scroller.scrollTop = Page ? nav.scrollOf(page) : 0;
    const frame = requestAnimationFrame(() => (restoring = false));
    return () => cancelAnimationFrame(frame);
  });

  function onScroll() {
    if (!restoring && scroller && Page) nav.rememberScroll(def.id, scroller.scrollTop);
  }
</script>

<main class="content glass">
  <div class="scroller" class:fill={def.fill} bind:this={scroller} onscroll={onScroll}>
    {#key def.id}
      <div class="page" class:fill={def.fill} class:ready={!!Page}>
        {#if Page}
          <PageBoundary><Page /></PageBoundary>
        {:else if loadError}
          <PageHeader title={def.label} />
          <div class="page-state surface">
            <div role="alert">
              <p>This page could not be loaded.</p>
              <p class="error-detail">{loadError}</p>
            </div>
            {#if def.load}<button class="btn small" onclick={() => modules.load(def.id, def.load!)}>Try again</button>{/if}
          </div>
        {:else}
          <PageLoading page={def} />
        {/if}
      </div>
    {/key}
  </div>
</main>

<style>
  .page-state {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px 20px;
    padding: 14px 16px;
    border-radius: var(--radius-md);
    color: var(--text-1);
    font-size: 13px;
  }

  .error-detail {
    margin-top: 3px;
    color: var(--text-3);
    overflow-wrap: anywhere;
  }

  @media (prefers-reduced-motion: no-preference) {
    .page.ready { animation: page-in 160ms var(--ease-out); }
  }

  @keyframes page-in {
    from { opacity: 0.7; }
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
