<script lang="ts">
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import type { Component } from "svelte";
  import type { SpotifyHubAction } from "./api";
  import LiveConsole from "./LiveConsole.svelte";
  import { spotifyHubState as hub } from "./state.svelte";

  let {
    action,
    title,
    description,
    detail,
    accent,
    icon: Icon,
  }: {
    action: SpotifyHubAction;
    title: string;
    description: string;
    detail: string;
    accent: "green" | "amber" | "red";
    icon: Component;
  } = $props();

  const status = $derived(hub.statusOf(action));
  const active = $derived(hub.activeAction === action);
  const progress = $derived(hub.progressOf(action));
  const state = $derived(hub.snapshot);
  const hasConsole = $derived(hub.consoles[action].lines.length > 0 || active);
  const unavailable = $derived(
    !state || (action === "restoreSpotify" && !state.spicetify.installed),
  );

  const label = $derived(
    action === "installSpicetify"
      ? hub.installLabel()
      : action === "restoreSpotify"
        ? "Restore Spotify"
        : hub.previewingPurge
          ? "Preparing preview…"
          : "Review & uninstall",
  );
  const current = $derived(action === "installSpicetify" && (label === "Up to date" || label === "Installed"));

  function run() {
    if (action === "installSpicetify") void hub.install();
    else if (action === "restoreSpotify") void hub.restore();
    else void hub.previewPurge();
  }
</script>

<article class="card {accent}" class:active class:error={status === "Error"}>
  <div class="rim" aria-hidden="true"></div>
  <!-- The card's height is reserved here, above the console: opening the
       console grows the card downwards instead of pulling the button up. -->
  <div class="main">
    <span class="icon"><Icon size={22} strokeWidth={1.65} /></span>
    <span class="status {status.toLowerCase()}" aria-live="polite"><span class="status-dot"></span>{status}</span>

    <div class="copy">
      <h2>{title}</h2>
      <p>{description}</p>
      <p class="detail">{detail}</p>
    </div>

    {#if active}
      <div class="progress">
        <div class="progress-meta">
          <span><LoaderCircle size={13} class="spin" /> {hub.stageLabel(action) ?? "Preparing"}</span>
          {#if progress !== null}<strong>{Math.round(progress * 100)}%</strong>{/if}
        </div>
        <div
          class="track"
          class:indeterminate={progress === null}
          role="progressbar"
          aria-label={hub.stageLabel(action) ?? "Preparing"}
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={progress === null ? undefined : Math.round(progress * 100)}
          aria-valuetext={progress === null
            ? `${hub.stageLabel(action) ?? "Preparing"} in progress`
            : `${Math.round(progress * 100)} percent`}
        >
          <span style:transform={progress === null ? undefined : `scaleX(${progress})`}></span>
        </div>
      </div>
    {:else}
      <div class="actions">
        <button
        class="btn action"
        class:danger={action === "purgeAll"}
        disabled={hub.loading || hub.locked || unavailable || hub.previewingPurge || current || (action === "installSpicetify" && hub.checkingUpdates)}
        onclick={run}
      >
        {label}
        {#if !current}<ArrowRight size={14} />{/if}
        </button>
        {#if action === "installSpicetify" && state?.spicetify.installed && state.desktop.installed}
          {#if state.spicetify.healthy}
            <button class="btn quiet" disabled={hub.loading || hub.locked || hub.checkingUpdates} onclick={() => hub.checkUpdates()}>Check again</button>
          {/if}
          <button class="btn quiet" disabled={hub.loading || hub.locked} onclick={() => hub.repair()}>Repair / reinstall</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if hasConsole}
    <LiveConsole {action} />
  {/if}
</article>

<style>
  .card {
    --tone: 62 207 142;
    --tone-soft: rgb(var(--tone) / 0.1);
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 17px;
    overflow: hidden;
    border: 1px solid rgb(var(--tone) / 0.18);
    border-radius: var(--radius-lg);
    background:
      var(--grain),
      radial-gradient(circle at 12% 0%, rgb(var(--tone) / 0.1), transparent 42%),
      linear-gradient(180deg, rgb(200 210 255 / 0.08), rgb(200 210 255 / 0.022) 62%, rgb(0 0 0 / 0.08));
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.11),
      inset 0 -1px 0 rgb(0 0 0 / 0.28),
      var(--elev-2);
    transition:
      transform var(--dur-med) var(--ease-out),
      border-color var(--dur-fast),
      box-shadow var(--dur-med) var(--ease-out);
  }

  .card.amber { --tone: 245 180 84; }
  .card.red { --tone: 229 72 77; }

  .card:hover:not(.active) {
    transform: translateY(-3px);
    border-color: rgb(var(--tone) / 0.32);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.17),
      inset 0 -1px 0 rgb(0 0 0 / 0.28),
      0 18px 48px -24px rgb(var(--tone) / 0.35),
      var(--elev-3);
  }

  .card.active { border-color: rgb(var(--tone) / 0.48); }
  .card.error { border-color: rgb(229 72 77 / 0.48); }

  .rim {
    position: absolute;
    top: 0;
    left: 12%;
    width: 76%;
    height: 1px;
    background: linear-gradient(90deg, transparent, rgb(var(--tone) / 0.72), transparent);
    box-shadow: 0 0 15px rgb(var(--tone) / 0.42);
  }

  .progress-meta,
  .progress-meta span {
    display: flex;
    align-items: center;
  }

  .main {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto 1fr auto;
    gap: 14px 10px;
    flex: 1;
  }

  .icon {
    grid-column: 1;
    grid-row: 1;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border: 1px solid rgb(var(--tone) / 0.24);
    border-radius: 13px;
    background: linear-gradient(160deg, rgb(var(--tone) / 0.16), rgb(var(--tone) / 0.035));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.16), 0 8px 20px -13px rgb(var(--tone) / 0.7);
    color: rgb(var(--tone));
  }

  .status {
    grid-column: 2;
    grid-row: 1;
    justify-self: end;
    align-self: center;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 999px;
    background: rgb(255 255 255 / 0.035);
    color: var(--text-3);
    font-size: 10.5px;
    font-weight: 600;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--idle);
  }

  .status.running .status-dot { background: rgb(var(--tone)); box-shadow: 0 0 8px rgb(var(--tone) / 0.6); animation: pulse 1s ease-in-out infinite alternate; }
  .status.completed .status-dot { background: var(--ok); box-shadow: 0 0 8px var(--ok-glow); }
  .status.partial .status-dot { background: #f5b454; box-shadow: 0 0 8px rgb(245 180 84 / 0.45); }
  .status.error .status-dot { background: var(--danger); box-shadow: 0 0 8px rgb(229 72 77 / 0.5); }

  .copy {
    grid-column: 1 / -1;
    grid-row: 2;
    min-width: 0;
  }

  h2 { font-size: 16px; }
  .copy p { margin-top: 6px; color: var(--text-2); font-size: 12px; line-height: 1.48; }
  .copy .detail { color: var(--text-3); font-size: 10.75px; }

  .actions {
    grid-column: 1 / -1;
    grid-row: 3;
    justify-self: start;
    align-self: end;
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
  }
  .action {
    border-color: rgb(var(--tone) / 0.22);
    background: var(--tone-soft);
  }
  .action:hover { border-color: rgb(var(--tone) / 0.4); background: rgb(var(--tone) / 0.16); }
  .quiet { font-size: 11px; }

  .progress {
    grid-column: 1 / -1;
    grid-row: 3;
    align-self: end;
    display: grid;
    gap: 7px;
  }
  .progress-meta { justify-content: space-between; gap: 8px; color: var(--text-2); font-size: 11px; }
  .progress-meta span { gap: 6px; }
  .progress-meta strong { color: var(--text-3); font-size: 10px; font-weight: 500; font-variant-numeric: tabular-nums; }
  .track { position: relative; height: 4px; overflow: hidden; border-radius: 999px; background: rgb(0 0 0 / 0.28); }
  .track span { position: absolute; inset: 0; border-radius: inherit; background: rgb(var(--tone)); box-shadow: 0 0 10px rgb(var(--tone) / 0.5); transform-origin: left; transition: transform 160ms linear; }
  .track.indeterminate span { width: 34%; animation: sweep 1.2s var(--ease-in-out) infinite; }

  @media (max-width: 980px) {
    .main {
      grid-template-columns: auto minmax(0, 1fr) auto;
      grid-template-rows: auto auto;
      align-items: center;
      gap: 10px 14px;
    }

    .icon {
      grid-column: 1;
      grid-row: 1 / span 2;
      align-self: start;
    }

    .copy {
      grid-column: 2;
      grid-row: 1 / span 2;
    }

    .status {
      grid-column: 3;
      grid-row: 1;
      align-self: start;
    }

    .actions {
      grid-column: 3;
      grid-row: 2;
      justify-self: end;
      align-self: end;
    }

    .progress {
      grid-column: 2 / -1;
      grid-row: 2;
    }
  }

  @keyframes sweep { from { transform: translateX(-100%); } to { transform: translateX(295%); } }
  @keyframes pulse { to { opacity: 0.4; } }

  :global(:root.solid) .card,
  :global(:root.solid) .card:hover:not(.active) {
    transform: none;
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1), inset 0 -1px 0 rgb(0 0 0 / 0.28), var(--elev-1);
  }

  :global(:root.solid) .rim { box-shadow: none; }

  @media (prefers-reduced-motion: reduce) {
    .card:hover:not(.active) { transform: none; }
    .rim { box-shadow: none; }
  }
</style>
