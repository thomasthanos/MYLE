<script lang="ts">
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Bluetooth from "@lucide/svelte/icons/bluetooth";
  import Globe from "@lucide/svelte/icons/globe";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Network from "@lucide/svelte/icons/network";
  import Router from "@lucide/svelte/icons/router";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Stethoscope from "@lucide/svelte/icons/stethoscope";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import type { MaintenanceCard } from "./api";
  import Console from "./Console.svelte";
  import { maintenanceState as s, type View } from "./state.svelte";

  let { card, view, wide = false }: { card: MaintenanceCard; view: View; wide?: boolean } = $props();

  const icons: Record<string, typeof Globe> = {
    dns: Globe,
    ip: Router,
    bluetooth: Bluetooth,
    network: Network,
    repair: Stethoscope,
    disk: HardDrive,
    audio: Volume2,
    updates: ArrowUp,
  };
  const Icon = $derived(icons[card.icon] ?? Stethoscope);
  const status = $derived(s.statusOf(card));
  // One clear action gets the accent — except on a card that warns you first,
  // where the button should not be the most inviting thing on the page.
  const single = $derived(card.actions.length === 1 && card.caution === null);
</script>

<article
  class="card {view}"
  class:wide
  class:has-notice={card.caution !== null}
  class:working={card.actions.some((a) => s.isRunning(a.id))}
  data-section={card.section}
>
  <div class="identity">
    <span class="icon"><Icon size={21} strokeWidth={1.65} /></span>
    <div class="body">
      <div class="head">
        <h3>{card.title}</h3>
        {#if card.admin}
          <span class="badge" title="Windows will ask for administrator approval">
            <ShieldCheck size={11} />
            Admin
          </span>
        {/if}
      </div>
      <p class="desc">{card.description}</p>
    </div>
  </div>

  {#if card.caution}
    <div class="notice">
      <TriangleAlert size={13} />
      <span>{card.caution}.</span>
    </div>
  {/if}

  <div class="footer">
    <span class="status {status.state}" role="status" aria-live="polite">
      <span class="status-dot" aria-hidden="true"></span>
      {status.label}
    </span>

    <div class="actions">
      {#each card.actions as action (action.id)}
        {#if s.isRunning(action.id)}
          <!-- The console header says whether it is UAC or the tool we wait on. -->
          <button class="btn" disabled>
            <LoaderCircle size={14} class="spin" />
            Working…
          </button>
        {:else if s.needsAdmin.has(action.id)}
          <button class="btn" disabled={s.locked} onclick={() => s.run(card, action, true)}>
            <ShieldCheck size={14} />
            Retry as administrator
          </button>
        {:else}
          <button class="btn" class:primary={single} disabled={s.locked} onclick={() => s.run(card, action)}>
            {action.label}
          </button>
        {/if}
      {/each}
    </div>
  </div>

  {#if card.console}
    <div class="console-slot"><Console {card} /></div>
  {/if}
</article>

<style>
  .card { --tone: var(--accent); --tone-soft: rgb(var(--accent-rgb) / 0.12); position: relative; display: flex; flex-direction: column; gap: 12px; min-width: 0; padding: 16px; overflow: hidden; border: 1px solid rgb(255 255 255 / 0.075); border-radius: var(--radius-lg); background: linear-gradient(180deg, rgb(255 255 255 / 0.052), rgb(255 255 255 / 0.018)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.085), 0 10px 28px -22px rgb(0 0 0 / 0.8); transition: transform var(--dur-med) var(--ease-out), border-color var(--dur-fast), box-shadow var(--dur-med) var(--ease-out); }

  .card::before { content: ""; position: absolute; top: 0; left: 18px; right: 18px; height: 1px; background: var(--tone); opacity: 0.55; pointer-events: none; }

  .card[data-section="network"] { --tone: #63d7e9; --tone-soft: rgb(83 211 234 / 0.11); }

  .card[data-section="health"] { --tone: rgb(var(--accent-soft-rgb)); --tone-soft: rgb(var(--accent-rgb) / 0.12); }

  .card[data-section="software"] { --tone: #54d6a0; --tone-soft: rgb(73 215 155 / 0.11); }

  .card:hover { transform: translateY(-2px); border-color: rgb(255 255 255 / 0.13); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1), 0 15px 34px -24px rgb(0 0 0 / 0.9); }

  .card.working { transform: none; border-color: color-mix(in srgb, var(--tone) 42%, transparent); }

  .identity { display: flex; align-items: flex-start; gap: 12px; min-width: 0; }

  .icon { display: grid; place-items: center; width: 42px; height: 42px; flex: none; border: 1px solid rgb(255 255 255 / 0.1); border-radius: 12px; background: var(--tone-soft); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.12); color: var(--tone); }

  .body { min-width: 0; }

  .head { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; }

  h3 { font-size: 14.5px; }

  .badge { display: inline-flex; align-items: center; gap: 4px; padding: 1px 6px; border: 1px solid rgb(255 255 255 / 0.075); border-radius: 999px; background: rgb(255 255 255 / 0.04); color: var(--text-3); font-size: 10.5px; font-weight: 500; }

  .desc { margin-top: 4px; color: var(--text-2); font-size: 12.5px; line-height: 1.5; }

  .notice { display: flex; align-items: flex-start; gap: 7px; padding: 8px 10px; border: 1px solid rgb(255 180 84 / 0.18); border-radius: 9px; background: rgb(255 180 84 / 0.055); color: #eec27d; font-size: 11.5px; line-height: 1.4; }

  .notice :global(svg) { flex: none; margin-top: 1px; }

  .footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-width: 0; margin-top: auto; padding-top: 11px; border-top: 1px solid rgb(255 255 255 / 0.06); }

  .status { display: inline-flex; align-items: center; gap: 7px; min-width: 0; color: var(--text-3); font-size: 11.5px; white-space: nowrap; }

  .status-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--idle); }

  .status.running { color: var(--tone); }

  .status.running .status-dot { background: var(--tone); animation: status-pulse 1.1s var(--ease-in-out) infinite alternate; }

  .status.completed { color: #6fe0ac; }

  .status.completed .status-dot { background: #49d79b; }

  .status.restartRequired,
  .status.needsAdmin { color: #eec27d; }

  .status.restartRequired .status-dot,
  .status.needsAdmin .status-dot { background: #e9ad55; }

  .status.error { color: #ffaaa6; }

  .status.error .status-dot { background: #ef666b; }

  .actions { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: 8px; }

  /* The console spans the whole card, under all columns. */
  .console-slot { grid-column: 1 / -1; }

  .card.list,
  .card.wide { grid-column: 1 / -1; display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; column-gap: 18px; }

  .card.list .footer,
  .card.wide .footer { grid-column: 2; grid-row: 1 / span 2; align-self: stretch; justify-content: flex-end; gap: 16px; margin-top: 0; padding-top: 0; padding-left: 18px; border-top: 0; border-left: 1px solid rgb(255 255 255 / 0.06); }

  .card.list .notice,
  .card.wide .notice { grid-column: 1; }

  .card.list .console-slot,
  .card.wide .console-slot { grid-row: 3; }

  .card.list:not(.has-notice) .footer,
  .card.wide:not(.has-notice) .footer { grid-row: 1; }

  .card.list:not(.has-notice) .console-slot,
  .card.wide:not(.has-notice) .console-slot { grid-row: 2; }

  @keyframes status-pulse {
    from { opacity: 0.38; }
    to { opacity: 1; }
  }

  @container maintenance (max-width: 650px) {
    .card.list,
    .card.wide {
      display: flex;
      flex-direction: column;
      align-items: stretch;
    }

    .card.list .footer,
    .card.wide .footer {
      justify-content: space-between;
      margin-top: auto;
      padding-top: 11px;
      padding-left: 0;
      border-top: 1px solid rgb(255 255 255 / 0.06);
      border-left: 0;
    }
  }

  :global(:root.solid) .card,
  :global(:root.solid) .card:hover { transform: none; box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.07); }

  :global(:root.solid) .status.running .status-dot { animation: none; }

  @media (prefers-reduced-motion: reduce) {
    .card,
    .card:hover {
      transform: none;
    }
  }
</style>
