<script lang="ts">
  import { onMount } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import HeartPulse from "@lucide/svelte/icons/heart-pulse";
  import History from "@lucide/svelte/icons/history";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RadarIcon from "@lucide/svelte/icons/radar";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { formatSize } from "./api";
  import { ACTION_LABELS, MODULES, health, type Finding, type HealthAction, type HealthModule } from "./health.svelte";

  let open = $state<Record<string, boolean>>({});
  let showRestore = $state(false);

  onMount(() => health.loadRestore());

  const pct = $derived(health.total ? Math.round((health.done / health.total) * 100) : 0);
  const moduleTitle = (id: string) => MODULES.find((m) => m.id === id)?.title ?? id;

  function findings(id: HealthModule): Finding[] {
    return health.reports[id]?.findings ?? [];
  }
  function chosen(id: HealthModule): Finding[] {
    return findings(id).filter((f) => health.selected.has(f.id));
  }
  /** Actions every selected item offers. */
  function common(list: Finding[]): HealthAction[] {
    if (!list.length) return [];
    return list[0].actions.filter((a) => list.every((f) => f.actions.includes(a)));
  }
  function allIn(id: HealthModule) {
    const list = findings(id).filter((f) => f.actions.length);
    const every = list.every((f) => health.selected.has(f.id));
    const next = new Set(health.selected);
    for (const f of list) every ? next.delete(f.id) : next.add(f.id);
    health.selected = next;
  }
  const when = (ms: number) => new Date(ms).toLocaleString();
</script>

<section class="health surface">
  <header>
    <div class="title">
      <HeartPulse size={16} />
      <div>
        <h2>Software Health</h2>
        <p>Leftovers, startup load, permissions and notifications of installed software. Nothing changes without a preview; every change can be undone.</p>
      </div>
    </div>
    <div class="actions">
      <button class="btn" disabled={!health.restore.length} onclick={() => (showRestore = !showRestore)}>
        <History size={14} /> Restore ({health.restore.length})
      </button>
      <button class="btn primary" disabled={health.scanning || health.busy} onclick={() => health.scan()}>
        {#if health.scanning}<LoaderCircle size={14} class="spin" />{:else}<RadarIcon size={14} />{/if}
        Scan software health
      </button>
    </div>
  </header>

  {#if health.scanning}
    <div class="progress" aria-live="polite">
      <span>Scanning {moduleTitle(health.current)}… {health.done}/{health.total}</span>
      <ProgressBar value={pct} label="Software Health scan" />
    </div>
  {/if}

  {#if showRestore}
    <div class="restore">
      {#each health.restore as entry (entry.id)}
        <div class="row">
          <span class="grow"><strong>{entry.title}</strong> <em>{moduleTitle(entry.module)} · {when(entry.at)}</em></span>
          {#if entry.undo.kind === "recycled"}
            <span class="muted">In the Recycle Bin</span>
          {:else}
            <button class="btn small" onclick={() => health.undo(entry)}><RotateCcw size={12} /> Restore</button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <div class="modules">
    {#each MODULES as m (m.id)}
      {@const list = findings(m.id)}
      {@const report = health.reports[m.id]}
      {@const picked = chosen(m.id)}
      <div class="module" class:open={open[m.id]}>
        <button class="head" onclick={() => (open[m.id] = !open[m.id])} aria-expanded={!!open[m.id]}>
          <ChevronRight size={14} class="chev" />
          <span class="name">{m.title}</span>
          <span class="hint">{m.hint}</span>
          {#if report}
            <span class="badge" class:clean={!list.length}>{list.length ? list.length : "OK"}</span>
          {:else}
            <span class="badge idle">—</span>
          {/if}
        </button>
        {#if open[m.id]}
          <div class="body">
            {#if report?.error}<p class="error">{report.error}</p>{/if}
            {#if !report}
              <p class="muted">Not scanned yet.</p>
            {:else if !list.length}
              <p class="muted">Nothing found.</p>
            {:else}
              <div class="tools">
                <button class="btn small" onclick={() => allIn(m.id)}>Select all</button>
                <span class="muted">{picked.length} selected</span>
                {#each common(picked) as action (action)}
                  <button class="btn small" class:primary={action !== "open"} disabled={health.busy} onclick={() => health.apply(m.id, action, picked)}>
                    {ACTION_LABELS[action]}
                  </button>
                {/each}
              </div>
              <ul>
                {#each list as f (f.id)}
                  <li class:off={f.enabled === false}>
                    <input type="checkbox" disabled={!f.actions.length} checked={health.selected.has(f.id)} onchange={() => health.toggle(f.id)} aria-label={f.title} />
                    <div class="grow">
                      <div class="line">
                        <strong title={f.title}>{f.title}</strong>
                        {#each f.flags as flag (flag)}<span class="flag" class:warn={/unsigned|developer|side|not from|administrator|close/i.test(flag)}>{flag}</span>{/each}
                        {#if f.enabled !== null}<span class="flag">{f.enabled ? "On" : "Off"}</span>{/if}
                      </div>
                      <p title={f.path ?? ""}>{f.detail}{#if f.path && !f.detail.includes(f.path)} · <code>{f.path}</code>{/if}</p>
                      {#if health.failures[f.id]}<p class="error">{health.failures[f.id]}</p>{/if}
                    </div>
                    {#if f.confidence !== null}<span class="conf" class:low={f.confidence < 60} title="How sure the scan is">{f.confidence}%</span>{/if}
                    {#if f.size}<span class="size">{formatSize(f.size)}</span>{/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</section>

<style>
  .health { display: grid; gap: 10px; margin-top: 12px; padding: 14px 16px; }
  header { display: flex; align-items: flex-start; justify-content: space-between; flex-wrap: wrap; gap: 10px 16px; }
  .title { display: flex; gap: 10px; min-width: 0; flex: 1 1 360px; }
  .title :global(svg) { flex: none; margin-top: 2px; color: var(--accent); }
  h2 { font-size: 14px; font-weight: 600; }
  .title p { margin-top: 2px; color: var(--text-3); font-size: 11.5px; }
  .actions, .tools { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
  .progress { display: grid; gap: 5px; color: var(--text-2); font-size: 11.5px; }
  .modules { display: grid; gap: 4px; }
  .module { border: 1px solid rgb(255 255 255 / 0.06); border-radius: 8px; background: rgb(255 255 255 / 0.015); }
  .head { display: flex; align-items: center; gap: 8px; width: 100%; padding: 7px 10px; border: 0; background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
  .head:hover { background: rgb(255 255 255 / 0.03); }
  .head :global(.chev) { flex: none; color: var(--text-3); transition: transform 160ms var(--ease-out); }
  .module.open .head :global(.chev) { transform: rotate(90deg); }
  .name { flex: none; font-size: 12.5px; font-weight: 600; }
  .hint { overflow: hidden; flex: 1; color: var(--text-3); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; }
  .badge { flex: none; min-width: 26px; padding: 1px 7px; border-radius: 999px; background: rgb(var(--accent-soft-rgb) / 0.18); color: rgb(var(--accent-soft-rgb)); font-size: 10.5px; font-weight: 600; text-align: center; }
  .badge.clean { background: rgb(62 207 142 / 0.14); color: #98dfbd; }
  .badge.idle { background: rgb(255 255 255 / 0.05); color: var(--text-3); }
  .body { display: grid; gap: 6px; padding: 4px 10px 10px 32px; }
  ul { display: grid; gap: 2px; max-height: 340px; overflow: auto; list-style: none; }
  li { display: flex; align-items: center; gap: 9px; padding: 5px 6px; border-radius: 6px; }
  li:hover { background: rgb(255 255 255 / 0.03); }
  li.off { opacity: 0.65; }
  .grow { flex: 1; min-width: 0; }
  .line { display: flex; align-items: center; flex-wrap: wrap; gap: 5px; }
  .line strong { overflow: hidden; max-width: 100%; font-size: 12px; font-weight: 500; white-space: nowrap; text-overflow: ellipsis; }
  li p { overflow: hidden; color: var(--text-3); font-size: 10.75px; white-space: nowrap; text-overflow: ellipsis; }
  code { font-size: 10.5px; }
  .flag { padding: 0 6px; border-radius: 999px; background: rgb(255 255 255 / 0.06); color: var(--text-2); font-size: 9.75px; }
  .flag.warn { background: rgb(245 165 36 / 0.14); color: #f5c26b; }
  .conf { flex: none; color: #98dfbd; font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .conf.low { color: #f5c26b; }
  .size { flex: none; min-width: 58px; color: var(--text-2); font-size: 11px; text-align: right; font-variant-numeric: tabular-nums; }
  .muted { color: var(--text-3); font-size: 11px; }
  .error { color: #ffb4b0; font-size: 11px; white-space: normal; }
  .restore { display: grid; gap: 2px; max-height: 220px; overflow: auto; padding: 6px; border-radius: 8px; background: rgb(255 255 255 / 0.02); }
  .row { display: flex; align-items: center; gap: 8px; padding: 4px 6px; font-size: 11.5px; }
  .row em { color: var(--text-3); font-style: normal; font-size: 10.5px; }
  .btn.small { padding: 3px 9px; font-size: 11px; }
  @media (max-width: 760px) { .hint { display: none; } }
</style>
