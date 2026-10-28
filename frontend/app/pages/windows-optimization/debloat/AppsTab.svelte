<script lang="ts">
  // The installed apps MYLE can remove, by who made them. Ticking one only chooses it; the bar below removes the chosen ones, with the rest.
  import Store from "@lucide/svelte/icons/store";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import CategoryCard from "./CategoryCard.svelte";
  import CategoryGrid from "./CategoryGrid.svelte";
  import ChoiceButtons from "./ChoiceButtons.svelte";
  import ListToolbar from "./ListToolbar.svelte";
  import SettingRow from "./SettingRow.svelte";
  import { appCategories } from "./catalog";
  import { inProfile } from "./selection";
  import { debloat } from "./state.svelte";

  const view = $derived(debloat.views.apps);
  const query = $derived(view.query.trim().toLowerCase());
  const matches = (title: string) => title.toLowerCase().includes(query);
  const shown = $derived(debloat.installedApps.filter((app) =>
    matches(app.title) &&
    (view.filter === "all" ||
      (view.filter === "suggested" && inProfile(app.level, "recommended")) ||
      (view.filter === "chosen" && debloat.isRemoving(app.id)))
  ));
  const removed = $derived((debloat.status?.apps ?? []).filter((app) => app.removedByMyle && !app.packages.length && matches(app.title)));
  /** Parts of Windows itself: ticked means removed (or to be removed). */
  const features = $derived(debloat.tweaks.filter((tweak) =>
    tweak.category === "features" && tweak.state !== "unavailable" &&
    matches(tweak.title + " " + tweak.summary) &&
    (view.filter === "all" ||
      (view.filter === "suggested" && inProfile(tweak.level, "maximum")) ||
      (view.filter === "chosen" && debloat.isOn(tweak)))
  ));
  const featureName = (title: string) => title.replace(/^Remove /, "");
</script>

<div class="optimization-ui">
  <ListToolbar bind:query={view.query} filter={view.filter}
    filters={[{ value: "all", label: "All" }, { value: "suggested", label: "Suggested" }, { value: "chosen", label: "Chosen" }]}
    onchange={(value) => (view.filter = value)} placeholder="Search installed apps…">
    <span class="choose">Choose</span>
    <ChoiceButtons options={[{ value: "recommended", label: "Suggested" }, { value: "all", label: "All" }, { value: "none", label: "None" }]}
      ariaLabel="Choose apps" disabled={debloat.locked || !debloat.status}
      onchange={(preset) => debloat.selectApps(preset as "recommended" | "all" | "none")} />
  </ListToolbar>

  {#if !debloat.status}
    <div class="empty-state"><strong>{debloat.error ? "Could not load the apps" : "Checking installed apps…"}</strong></div>
  {:else if !shown.length && !removed.length && !features.length}
    <div class="empty-state">
      <strong>{debloat.installedApps.length ? "No matching apps" : "Nothing to remove"}</strong>
      <span>{debloat.installedApps.length ? "Your choices are kept when you change the search or filter." : "None of the apps on MYLE's list is installed."}</span>
      {#if debloat.installedApps.length}<button class="btn" onclick={() => ((view.query = ""), (view.filter = "all"))}>Show all</button>{/if}
    </div>
  {:else}
    <CategoryGrid>
      {#each appCategories as category (category.id)}
        {@const rows = shown.filter((app) => app.group === category.id)}
        {@const removedRows = removed.filter((app) => app.group === category.id)}
        {#if rows.length || removedRows.length}
          <CategoryCard id={category.id} title={category.title} wide={category.id === "microsoft"}
            count={`${rows.filter((app) => debloat.isRemoving(app.id)).length}/${rows.length} chosen`}>
            {#if category.id === "xbox" && rows.length}
              <p class="hint"><TriangleAlert size={12} /> Some games need these. Keep them if you play on this PC.</p>
            {/if}
            <div class="app-grid" class:many={category.id === "microsoft"}>
              {#each rows as app (app.id)}
                <SettingRow control="check" title={app.title} subtitle={app.about} keep={app.keep}
                  checked={debloat.isRemoving(app.id)} disabled={debloat.locked}
                  recommended={inProfile(app.level, "recommended")} onchange={(on) => debloat.setApp(app.id, on)} />
              {/each}
            </div>
            {#if removedRows.length}
              <div class="removed">
                <span>Removed by MYLE</span>
                {#each removedRows as app (app.id)}
                  <button type="button" class="again" disabled={debloat.locked} title="Opens it in the Microsoft Store." onclick={() => debloat.openStore(app)}>
                    <Store size={12} /> {app.title}
                  </button>
                {/each}
              </div>
            {/if}
          </CategoryCard>
        {/if}
      {/each}
      {#if features.length}
        <CategoryCard id="features" title="Windows features" wide
          description="Parts of Windows few people use. One removed here can be added back; that downloads it from Windows Update."
          count={`${features.filter((tweak) => debloat.isOn(tweak)).length}/${features.length} removed or chosen`}>
          <div class="app-grid many">
            {#each features as tweak (tweak.id)}
              <SettingRow control="check" title={featureName(tweak.title)} summary={tweak.summary} note={tweak.note}
                checked={debloat.isOn(tweak)} pending={debloat.isPending(tweak)} disabled={debloat.locked}
                recommended={inProfile(tweak.level, "recommended")} caution={tweak.risk === "caution"}
                onchange={(on) => debloat.setTweak(tweak, on)}>
                {#snippet aside()}
                  {#if tweak.state === "applied" && !debloat.isPending(tweak)}<span class="gone">Removed</span>{/if}
                {/snippet}
              </SettingRow>
            {/each}
          </div>
        </CategoryCard>
      {/if}
    </CategoryGrid>
  {/if}
</div>

<style>
  .choose { color: var(--text-3); font-size: 11px; }
  .gone { padding: 0 5px; border-radius: 5px; background: rgb(62 207 142 / 0.08); color: #98dfbd; font-size: 10px; font-weight: 600; line-height: 18px; }
  .hint { display: flex; align-items: center; gap: 6px; margin: 0 0 4px 8px; color: #e6c48f; font-size: 11px; }
  .hint :global(svg) { flex: none; }
  .app-grid { display: grid; grid-template-columns: minmax(0, 1fr); gap: 1px 8px; }
  @container optimization-list (min-width: 520px) { .app-grid.many { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @container optimization-list (min-width: 820px) { .app-grid.many { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @container optimization-list (min-width: 1150px) { .app-grid.many { grid-template-columns: repeat(4, minmax(0, 1fr)); } }
  .removed { display: flex; align-items: center; flex-wrap: wrap; gap: 5px; margin: 6px 0 0 8px; padding-top: 7px; border-top: 1px solid rgb(255 255 255 / 0.05); }
  .removed > span { margin-right: 4px; color: var(--text-3); font-size: 10.5px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
  .again { display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 8px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 7px; color: var(--text-2); font-size: 11.5px; }
  .again:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.3); color: var(--text-1); }
  .again:disabled { opacity: 0.45; }
</style>
