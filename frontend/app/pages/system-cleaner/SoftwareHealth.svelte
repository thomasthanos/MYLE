<script lang="ts">
  import { onMount, tick } from "svelte";
  import Radar from "@lucide/svelte/icons/radar";
  import History from "@lucide/svelte/icons/history";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Search from "@lucide/svelte/icons/search";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import FolderMinus from "@lucide/svelte/icons/folder-minus";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Camera from "@lucide/svelte/icons/camera";
  import Mic from "@lucide/svelte/icons/mic";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import AppWindow from "@lucide/svelte/icons/app-window";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { formatSize } from "./api";
  import { cleanerState } from "./state.svelte";
  import { ACTION_LABELS, MODULES, health, type HealthAction } from "./health.svelte";
  import { CLEANUP_MODULES, cleanupPlan, compatibleActions, filterFindings, selectResults, type ListFilter } from "./health-model";

  let category = $state("cleanup");
  let query = $state("");
  let filter = $state<ListFilter>("all");
  let sort = $state<"name" | "size">("name");
  let showRestore = $state(false);
  let page = $state(1);
  let resultsSummary: HTMLSpanElement | undefined;
  const pageSize = 50;
  const disabled = $derived(health.scanning || health.busy || cleanerState.busy);
  const rows = $derived(filterFindings(health.findings, { category, query, filter, sort, failures: health.failures }));
  const pageCount = $derived(Math.max(1, Math.ceil(rows.length / pageSize)));
  const currentPage = $derived(Math.min(page, pageCount));
  const visibleRows = $derived(rows.slice((currentPage - 1) * pageSize, currentPage * pageSize));
  const plan = $derived(cleanupPlan(health.findings, health.selected, health.overrides));
  const controls = $derived(health.picked.filter((f) => !CLEANUP_MODULES.has(f.module)));
  const controlActions = $derived(compatibleActions(controls));
  const selectedBytes = $derived(health.picked.reduce((n, f) => n + (f.size ?? 0), 0));
  const failedCount = $derived(Object.keys(health.failures).filter((id) => health.findings.some((f) => f.id === id)).length);
  const moduleTitle = (id: string) => MODULES.find((m) => m.id === id)?.title ?? id;
  const count = (id: string) => health.findings.filter((f) => id === "cleanup" ? CLEANUP_MODULES.has(f.module) : id === "controls" ? !CLEANUP_MODULES.has(f.module) : f.module === id).length;
  const filters: { id: ListFilter; label: string }[] = [{ id: "all", label: "All results" }, { id: "recommended", label: "Recommended" }, { id: "review", label: "Needs review" }, { id: "failed", label: "Failed" }];
  const lastSucceeded = $derived(health.lastResults.filter((r) => r.ok).length);
  const lastFailed = $derived(health.lastResults.filter((r) => !r.ok).length);
  const categoryError = $derived(health.reports[category]?.error);
  const permissionIcons: Record<string, typeof Camera> = { "Κάμερα": Camera, "Μικρόφωνο": Mic, "Τοποθεσία": MapPin, Camera, Microphone: Mic, Location: MapPin };
  onMount(() => health.loadRestore());
  $effect(() => { category; query; filter; sort; page = 1; });

  function chooseRecommended() {
    if (disabled) return;
    health.selected = selectResults(health.selected, rows.filter((f) => f.recommended && f.cleanupAction));
  }
  function controlAction(action: HealthAction) {
    void health.applyBatch(controls.filter((f) => f.actions.includes(action)).map((f) => ({ id: f.id, action })));
  }
  async function changePage(next: number) {
    page = next;
    await tick();
    resultsSummary?.focus({ preventScroll: true });
    resultsSummary?.scrollIntoView({ block: "start" });
  }
</script>

<section class="health" aria-label="Software Health">
  <header class="overview">
    <div>
      <h2>Software Health</h2>
      <p>Clean leftover files or manage what your apps can do. Review changes before applying them.</p>
    </div>
    <div class="actions">
      {#if health.restore.length}
        <button class="btn" disabled={disabled} aria-expanded={showRestore} onclick={() => showRestore = !showRestore}><History size={15} /> History ({health.restore.length})</button>
      {/if}
      <button class="btn primary" disabled={disabled} onclick={() => health.scan()}>
        {#if health.scanning}<LoaderCircle size={15} class="spin" />{:else}<Radar size={15} />{/if}
        {health.scanning ? "Scanning…" : "Scan software"}
      </button>
    </div>
  </header>

  {#if health.scanning || health.operation}
    <div class="work surface" role="status">
      {#if health.operation}
        <span>{health.operation.current || "Finishing changes…"}<b>{health.operation.done}/{health.operation.total}</b></span>
        <ProgressBar value={health.operation.total ? health.operation.done / health.operation.total : 0} label="Software Health changes" />
      {:else}
        <span>{health.running.size ? `Scanning ${[...health.running].map(moduleTitle).join(", ")}` : "Preparing scan…"}<b>{health.done}/{health.total}</b></span>
        <ProgressBar value={health.fraction} label="Software Health scan" />
      {/if}
    </div>
  {/if}

  {#if health.lastResults.length && !disabled}
    <div class="outcome" role="status"><CheckCheck size={16} /><span>{lastSucceeded} completed{lastFailed ? ` · ${lastFailed} need attention` : " · Changes applied"}</span>{#if failedCount}<button class="btn small" onclick={() => health.retryFailed()}>Retry failed ({failedCount})</button>{/if}</div>
  {/if}

  {#if showRestore}
    <section class="history surface" aria-label="Change history">
      <h3>Change history</h3>
      {#each health.restore as entry (entry.id)}
        <div class="history-row">
          <div><strong>{entry.title}</strong><small>{moduleTitle(entry.module)} · {new Date(entry.at).toLocaleString()}</small></div>
          {#if entry.undo.kind === "recycled"}<span class="muted">Restore from Windows Recycle Bin</span>
          {:else}<button class="btn small" disabled={disabled} onclick={() => health.undo(entry)}><RotateCcw size={13} /> Restore</button>{/if}
        </div>
      {/each}
    </section>
  {/if}

  <div class="workspace surface">
    <aside class="categories" aria-label="Software categories">
      {#each [{ id: "cleanup", title: "Cleanup", icon: FolderMinus }, { id: "controls", title: "App controls", icon: SlidersHorizontal }] as group (group.id)}
        <button class="group" class:active={category === group.id} aria-pressed={category === group.id} onclick={() => category = group.id}><group.icon size={17} /><strong>{group.title}</strong><span>{count(group.id)}</span></button>
        {#each MODULES.filter((m) => CLEANUP_MODULES.has(m.id) === (group.id === "cleanup")) as m (m.id)}
          <button class="category" class:active={category === m.id} aria-pressed={category === m.id} onclick={() => category = m.id}>
            <span>{m.title}</span><small class:error={!!health.reports[m.id]?.error}>{health.running.has(m.id) ? "…" : health.reports[m.id]?.error ? "!" : health.reports[m.id] ? count(m.id) : "—"}</small>
          </button>
        {/each}
      {/each}
    </aside>

    <div class="results">
      <div class="mobile-category">
        <label>Category <select class="input" bind:value={category}><option value="cleanup">All cleanup results</option>{#each MODULES.filter((m) => CLEANUP_MODULES.has(m.id)) as m}<option value={m.id}>{m.title}</option>{/each}<option value="controls">All app controls</option>{#each MODULES.filter((m) => !CLEANUP_MODULES.has(m.id)) as m}<option value={m.id}>{m.title}</option>{/each}</select></label>
      </div>
      <div class="search-row">
        <label class="search"><Search size={16} /><input class="input" type="search" bind:value={query} aria-label="Search software results" placeholder="Search apps or paths…" /></label>
        <select class="input" aria-label="Sort results" bind:value={sort}><option value="name">Name</option><option value="size">Largest first</option></select>
      </div>
      <div class="filters" aria-label="Result filters">
        {#each filters as f}<button class="chip" class:active={filter === f.id} aria-pressed={filter === f.id} onclick={() => filter = f.id}>{f.label}</button>{/each}
      </div>
      <div class="list-tools">
        <span class="muted" tabindex="-1" bind:this={resultsSummary} aria-label={pageCount > 1 ? `${rows.length} results, page ${currentPage} of ${pageCount}` : undefined}>{rows.length} result{rows.length === 1 ? "" : "s"}</span>
        <button class="text-btn" disabled={disabled || !rows.some((f) => f.recommended)} onclick={chooseRecommended}>Select recommended</button>
        <button class="text-btn" disabled={disabled || !rows.some((f) => f.actions.length)} onclick={() => health.selected = selectResults(health.selected, rows)}>Select all results</button>
        <button class="text-btn" disabled={disabled || !health.selected.size} onclick={() => health.selected = new Set()}>Clear selection</button>
      </div>
      {#if categoryError}<p class="error" role="alert">{categoryError}</p>{/if}
      {#if category === "cleanup" || category === "controls"}
        {#each MODULES.filter((m) => CLEANUP_MODULES.has(m.id) === (category === "cleanup") && health.reports[m.id]?.error) as m}<p class="error" role="alert">{m.title}: {health.reports[m.id].error}</p>{/each}
      {/if}

      {#if !rows.length}
        <div class="empty">
          <Radar size={28} />
          <h3>{!Object.keys(health.reports).length ? "Start with a software scan" : query || filter !== "all" ? "No matching results" : "Nothing to review here"}</h3>
          <p>{!Object.keys(health.reports).length ? "Find leftovers and app settings, then review your selections together." : "Choose another category or change your filters."}</p>
        </div>
      {:else}
        <ul class="list" aria-label="Software results">
          {#each visibleRows as f (f.id)}
            <li class:selected={health.selected.has(f.id)} class:failed={!!health.failures[f.id]}>
              <div class="finding">
                <input type="checkbox" aria-label="Select {f.title}{f.module === 'permissions' && f.flags[0] ? `: ${f.flags[0]}` : ''}" disabled={disabled || !f.actions.length} checked={health.selected.has(f.id)} onchange={() => health.toggle(f.id)} />
                {#if f.module === "permissions"}{@const PermissionIcon = permissionIcons[f.flags[0]] ?? AppWindow}<span class="permission-icon"><PermissionIcon size={18} /></span>{/if}
                <div class="summary">
                  <div class="title-line"><strong>{f.title}</strong>{#if f.module === "permissions" && f.flags[0]}<span class="tag capability">{f.flags[0]}</span>{/if}{#if f.recommended}<span class="tag recommended">Recommended</span>{:else if f.actions.length && f.cleanupAction}<span class="tag review">Needs review</span>{/if}{#if f.enabled !== null}<span class="tag">{f.module === "permissions" ? f.enabled ? "Allowed" : "Blocked" : f.enabled ? "On" : "Off"}</span>{/if}</div>
                  <p>{f.detail}</p>
                  {#if health.failures[f.id]}<p class="error" role="status">{health.failures[f.id]}</p>{/if}
                </div>
                <div class="meta">{#if f.size !== null}<strong>{formatSize(f.size)}</strong>{/if}{#if f.module === "permissions" && f.actions.length === 1}<button class="btn small" disabled={disabled} onclick={() => health.applyBatch([{ id: f.id, action: f.actions[0] }])}>{ACTION_LABELS[f.actions[0]]}</button>{:else}<small>{f.cleanupAction ? ACTION_LABELS[health.overrides[f.id] ?? f.cleanupAction] : !f.actions.length ? "Information only" : f.actions.map((a) => ACTION_LABELS[a]).join(" / ")}</small>{/if}</div>
              </div>
              <details class="details">
                <summary>Details{f.actions.includes("forceClean") ? " & cleanup options" : ""}</summary>
                <div>
                  {#if f.path}<code>{f.path}</code>{/if}
                  {#if f.reviewReason}<p>{f.reviewReason}</p>{/if}
                  {#if f.flags.length}<p>{f.flags.join(" · ")}</p>{/if}
                  {#if f.cleanupAction && f.actions.includes("forceClean")}
                    <label>Cleanup action <select class="input" disabled={disabled} value={health.overrides[f.id] ?? f.cleanupAction} onchange={(e) => health.overrides = { ...health.overrides, [f.id]: e.currentTarget.value as HealthAction }}><option value="deleteKey">Remove registry entry only (backup first)</option><option value="forceClean">Force clean: entry and remaining folders</option></select></label>
                    <p>Force clean moves remaining installation folders to the Recycle Bin.</p>
                  {/if}
                </div>
              </details>
            </li>
          {/each}
        </ul>
        {#if pageCount > 1}<div class="pagination"><button class="btn small" disabled={currentPage === 1} onclick={() => changePage(currentPage - 1)}>Previous</button><span>Page {currentPage} of {pageCount}</span><button class="btn small" disabled={currentPage === pageCount} onclick={() => changePage(currentPage + 1)}>Next</button></div>{/if}
      {/if}
    </div>
  </div>

  <footer class="selection surface" aria-label="Selected software actions">
    <div><strong>{health.selected.size} selected</strong><span>{selectedBytes ? formatSize(selectedBytes) : "Across all categories and filters"}</span>{#if health.selected.size > health.picked.length}<span>{health.selected.size - health.picked.length} unavailable after a scan error · scan again to review</span>{/if}</div>
    <div class="actions">
      {#each controlActions as choice}<button class="btn" disabled={disabled} onclick={() => controlAction(choice.action)}>{ACTION_LABELS[choice.action]} ({choice.count})</button>{/each}
      {#if failedCount}<button class="btn" disabled={disabled} onclick={() => health.retryFailed()}>Retry failed ({failedCount})</button>{/if}
      <button class="btn primary" disabled={disabled || !plan.length} onclick={() => health.applyBatch(plan)}><CheckCheck size={15} /> Review cleanup ({plan.length})</button>
    </div>
  </footer>
</section>

<style>
  .health { display: grid; gap: 16px; min-width: 0; }
  .overview, .actions, .search-row, .filters, .list-tools, .history-row, .outcome, .pagination { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .overview { justify-content: space-between; gap: 16px; }
  h2 { font-size: var(--fluid-lg); font-weight: 600; }
  .overview p, .muted, .empty p { color: var(--text-2); font-size: var(--fluid-sm); }
  .overview p { margin-top: 4px; max-width: 70ch; }
  .work { display: grid; gap: 8px; padding: 14px 18px; }
  .work span { display: flex; gap: 16px; font-size: var(--fluid-sm); }
  .work b { margin-left: auto; font-variant-numeric: tabular-nums; }
  .outcome { color: var(--ok); padding: 0 4px; font-size: var(--fluid-sm); }
  .history { padding: 18px; }
  .history-row { justify-content: space-between; padding: 10px 0; border-bottom: 1px solid var(--divider); }
  .history-row div { min-width: 0; flex: 1; }
  .history-row strong, .history-row small { display: block; overflow-wrap: anywhere; }
  .history-row small { color: var(--text-3); margin-top: 4px; }
  .workspace { display: grid; grid-template-columns: 225px minmax(0, 1fr); overflow: hidden; }
  .categories { padding: 14px 10px; background: rgb(0 0 0 / .08); border-right: 1px solid var(--divider); }
  .group, .category { display: flex; align-items: center; gap: 9px; width: 100%; min-height: 38px; padding: 8px 10px; border: 0; border-radius: 8px; background: transparent; color: var(--text-2); font: inherit; text-align: left; cursor: pointer; }
  .group { color: var(--text-1); margin: 4px 0; }
  .group:nth-of-type(6) { margin-top: 24px; }
  .group :global(svg) { color: #7dd3fc; flex: none; }
  .group:nth-of-type(6) :global(svg) { color: #c4b5fd; }
  .group span, .category small { margin-left: auto; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .category { padding-left: 36px; font-size: var(--fluid-xs); }
  .category span { flex: 1; }
  .category:hover, .group:hover { background: var(--hover); }
  .category.active, .group.active { background: var(--selected); color: var(--text-1); }
  .results { min-width: 0; padding: 18px; }
  .search-row { flex-wrap: nowrap; }
  .search { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; }
  .search :global(svg) { color: var(--text-3); flex: none; }
  .search input { width: 100%; min-width: 0; }
  .filters { margin-top: 14px; gap: 6px; }
  .list-tools { margin: 16px 0 10px; gap: 12px; }
  .list-tools .muted { margin-right: auto; }
  .text-btn { border: 0; background: transparent; color: var(--text-2); font: inherit; font-size: var(--fluid-xs); cursor: pointer; padding: 4px 0; }
  .text-btn:hover { color: var(--text-1); }
  .text-btn:disabled { opacity: .4; cursor: default; }
  .list { list-style: none; padding: 0; margin: 0; }
  li { padding: 12px 10px; border-top: 1px solid var(--divider); }
  li.selected { background: rgb(var(--accent-rgb) / .055); }
  .finding { display: flex; gap: 12px; align-items: flex-start; }
  .finding > input { flex: none; margin-top: 3px; }
  .permission-icon { display: grid; place-items: center; flex: none; width: 32px; height: 32px; border-radius: 9px; color: #a8c9ff; background: rgb(168 201 255 / .09); box-shadow: inset 0 1px 0 rgb(255 255 255 / .09); }
  .tag.capability { color: #a8c9ff; }
  .summary { flex: 1; min-width: 0; }
  .title-line { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .title-line strong { font-size: var(--fluid-sm); font-weight: 600; overflow-wrap: anywhere; }
  .summary p { font-size: var(--fluid-xs); color: var(--text-2); margin-top: 4px; line-height: 1.45; overflow-wrap: anywhere; }
  .tag { font-size: 11px; color: var(--text-3); background: rgb(255 255 255 / .05); border-radius: 5px; padding: 2px 5px; }
  .tag.recommended { color: #98dfbd; background: rgb(62 207 142 / .09); }
  .tag.review { color: #f5c26b; background: rgb(245 165 36 / .08); }
  .meta { display: grid; gap: 4px; text-align: right; flex: 0 1 145px; }
  .meta strong { font-size: var(--fluid-sm); font-variant-numeric: tabular-nums; }
  .meta small { font-size: var(--fluid-xs); color: var(--text-3); }
  .details { margin: 7px 0 0 28px; font-size: var(--fluid-xs); color: var(--text-3); }
  .details summary { cursor: pointer; width: fit-content; }
  .details > div { display: grid; gap: 7px; margin-top: 10px; color: var(--text-2); }
  code { overflow-wrap: anywhere; white-space: normal; font-size: var(--fluid-xs); }
  .details label { display: grid; gap: 6px; max-width: 450px; }
  .error, .summary p.error { color: #ffb4b0; font-size: var(--fluid-xs); }
  .empty { display: grid; justify-items: center; text-align: center; gap: 10px; padding: 55px 20px; }
  .empty :global(svg) { color: var(--accent); }
  .pagination { justify-content: flex-end; margin-top: 12px; font-size: var(--fluid-xs); color: var(--text-2); }
  .selection { position: sticky; bottom: 0; display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 12px 18px; background: var(--bg-1); z-index: 2; }
  .selection > div:first-child { display: grid; gap: 3px; }
  .selection strong { font-size: var(--fluid-sm); }
  .selection span { font-size: var(--fluid-xs); color: var(--text-3); }
  .selection .actions { justify-content: flex-end; }
  .mobile-category { display: none; }
  @media (max-width: 1000px) {
    .workspace { grid-template-columns: minmax(0, 1fr); }
    .categories { display: none; }
    .mobile-category { display: block; margin-bottom: 12px; }
    .mobile-category label { display: grid; gap: 5px; color: var(--text-2); font-size: var(--fluid-xs); }
  }
  @media (max-width: 600px) {
    .results { padding: 12px; }
    .meta { flex-basis: 85px; }
    .search-row { flex-wrap: wrap; }
    .search { flex-basis: 100%; }
    .selection .actions { width: 100%; justify-content: flex-start; }
  }
</style>
