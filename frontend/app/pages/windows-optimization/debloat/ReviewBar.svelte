<script lang="ts">
  // Under every tab that records choices: how many there are, and the way
  // to review and apply them.
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import { debloat } from "./state.svelte";

  const parts = $derived(
    [
      debloat.pending.on.length && `${debloat.pending.on.length} to turn on`,
      debloat.pending.off.length && `${debloat.pending.off.length} to turn off`,
      debloat.pendingApps.length && `${debloat.pendingApps.length} app${debloat.pendingApps.length === 1 ? "" : "s"} to remove`,
    ].filter(Boolean),
  );
</script>

{#if debloat.pendingCount && !debloat.busy}
  <div class="review-bar surface" role="region" aria-label="Changes ready">
    <span class="count">{debloat.pendingCount}</span>
    <div class="summary">
      <strong>{debloat.pendingCount === 1 ? "1 change ready" : `${debloat.pendingCount} changes ready`}</strong>
      <span>{parts.join(" · ")}. Nothing changes until you apply.</span>
    </div>
    <div class="actions">
      <button type="button" class="btn quiet" disabled={debloat.locked} onclick={() => debloat.clearChoices()}>Clear</button>
      <button type="button" class="btn primary" disabled={debloat.locked} onclick={() => (debloat.reviewing = true)}>
        <ListChecks size={15} /> Review &amp; apply
      </button>
    </div>
  </div>
{/if}

<style>
  .review-bar { position: sticky; bottom: 0; z-index: 3; display: flex; align-items: center; gap: 12px; margin-top: 12px; padding: 9px 10px 9px 12px; border-color: rgb(var(--accent-rgb) / 0.32); background: var(--float-bg); box-shadow: 0 -10px 24px -14px rgb(0 0 0 / 0.8); }
  .count { display: grid; place-items: center; flex: none; min-width: 28px; height: 28px; padding: 0 7px; border-radius: 9px; background: var(--accent-grad); color: #fff; font-size: 12.5px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .summary { display: grid; flex: 1; gap: 1px; min-width: 0; }
  strong { color: var(--text-1); font-size: 12.5px; font-weight: 600; }
  .summary span { overflow: hidden; color: var(--text-2); font-size: 11.5px; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; align-items: center; gap: 6px; flex: none; }
  .btn { min-height: 32px; height: auto; padding: 6px 12px; font-size: 12px; }
  .btn.primary { border-color: rgb(var(--accent-rgb) / 0.45); background: rgb(var(--accent-rgb) / 0.3); color: var(--text-1); }
  .btn.primary:hover:not(:disabled) { background: rgb(var(--accent-rgb) / 0.42); filter: none; }
  .btn.quiet { border-color: transparent; background: transparent; color: var(--text-2); }
  .btn.quiet:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  :global(:root.solid) .review-bar { box-shadow: none; }
</style>
