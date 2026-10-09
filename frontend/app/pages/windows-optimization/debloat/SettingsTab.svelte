<script lang="ts">
  // Every setting MYLE can change, one line each. A switch only records the
  // choice; the bar below applies the choices together.
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Star from "@lucide/svelte/icons/star";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import CategoryCard from "./CategoryCard.svelte";
  import CategoryGrid from "./CategoryGrid.svelte";
  import ListToolbar from "./ListToolbar.svelte";
  import SettingRow from "./SettingRow.svelte";
  import type { TweakStatus } from "./api";
  import { tweakCategories } from "./catalog";
  import { inProfile } from "./selection";
  import { debloat } from "./state.svelte";

  const view = $derived(debloat.views.settings);
  const query = $derived(view.query.trim().toLowerCase());
  /** Windows features have their own place, with the apps. */
  const settings = $derived(debloat.tweaks.filter((tweak) => tweak.state !== "unavailable" && tweak.category !== "features"));
  const shown = $derived(settings.filter((tweak) =>
    (tweak.title + " " + tweak.summary + " " + (tweak.note ?? "")).toLowerCase().includes(query) &&
    (view.filter === "all" ||
      (view.filter === "notApplied" && tweak.state !== "applied") ||
      (view.filter === "pending" && debloat.isPending(tweak)))
  ));
  const categories = $derived(tweakCategories.filter((category) => category.id !== "features"));

  /** Open while it has something left to do, unless the user folded it; a search opens all. */
  function isOpen(id: string, rows: TweakStatus[]) {
    if (query) return true;
    return debloat.opened[id] ?? rows.some((tweak) => tweak.state !== "applied" || debloat.isPending(tweak));
  }

  const recommendable = (rows: TweakStatus[]) =>
    rows.some((tweak) => inProfile(tweak.level, "recommended") && !debloat.isOn(tweak));
</script>

<div class="optimization-ui">
  <ListToolbar bind:query={view.query} filter={view.filter}
    filters={[{ value: "all", label: "All" }, { value: "notApplied", label: "Not on" }, { value: "pending", label: "Pending" }]}
    onchange={(value) => (view.filter = value)} placeholder="Search settings…">
    <button class="btn" disabled={debloat.locked || !debloat.undoable.length} title="Puts back exactly what MYLE changed." onclick={() => debloat.undo(debloat.undoable)}>
      <Undo2 size={14} /> Undo MYLE changes{debloat.undoable.length ? ` (${debloat.undoable.length})` : ""}
    </button>
  </ListToolbar>

  <p class="legend">
    <span>A switch only marks your choice; nothing changes until you review and apply below.</span>
    <span class="keys">
      <span><Star size={11} class="star" /> Recommended</span>
      <span><TriangleAlert size={11} class="warn" /> Read first: something stops working or works differently</span>
      <span><RotateCcw size={11} /> Needs a restart</span>
    </span>
  </p>

  {#if !debloat.status}
    <div class="empty-state"><strong>{debloat.error ? "Could not load the settings" : "Checking this PC…"}</strong></div>
  {:else if !shown.length}
    <div class="empty-state"><strong>No matching settings</strong><span>Your choices are kept when you change the search or filter.</span>
      <button class="btn" onclick={() => ((view.query = ""), (view.filter = "all"))}>Show all</button>
    </div>
  {:else}
    <CategoryGrid>
      {#each categories as category (category.id)}
        {@const rows = shown.filter((tweak) => tweak.category === category.id)}
        {#if rows.length}
          {@const on = rows.filter((tweak) => debloat.isOn(tweak)).length}
          <CategoryCard id={category.id} title={category.title} count={`${on}/${rows.length} on`} collapsible
            open={isOpen(category.id, rows)} ontoggle={(open) => (debloat.opened[category.id] = open)}>
            {#snippet tools()}
              {#if recommendable(rows)}
                <button type="button" class="link-btn" disabled={debloat.locked} title="Turns on the recommended settings of this group." onclick={() => debloat.recommend(rows)}>Recommended</button>
              {/if}
            {/snippet}
            <div class="rows">
              {#each rows as tweak (tweak.id)}
                <SettingRow
                  title={tweak.title}
                  summary={tweak.summary}
                  note={tweak.note}
                  checked={debloat.isOn(tweak)}
                  pending={debloat.isPending(tweak)}
                  disabled={debloat.locked}
                  recommended={inProfile(tweak.level, "recommended")}
                  caution={tweak.risk === "caution"}
                  restart={tweak.restart}
                  partial={tweak.state === "partial"}
                  onchange={(on) => debloat.setTweak(tweak, on)}
                />
              {/each}
            </div>
          </CategoryCard>
        {/if}
      {/each}
    </CategoryGrid>
  {/if}
</div>

<style>
  .rows { display: grid; gap: 1px; }
  .legend { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 6px 16px; color: var(--text-3); font-size: 11.5px; }
  .keys { display: flex; flex-wrap: wrap; gap: 4px 14px; }
  .keys > span { display: inline-flex; align-items: center; gap: 5px; }
  .keys :global(.star) { color: rgb(var(--accent-soft-rgb)); }
  .keys :global(.warn) { color: #efc38a; }
  .link-btn { padding: 3px 8px; border-radius: 6px; color: rgb(var(--accent-soft-rgb)); font-size: 11px; font-weight: 550; }
  .link-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  .link-btn:disabled { opacity: 0.45; }
</style>
