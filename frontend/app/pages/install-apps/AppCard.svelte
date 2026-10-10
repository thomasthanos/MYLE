<script lang="ts">
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import X from "@lucide/svelte/icons/x";
  import AppIcon from "./AppIcon.svelte";
  import JobProgress from "./JobProgress.svelte";
  import { appsState, type AppEntry } from "./state.svelte";

  let { app, view }: { app: AppEntry; view: "grid" | "list" } = $props();

  const uid = $props.id();
  const status = $derived(appsState.statusOf(app));
  const info = $derived(appsState.statusInfo(app));
  const job = $derived(appsState.jobs[app.id.toLowerCase()]);
  const selected = $derived(appsState.isSelected(app));
  const showActivate = $derived(!!app.activateLabel && status !== "missing" && status !== "unknown" && !info?.activated && !job);
  const statusText = $derived.by(() => {
    switch (status) {
      case "update": return `Update ${info?.version ?? ""} → ${info?.available ?? ""}`.replace("  ", " ");
      case "installed": return info?.version ? `Installed · ${info.version}` : "Installed";
      case "missing": return app.version ? `Not installed · ${app.version}` : "Not installed";
      default: return "Checking…";
    }
  });
</script>

<div class="card surface {view}" class:selected class:working={!!job}>
  <input id="{uid}-check" type="checkbox" class="check" checked={selected} disabled={appsState.externallyLocked} onchange={() => appsState.toggle(app)} />
  <label class="main" for="{uid}-check">
    <AppIcon {app} {status} size={view === "grid" ? 32 : 24} />
    <span class="names">
      <span class="name">{app.name}</span>
      <span class="id">{app.id}</span>
    </span>
  </label>
  <div class="foot">
    {#if job}<JobProgress {job} />{:else}<span class="status {status}">{statusText}</span>{/if}
  </div>
  <div class="actions">
    {#if showActivate}<button class="btn small" disabled={appsState.externallyLocked} onclick={() => appsState.activate(app)}>{app.activateLabel}</button>{/if}
    {#if job}
      <button class="icon-btn" title="Cancel" aria-label="Cancel {app.name}" onclick={() => appsState.cancel(app)}><X size={16} /></button>
    {:else}
      <button class="icon-btn" title="Open website" aria-label="Open the {app.name} website" onclick={() => appsState.openSite(app)}><ArrowUpRight size={16} /></button>
    {/if}
  </div>
</div>

<style>
  /* Off-screen cards skip layout and paint: the catalog is well over a thousand elements. */
  .card { position: relative; display: grid; min-width: 0; content-visibility: auto; contain-intrinsic-size: auto 118px; transition: border-color var(--dur-fast), background var(--dur-fast); }
  .card:hover { border-color: rgb(255 255 255 / 0.12); }
  .card.selected { border-color: rgb(var(--accent-rgb) / 0.45); background: var(--selected); }
  /* Grid: checkbox top-right, action bottom-right; list: one compact row. */
  .card.grid { grid-template-columns: minmax(0, 1fr) auto; grid-template-areas: "main check" "foot actions"; gap: 12px 8px; padding: 13px 13px 11px; }
  .card.list { contain-intrinsic-size: auto 52px; grid-template-columns: auto minmax(0, 1.4fr) minmax(0, 1fr) auto; grid-template-areas: "check main foot actions"; align-items: center; gap: 12px; padding: 7px 10px 7px 12px; }
  .check { grid-area: check; }
  .grid .check { justify-self: end; }
  .main { grid-area: main; display: flex; align-items: center; gap: 11px; min-width: 0; }
  .names { display: grid; min-width: 0; }
  .name, .id { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .name { font-weight: 600; }
  .id { color: var(--text-3); font-family: var(--font-mono); font-size: 11px; }
  .foot { grid-area: foot; align-self: center; min-width: 0; }
  .status { display: inline-block; max-width: 100%; height: 20px; padding: 0 7px; overflow: hidden; border: 1px solid rgb(255 255 255 / 0.035); border-radius: 999px; background: rgb(255 255 255 / 0.015); color: var(--text-3); font-size: 11.25px; font-weight: 500; line-height: 18px; white-space: nowrap; text-overflow: ellipsis; font-variant-numeric: tabular-nums; }
  .status.installed { border-color: rgb(62 207 142 / 0.1); background: rgb(62 207 142 / 0.035); color: rgb(80 220 158 / 0.78); }
  .status.update { border-color: rgb(77 163 255 / 0.12); background: linear-gradient(180deg, rgb(77 163 255 / 0.07), rgb(77 163 255 / 0.03)); color: rgb(103 180 255 / 0.82); }
  .status.missing, .status.unknown { color: rgb(205 214 240 / 0.48); }
  .actions { grid-area: actions; display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
</style>
