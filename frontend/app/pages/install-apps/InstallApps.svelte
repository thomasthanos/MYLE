<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import AppCard from "./AppCard.svelte";
  import SelectionBar from "./SelectionBar.svelte";
  import Toolbar from "./Toolbar.svelte";
  import { appsState, type AppEntry } from "./state.svelte";

  onMount(() => {
    void appsState.init();
    return () => appsState.forgetRecent();
  });

  const nothingShown = $derived(appsState.shownCount === 0 && !appsState.showCatalog);
</script>

{#snippet apps(list: AppEntry[])}
  <div class="apps">
    {#each list as app (app.id)}
      <AppCard {app} />
    {/each}
  </div>
{/snippet}

<PageHeader title="Install Apps" subtitle="Install, update and keep track of your apps in one place." />

<Toolbar />

{#if appsState.wingetError}
  <div class="banner surface" role="alert">
    <CircleAlert size={18} />
    <span>{appsState.wingetError}</span>
  </div>
{/if}

{#if appsState.visiblePinned.length}
  <section>
    <h2>Pinned from catalog <span class="n">{appsState.visiblePinned.length}</span></h2>
    {@render apps(appsState.visiblePinned)}
  </section>
{/if}

{#each appsState.groups as group (group.title ?? "all")}
  <section class="lazy">
    {#if group.title}
      <h2>{group.title} <span class="n">{group.apps.length}</span></h2>
    {/if}
    {@render apps(group.apps)}
  </section>
{/each}

{#if appsState.showCatalog}
  <section>
    <h2>More from catalog</h2>
    {#if appsState.catalogLoading}
      <p class="hint"><LoaderCircle size={15} class="spin" /> Searching winget…</p>
    {:else if appsState.catalogResults.length}
      {@render apps(appsState.catalogResults)}
    {:else}
      <p class="hint">No matches in the winget catalog either.</p>
    {/if}
  </section>
{/if}

{#if nothingShown}
  <p class="empty">
    {appsState.filter === "updates" && appsState.checkedOnce ? "Everything is up to date." : "No apps match these filters."}
  </p>
{/if}

<SelectionBar />

<style>
  section {
    margin-bottom: 26px;
  }

  /* Off-screen categories skip layout and paint until scrolled near. */
  section.lazy {
    content-visibility: auto;
    contain-intrinsic-size: auto 320px;
  }

  h2 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-bottom: 12px;
    color: var(--text-2);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .n {
    color: var(--text-3);
    font-weight: 500;
  }

  .apps {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 10px;
  }

  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 20px;
    padding: 12px 14px;
    border-color: rgb(229 72 77 / 0.35);
    color: #ffb4b0;
    font-size: 13px;
  }

  .hint,
  .empty {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
    font-size: 13px;
  }

  .empty {
    justify-content: center;
    padding: 48px 0;
  }
</style>
