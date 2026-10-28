<script lang="ts">
  import { onMount, tick } from "svelte";
  import Radar from "@lucide/svelte/icons/radar";
  import History from "@lucide/svelte/icons/history";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Search from "@lucide/svelte/icons/search";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import FolderMinus from "@lucide/svelte/icons/folder-minus";
  import FolderX from "@lucide/svelte/icons/folder-x";
  import FolderDown from "@lucide/svelte/icons/folder-down";
  import Folder from "@lucide/svelte/icons/folder";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Camera from "@lucide/svelte/icons/camera";
  import Mic from "@lucide/svelte/icons/mic";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import AppWindow from "@lucide/svelte/icons/app-window";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Info from "@lucide/svelte/icons/info";
  import Copy from "@lucide/svelte/icons/copy";
  import Clock from "@lucide/svelte/icons/clock";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Trash from "@lucide/svelte/icons/trash";
  import Recycle from "@lucide/svelte/icons/recycle";
  import Bell from "@lucide/svelte/icons/bell";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import Select, { type SelectOption } from "../../../lib/components/Select.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import { formatSize } from "./api";
  import { cleanerState } from "./state.svelte";
  import { ACTION_LABELS, MODULES, health, type HealthAction } from "./health.svelte";
  import { CLEANUP_MODULES, cleanupPlan, compatibleActions, filterFindings, selectResults, type ListFilter } from "./health-model";

  let category = $state("cleanup");
  let query = $state("");
  let filter = $state<ListFilter>("all");
  let sort = $state<"name" | "size">("name");

  const sortOptions: readonly SelectOption<"name" | "size">[] = [
    { value: "name", label: "Name" },
    { value: "size", label: "Largest first" },
  ];

  const categoryOptions = $derived<readonly SelectOption<string>[]>([
    { value: "cleanup", label: "All cleanup results" },
    ...MODULES.filter((m) => CLEANUP_MODULES.has(m.id)).map((m) => ({ value: m.id, label: m.title })),
    { value: "controls", label: "All app controls" },
    ...MODULES.filter((m) => !CLEANUP_MODULES.has(m.id)).map((m) => ({ value: m.id, label: m.title })),
  ]);

  const cleanupActionOptions: readonly SelectOption<HealthAction>[] = [
    { value: "deleteKey", label: "Remove registry entry only (backup first)" },
    { value: "forceClean", label: "Force clean: entry and remaining folders" },
  ];
  let showRestore = $state(false);
  let page = $state(1);
  let copiedId = $state<string | null>(null);
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
  const moduleIcons: Record<string, typeof FolderX> = {
    leftovers: FolderX,
    broken: CircleAlert,
    redundant: Trash,
    hibernation: SlidersHorizontal,
    permissions: AppWindow,
    notifications: Bell,
    installers: FolderDown,
    suspicious: ShieldAlert,
  };

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

  async function copyPath(path: string | null, id: string) {
    if (!path) return;
    try {
      await navigator.clipboard.writeText(path);
      copiedId = id;
      setTimeout(() => {
        if (copiedId === id) copiedId = null;
      }, 2000);
      toast.info("Path copied to clipboard");
    } catch {
      // ignore
    }
  }

  async function openInExplorer(path: string | null) {
    if (!path) return;
    try {
      const { revealItemInDir, openPath } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(path).catch(() => openPath(path));
    } catch {
      toast.error("Could not open in File Explorer");
    }
  }

  function extractIdleDays(detail: string): string | null {
    const match = detail.match(/(\d+)\s+days?/i);
    return match ? `${match[1]}d idle` : null;
  }

  function isEmptyFolder(detail: string): boolean {
    return /is empty/i.test(detail);
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
        <label>Category
          <Select bind:value={category} options={categoryOptions} ariaLabel="Category" fullWidth />
        </label>
      </div>
      <div class="search-row">
        <label class="search"><Search size={16} /><input class="input" type="search" bind:value={query} aria-label="Search software results" placeholder="Search apps or paths…" /></label>
        <Select bind:value={sort} options={sortOptions} ariaLabel="Sort results" minWidth="135px" align="end" />
      </div>
      <div class="filters" aria-label="Result filters">
        {#each filters as f}<button class="chip" class:active={filter === f.id} aria-pressed={filter === f.id} onclick={() => filter = f.id}>{f.label}</button>{/each}
      </div>

      <div class="list-tools">
        <div class="tools-left">
          <span class="results-badge" tabindex="-1" bind:this={resultsSummary} aria-label={pageCount > 1 ? `${rows.length} results, page ${currentPage} of ${pageCount}` : undefined}>
            <strong>{rows.length}</strong> {rows.length === 1 ? "result" : "results"}
          </span>
          {#if health.selected.size}
            <span class="selection-pill">
              <Check size={12} /> {health.selected.size} selected
            </span>
          {/if}
        </div>
        <div class="tools-actions">
          {#if rows.some((f) => f.recommended)}
            <button class="tool-btn recommend-btn" disabled={disabled} onclick={chooseRecommended}>
              <Sparkles size={13} /> Select recommended
            </button>
          {/if}
          <button class="tool-btn" disabled={disabled || !rows.some((f) => f.actions.length)} onclick={() => health.selected = selectResults(health.selected, rows)}>
            <CheckCheck size={13} /> Select all results
          </button>
          {#if health.selected.size}
            <button class="tool-btn clear-btn" disabled={disabled} onclick={() => health.selected = new Set()}>
              <X size={13} /> Clear selection
            </button>
          {/if}
        </div>
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
            {@const isSelected = health.selected.has(f.id)}
            {@const hasFailure = !!health.failures[f.id]}
            {@const PermissionIcon = f.module === "permissions" ? (permissionIcons[f.flags[0]] ?? AppWindow) : null}
            {@const ModuleIcon = PermissionIcon ?? (moduleIcons[f.module] ?? FolderX)}
            <li class="card-item" class:selected={isSelected} class:failed={hasFailure}>
              <div class="card-top">
                <label class="checkbox-wrap">
                  <input
                    type="checkbox"
                    aria-label="Select {f.title}{f.module === 'permissions' && f.flags[0] ? `: ${f.flags[0]}` : ''}"
                    disabled={disabled || !f.actions.length}
                    checked={isSelected}
                    onchange={() => health.toggle(f.id)}
                  />
                </label>

                <div class="icon-bubble module-{f.module}">
                  <ModuleIcon size={17} strokeWidth={1.8} />
                </div>

                <div class="card-main">
                  <div class="card-header-row">
                    <div class="title-group">
                      <strong class="item-title">{f.title}</strong>
                      {#if f.module === "permissions" && f.flags[0]}
                        <span class="tag capability">{f.flags[0]}</span>
                      {/if}
                      {#if f.recommended}
                        <span class="tag recommended"><Sparkles size={11} /> Recommended</span>
                      {:else if f.actions.length && f.cleanupAction}
                        <span class="tag review">Needs review</span>
                      {/if}
                      {#if f.enabled !== null}
                        <span class="tag status" class:active-status={f.enabled}>
                          {f.module === "permissions" ? (f.enabled ? "Allowed" : "Blocked") : (f.enabled ? "On" : "Off")}
                        </span>
                      {/if}
                      {#if isEmptyFolder(f.detail)}
                        <span class="tag subtle">Empty</span>
                      {/if}
                      {#if extractIdleDays(f.detail)}
                        <span class="tag subtle"><Clock size={11} /> {extractIdleDays(f.detail)}</span>
                      {/if}
                    </div>

                    <div class="meta-col">
                      {#if f.module === "permissions" && f.actions.length === 1}
                        <button class="btn small action-trigger" disabled={disabled} onclick={() => health.applyBatch([{ id: f.id, action: f.actions[0] }])}>
                          {ACTION_LABELS[f.actions[0]]}
                        </button>
                      {:else}
                        <span class="action-badge">
                          {#if f.cleanupAction === "recycle" || f.actions.includes("recycle")}
                            <Recycle size={11} />
                          {/if}
                          <small>{f.cleanupAction ? ACTION_LABELS[health.overrides[f.id] ?? f.cleanupAction] : (!f.actions.length ? "Information only" : f.actions.map((a) => ACTION_LABELS[a]).join(" / "))}</small>
                        </span>
                      {/if}
                      {#if f.size !== null}
                        <strong class="size-text">{formatSize(f.size)}</strong>
                      {/if}
                    </div>
                  </div>

                  <p class="detail-text">{f.detail}</p>

                  {#if hasFailure}
                    <div class="failure-callout" role="status">
                      <CircleAlert size={15} class="failure-icon" />
                      <div class="failure-body">
                        <strong class="failure-title">{health.failures[f.id]}</strong>
                        {#if health.failures[f.id].includes("code 120") || health.failures[f.id].toLowerCase().includes("in use")}
                          <p class="failure-sub">This folder may require Administrator privileges, or some of its files are locked by a Windows service.</p>
                        {/if}
                      </div>
                    </div>
                  {/if}

                  <details class="card-details">
                    <summary class="details-summary">
                      <ChevronRight size={13} class="disclosure-arrow" />
                      <span>Details{f.actions.includes("forceClean") ? " & cleanup options" : ""}</span>
                    </summary>

                    <div class="details-pane">
                      {#if f.path}
                        <div class="path-box">
                          <Folder size={14} class="path-folder-icon" />
                          <code class="path-code" title={f.path}>{f.path}</code>
                          <div class="path-actions">
                            <button type="button" class="mini-btn" title="Copy path" onclick={() => copyPath(f.path, f.id)}>
                              {#if copiedId === f.id}
                                <Check size={13} class="success-color" />
                                <span class="btn-text">Copied</span>
                              {:else}
                                <Copy size={13} />
                                <span class="btn-text">Copy</span>
                              {/if}
                            </button>
                            <button type="button" class="mini-btn" title="Open in File Explorer" onclick={() => openInExplorer(f.path)}>
                              <ExternalLink size={13} />
                              <span class="btn-text">Open</span>
                            </button>
                          </div>
                        </div>
                      {/if}

                      {#if f.reviewReason}
                        <div class="review-box">
                          <Info size={14} class="info-icon" />
                          <p>{f.reviewReason}</p>
                        </div>
                      {/if}

                      {#if f.flags.length}
                        <div class="flags-row">
                          {#each f.flags as flag}
                            <span class="flag-chip">
                              {#if flag.toLowerCase().includes("admin")}
                                <ShieldAlert size={11} />
                              {/if}
                              {flag}
                            </span>
                          {/each}
                        </div>
                      {/if}

                      {#if f.cleanupAction && f.actions.includes("forceClean")}
                        <div class="cleanup-options-box">
                          <label class="cleanup-label">Cleanup action
                            <Select
                              value={(health.overrides[f.id] ?? f.cleanupAction) as HealthAction}
                              options={cleanupActionOptions}
                              disabled={disabled}
                              ariaLabel="Cleanup action"
                              fullWidth
                              onchange={(val) => {
                                health.overrides = { ...health.overrides, [f.id]: val };
                              }}
                            />
                          </label>
                          <p class="cleanup-hint">Force clean moves remaining installation folders to the Recycle Bin.</p>
                        </div>
                      {/if}
                    </div>
                  </details>
                </div>
              </div>
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
  .workspace { display: grid; grid-template-columns: 225px minmax(0, 1fr); overflow: hidden; border-radius: 14px; }
  .categories { padding: 14px 10px; background: rgb(0 0 0 / .08); border-right: 1px solid var(--divider); }
  .group, .category { display: flex; align-items: center; gap: 9px; width: 100%; min-height: 38px; padding: 8px 10px; border: 0; border-radius: 8px; background: transparent; color: var(--text-2); font: inherit; text-align: left; cursor: pointer; transition: background var(--dur-fast), color var(--dur-fast); }
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

  /* Toolbar */
  .list-tools { margin: 16px 0 12px; display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .tools-left { display: flex; align-items: center; gap: 8px; }
  .results-badge { font-size: var(--fluid-xs); color: var(--text-2); font-variant-numeric: tabular-nums; }
  .results-badge strong { color: var(--text-1); }
  .selection-pill { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; font-weight: 600; color: var(--accent); background: rgb(var(--accent-rgb) / .12); border: 1px solid rgb(var(--accent-rgb) / .28); border-radius: 6px; padding: 2px 7px; }
  .tools-actions { display: flex; align-items: center; gap: 8px; margin-left: auto; flex-wrap: wrap; }
  .tool-btn { display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--glass-border); border-radius: 7px; background: rgb(255 255 255 / .04); color: var(--text-2); font: inherit; font-size: var(--fluid-xs); padding: 4px 10px; cursor: pointer; transition: background var(--dur-fast), color var(--dur-fast), border-color var(--dur-fast); }
  .tool-btn:hover:not(:disabled) { background: rgb(255 255 255 / .09); color: var(--text-1); border-color: rgb(255 255 255 / .18); }
  .tool-btn:disabled { opacity: .4; cursor: default; }
  .recommend-btn { color: #a7f3d0; border-color: rgb(16 185 129 / .25); background: rgb(16 185 129 / .08); }
  .recommend-btn:hover:not(:disabled) { background: rgb(16 185 129 / .15); color: #ecfdf5; border-color: rgb(16 185 129 / .4); }
  .clear-btn:hover:not(:disabled) { color: #fca5a5; border-color: rgb(239 68 68 / .3); background: rgb(239 68 68 / .08); }

  /* Cards List */
  .list { display: grid; gap: 8px; list-style: none; padding: 0; margin: 0; }
  .card-item { display: flex; flex-direction: column; padding: 11px 14px; border: 1px solid var(--glass-border); border-radius: 12px; background: var(--surface-fill); box-shadow: var(--surface-depth); transition: border-color var(--dur-fast), background var(--dur-fast), box-shadow var(--dur-fast); }
  .card-item:hover { border-color: rgb(255 255 255 / .15); background: rgb(255 255 255 / .03); }
  .card-item.selected { border-color: rgb(var(--accent-rgb) / .45); background: rgb(var(--accent-rgb) / .07); box-shadow: 0 0 0 1px rgb(var(--accent-rgb) / .2), var(--surface-depth); }
  .card-item.failed { border-color: rgb(229 72 77 / .42); background: rgb(229 72 77 / .035); }

  /* Card Head */
  .card-top { display: flex; align-items: flex-start; gap: 10px; }
  .checkbox-wrap { display: flex; align-items: center; justify-content: center; height: 30px; width: 18px; cursor: pointer; flex: none; }
  .checkbox-wrap input { cursor: pointer; width: 16px; height: 16px; accent-color: var(--accent); margin: 0; }

  /* Icon Bubble */
  .icon-bubble { display: grid; place-items: center; width: 30px; height: 30px; flex: none; border-radius: 8px; box-shadow: inset 0 1px 0 rgb(255 255 255 / .1); }
  .module-leftovers { color: #fb923c; background: rgb(251 146 60 / .12); }
  .module-broken { color: #f87171; background: rgb(248 113 113 / .12); }
  .module-redundant { color: #38bdf8; background: rgb(56 189 248 / .12); }
  .module-hibernation { color: #c084fc; background: rgb(192 132 252 / .12); }
  .module-permissions { color: #60a5fa; background: rgb(96 165 250 / .12); }
  .module-notifications { color: #fbbf24; background: rgb(251 191 36 / .12); }
  .module-installers { color: #34d399; background: rgb(52 211 153 / .12); }
  .module-suspicious { color: #f43f5e; background: rgb(244 63 94 / .12); }

  /* Card Body */
  .card-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 5px; }
  .card-header-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 30px; }
  .title-group { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .item-title { font-size: var(--fluid-sm); font-weight: 600; color: var(--text-1); line-height: 1.25; overflow-wrap: anywhere; }

  /* Badges & Tags */
  .tag { font-size: 11px; color: var(--text-3); background: rgb(255 255 255 / .05); border: 1px solid rgb(255 255 255 / .07); border-radius: 5px; padding: 2px 6px; line-height: 1.2; display: inline-flex; align-items: center; gap: 3px; }
  .tag.capability { color: #93c5fd; background: rgb(147 197 253 / .1); border-color: rgb(147 197 253 / .22); }
  .tag.recommended { color: #6ee7b7; background: rgb(16 185 129 / .12); border-color: rgb(16 185 129 / .26); font-weight: 600; }
  .tag.review { color: #fbbf24; background: rgb(245 158 11 / .12); border-color: rgb(245 158 11 / .26); font-weight: 600; }
  .tag.status.active-status { color: #6ee7b7; background: rgb(16 185 129 / .1); }
  .tag.subtle { color: var(--text-3); background: rgb(255 255 255 / .04); }

  /* Right Meta */
  .meta-col { display: flex; align-items: center; gap: 8px; flex: none; text-align: right; }
  .size-text { font-size: var(--fluid-sm); font-weight: 600; font-variant-numeric: tabular-nums; color: var(--text-1); min-width: 45px; text-align: right; }
  .action-badge { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text-3); background: rgb(255 255 255 / .04); padding: 2px 7px; border-radius: 5px; border: 1px solid rgb(255 255 255 / .07); white-space: nowrap; height: 22px; }
  .action-badge :global(svg) { color: var(--accent); }
  .action-badge small { font-size: 11px; color: inherit; }

  .detail-text { font-size: var(--fluid-xs); color: var(--text-2); line-height: 1.45; margin: 0; overflow-wrap: anywhere; }

  /* Failure Callout */
  .failure-callout { display: flex; align-items: flex-start; gap: 8px; padding: 7px 10px; margin-top: 2px; border-radius: 7px; background: rgb(229 72 77 / .09); border: 1px solid rgb(229 72 77 / .26); color: #ff8589; }
  .failure-icon { flex: none; margin-top: 2px; color: #ff8589; }
  .failure-body { font-size: var(--fluid-xs); display: grid; gap: 2px; }
  .failure-title { font-weight: 600; line-height: 1.3; color: #ff9da1; font-size: 12px; }
  .failure-sub { color: rgb(255 255 255 / .7); font-size: 11px; margin: 0; line-height: 1.35; }

  /* Details Accordion */
  .card-details { margin-top: 2px; font-size: var(--fluid-xs); }
  .details-summary { display: inline-flex; align-items: center; gap: 4px; color: var(--text-3); cursor: pointer; user-select: none; list-style: none; font-size: 11.5px; padding: 2px 6px; border-radius: 4px; transition: color var(--dur-fast), background var(--dur-fast); width: fit-content; }
  .details-summary::-webkit-details-marker { display: none; }
  .details-summary:hover { color: var(--text-1); background: rgb(255 255 255 / .05); }
  .disclosure-arrow { transition: transform var(--dur-fast); flex: none; }
  :global(.card-details[open]) .disclosure-arrow { transform: rotate(90deg); }

  .details-pane { display: grid; gap: 7px; margin-top: 6px; padding: 2px 0 0; }

  /* Path Box */
  .path-box { display: flex; align-items: center; gap: 8px; background: rgb(0 0 0 / .3); border: 1px solid rgb(255 255 255 / .08); border-radius: 8px; padding: 6px 10px; font-family: var(--font-mono); }
  .path-folder-icon { color: #7dd3fc; flex: none; }
  .path-code { flex: 1; min-width: 0; font-size: 12px; color: var(--text-1); overflow-wrap: anywhere; }
  .path-actions { display: flex; align-items: center; gap: 6px; flex: none; }
  .mini-btn { display: inline-flex; align-items: center; gap: 4px; border: 1px solid rgb(255 255 255 / .1); background: rgb(255 255 255 / .06); color: var(--text-2); font: inherit; font-size: 11px; border-radius: 6px; padding: 3px 8px; cursor: pointer; transition: all var(--dur-fast); }
  .mini-btn:hover { background: rgb(255 255 255 / .12); color: var(--text-1); border-color: rgb(255 255 255 / .2); }
  .btn-text { font-size: 11px; }
  .success-color { color: var(--ok); }

  /* Review Reason */
  .review-box { display: flex; align-items: flex-start; gap: 8px; background: rgb(255 255 255 / .03); border: 1px solid rgb(255 255 255 / .06); border-radius: 8px; padding: 7px 11px; color: var(--text-2); font-size: 12px; line-height: 1.4; }
  .info-icon { color: #7dd3fc; flex: none; margin-top: 2px; }
  .review-box p { margin: 0; }

  /* Flags */
  .flags-row { display: flex; flex-wrap: wrap; gap: 6px; }
  .flag-chip { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text-2); background: rgb(255 255 255 / .06); border: 1px solid rgb(255 255 255 / .08); border-radius: 6px; padding: 2px 7px; }
  .flag-chip :global(svg) { color: #fbbf24; }

  .cleanup-options-box { display: grid; gap: 6px; margin-top: 4px; }
  .cleanup-label { display: grid; gap: 6px; max-width: 450px; color: var(--text-2); font-size: var(--fluid-xs); }
  .cleanup-hint { color: var(--text-3); font-size: 11px; margin: 0; }

  .error { color: #ffb4b0; font-size: var(--fluid-xs); }
  .empty { display: grid; justify-items: center; text-align: center; gap: 10px; padding: 55px 20px; }
  .empty :global(svg) { color: var(--accent); }
  .pagination { justify-content: flex-end; margin-top: 12px; font-size: var(--fluid-xs); color: var(--text-2); }
  .selection { position: sticky; bottom: 0; display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 12px 18px; background: var(--bg-1); z-index: 2; border-top: 1px solid var(--glass-border); box-shadow: 0 -4px 20px rgb(0 0 0 / .35); }
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
    .meta-col { flex-basis: 85px; }
    .search-row { flex-wrap: wrap; }
    .search { flex-basis: 100%; }
    .selection .actions { width: 100%; justify-content: flex-start; }
    .tools-actions { width: 100%; margin-left: 0; }
  }
</style>