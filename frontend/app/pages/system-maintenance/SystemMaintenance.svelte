<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import HeartPulse from "@lucide/svelte/icons/heart-pulse";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import List from "@lucide/svelte/icons/list";
  import PackageCheck from "@lucide/svelte/icons/package-check";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Wifi from "@lucide/svelte/icons/wifi";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import type { SectionId } from "./api";
  import MaintenanceCard from "./MaintenanceCard.svelte";
  import { maintenanceState as s } from "./state.svelte";

  onMount(() => {
    void s.load();
  });

  const sections: { id: SectionId; title: string; icon: typeof Wifi }[] = [
    { id: "network", title: "Network & Connectivity", icon: Wifi },
    { id: "health", title: "System Health & Diagnostics", icon: HeartPulse },
    { id: "software", title: "Software Updates", icon: PackageCheck },
  ];

  const bySection = $derived(sections.map((section) => ({ ...section, cards: s.cards.filter((c) => c.section === section.id) })));
</script>

<div class="maintenance-page" aria-busy={s.loading}>
  <div class="top">
    <PageHeader
      title="System Maintenance"
      subtitle="Repair connections, check that Windows' own files are intact, and keep everything up to date."
    />
    <div class="tools">
      <span class="safety" title="Running two repairs at once can leave Windows in a worse state">
        <ShieldCheck size={13} />
        One action at a time
      </span>
      <div class="segmented" role="group" aria-label="View">
        <button
          class="icon-btn"
          class:active={s.view === "grid"}
          aria-pressed={s.view === "grid"}
          title="Grid view"
          onclick={() => s.setView("grid")}
        >
          <LayoutGrid size={16} />
        </button>
        <button
          class="icon-btn"
          class:active={s.view === "list"}
          aria-pressed={s.view === "list"}
          title="List view"
          onclick={() => s.setView("list")}
        >
          <List size={16} />
        </button>
      </div>
    </div>
  </div>

  {#if s.error}
    <div class="banner surface" role="alert">
      <CircleAlert size={18} />
      <span>{s.error}</span>
    </div>
  {/if}

  {#if s.loading}
    <div class="skeleton-heading shimmer"></div>
    <div class="skeleton-grid" aria-label="Loading maintenance tools">
      {#each Array(4) as _}
        <div class="skeleton-card">
          <span class="skeleton-icon shimmer"></span>
          <div class="skeleton-copy">
            <span class="skeleton-title shimmer"></span>
            <span class="skeleton-line shimmer"></span>
            <span class="skeleton-line short shimmer"></span>
          </div>
          <span class="skeleton-footer shimmer"></span>
        </div>
      {/each}
    </div>
  {:else}
    {#each bySection as section (section.id)}
      {#if section.cards.length}
        {@const SectionIcon = section.icon}
        <section data-section={section.id}>
          <div class="section-heading">
            <span class="section-icon"><SectionIcon size={14} strokeWidth={1.8} /></span>
            <h2>{section.title}</h2>
            <span class="section-count">{section.cards.length}</span>
            <span class="section-divider" aria-hidden="true"></span>
          </div>
          <div class="cards {s.view}">
            {#each section.cards as card, i (card.id)}
              <MaintenanceCard
                {card}
                view={s.view}
                wide={s.view === "grid" && section.cards.length % 2 === 1 && i === section.cards.length - 1}
              />
            {/each}
          </div>
        </section>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .maintenance-page { container-name: maintenance; container-type: inline-size; }

  .top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; flex-wrap: wrap; }

  .tools { display: flex; align-items: center; gap: 10px; /* Line the controls up with the page title, not the subtitle. */ margin-top: 4px; }

  .safety { display: inline-flex; align-items: center; gap: 6px; padding: 5px 11px; border: 1px solid rgb(var(--accent-rgb) / 0.18); border-radius: 999px; background: rgb(var(--accent-rgb) / 0.065); color: var(--text-2); font-size: 11.5px; font-weight: 500; }

  .segmented { display: flex; gap: 2px; padding: 1px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 10px; background: rgb(0 0 0 / 0.15); }

  .segmented .icon-btn { width: 32px; height: 28px; }

  .banner { display: flex; align-items: center; gap: 10px; margin-bottom: 16px; padding: 12px 14px; border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; font-size: 13px; }

  section { --section-tone: rgb(var(--accent-soft-rgb)); margin-bottom: 24px; }

  section[data-section="network"] { --section-tone: #63d7e9; }
  section[data-section="health"] { --section-tone: rgb(var(--accent-soft-rgb)); }
  section[data-section="software"] { --section-tone: #54d6a0; }

  .section-heading { display: flex; align-items: center; gap: 8px; margin-bottom: 11px; }

  .section-icon { display: grid; place-items: center; width: 24px; height: 24px; border: 1px solid color-mix(in srgb, var(--section-tone) 24%, transparent); border-radius: 7px; background: color-mix(in srgb, var(--section-tone) 9%, transparent); color: var(--section-tone); }

  h2 { color: var(--text-2); font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; }

  .section-count { color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; }

  .section-divider { height: 1px; flex: 1; background: linear-gradient(90deg, rgb(255 255 255 / 0.085), transparent); }

  .cards { display: grid; gap: 12px; align-items: stretch; }

  /* A card with its console open must not stretch the one beside it. */
  .cards:has(:global(.console.expanded)) { align-items: start; }

  .cards.grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }

  .cards.list { grid-template-columns: 1fr; }

  .skeleton-heading { width: 210px; height: 22px; margin: 0 0 12px; }

  .skeleton-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }

  .skeleton-card { display: grid; grid-template-columns: 42px minmax(0, 1fr); gap: 12px; min-height: 142px; padding: 16px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: var(--radius-lg); background: rgb(255 255 255 / 0.02); }

  .skeleton-icon { width: 42px; height: 42px; border-radius: 12px; }

  .skeleton-copy { display: flex; flex-direction: column; gap: 8px; padding-top: 3px; }

  .skeleton-title { width: 42%; height: 14px; }
  .skeleton-line { width: 88%; height: 10px; }
  .skeleton-line.short { width: 62%; }
  .skeleton-footer { grid-column: 1 / -1; align-self: end; width: 100%; height: 28px; margin-top: 8px; }

  @container maintenance (max-width: 760px) {
    .cards.grid,
    .skeleton-grid {
      grid-template-columns: 1fr;
    }

    .top {
      gap: 4px;
    }

    .tools {
      width: 100%;
      justify-content: space-between;
      margin-top: -10px;
    }
  }

  :global(:root.solid) .shimmer::after { animation: none; opacity: 0.3; }
</style>
