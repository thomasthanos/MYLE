<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Music2 from "@lucide/svelte/icons/music-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Store from "@lucide/svelte/icons/store";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Waves from "@lucide/svelte/icons/waves";
  import HubCard from "./HubCard.svelte";
  import PurgeDialog from "./PurgeDialog.svelte";
  import { spotifyHubState as hub } from "./state.svelte";

  onMount(() => {
    void hub.load();
  });

  const state = $derived(hub.snapshot);
  const chips = $derived([
    {
      label: "Spotify Desktop",
      value: state?.desktop.installed ? state.desktop.version ?? "Installed" : "Not installed",
      tone: state?.desktop.installed ? "ok" : "idle",
      icon: ShieldCheck,
    },
    {
      label: "Spotify Store",
      value: state?.store.installed ? state.store.version ?? "Installed" : "Not installed",
      tone: state?.store.installed ? "warn" : "idle",
      icon: Store,
    },
    {
      label: "Spicetify",
      value: state?.spicetify.installed ? state.spicetify.version ?? "Installed" : "Not installed",
      tone: state?.spicetify.installed ? (state.spicetify.healthy ? "ok" : "warn") : "idle",
      icon: Waves,
    },
    {
      label: "Marketplace",
      value: state?.marketplace.installed ? state.marketplace.version ?? "Installed" : "Not installed",
      tone: state?.marketplace.installed ? "ok" : "idle",
      icon: Store,
    },
  ]);
</script>

<div class="page">
  <div class="top">
    <header>
      <div class="title"><span class="title-icon"><Music2 size={18} /></span><h1>Spotify &amp; Spicetify Hub</h1></div>
      <p>Customize Spotify safely, return to stock, or remove every detected local component.</p>
    </header>
    <span class="mutex"><ShieldCheck size={13} /> One action at a time</span>
  </div>

  <div class="chips" aria-label="Spotify status">
    {#each chips as chip (chip.label)}
      <span class="status-chip {chip.tone}">
        <chip.icon size={13} />
        <span>{chip.label}</span>
        <strong>{hub.loading && !state ? "Checking…" : chip.value}</strong>
      </span>
    {/each}
  </div>

  {#if hub.error}
    <div class="banner surface" role="alert"><CircleAlert size={17} /><span>{hub.error}</span></div>
  {/if}

  {#if hub.updateError}
    <div class="notice surface warn" role="status"><CircleAlert size={15} /><span>{hub.updateError} Use Check for updates to retry.</span></div>
  {:else if hub.releases && state?.marketplace.installed && !state.marketplace.version}
    <div class="notice surface" role="status"><CircleAlert size={15} /><span>Marketplace is installed, but its version could not be verified. Repair records the verified release for future update checks.</span></div>
  {/if}

  {#if state?.prerequisites.message}
    <div class="notice surface" class:warn={!state.prerequisites.supported}>
      <CircleAlert size={15} />
      <span>{state.prerequisites.message}</span>
    </div>
  {/if}

  {#if hub.installAppsBusy && !hub.ownBusy}
    <div class="notice surface"><LoaderCircle size={14} class="spin" /><span>Another app task is running. Hub actions are temporarily locked.</span></div>
  {/if}

  <div class="cards">
    <HubCard
      action="installSpicetify"
      title={state?.spicetify.installed ? "Spicetify & Marketplace" : "Install Spicetify"}
      description="Checks installed versions against the latest verified stable CLI and Marketplace releases."
      detail="Existing extensions and themes are preserved, and an active custom theme is never replaced."
      accent="green"
      icon={Download}
    />
    <HubCard
      action="restoreSpotify"
      title="Restore stock Spotify"
      description="Run the official restore first, then remove Spicetify, Marketplace and this app's PATH entry."
      detail="Recovery files stay untouched if the vanilla restore does not complete successfully."
      accent="amber"
      icon={RotateCcw}
    />
    <HubCard
      action="purgeAll"
      title="Full uninstall Spotify"
      description="Remove detected Desktop and Store installations plus allowlisted local data and shortcuts."
      detail="A preview and the typed phrase REMOVE SPOTIFY are required before any deletion begins."
      accent="red"
      icon={Trash2}
    />
  </div>
</div>

{#if hub.purgePreview}<PurgeDialog />{/if}

<style>
  .page { min-width: 0; }
  .top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  header { min-width: 0; }
  .title { display: flex; align-items: center; gap: 9px; }
  .title-icon { display: grid; place-items: center; width: 28px; height: 28px; border: 1px solid rgb(62 207 142 / 0.18); border-radius: 9px; background: rgb(62 207 142 / 0.065); color: rgb(92 218 166 / 0.85); }
  h1 { font-size: 26px; line-height: 1.2; }
  header p { margin-top: 4px; color: var(--text-2); }
  .mutex { display: inline-flex; align-items: center; gap: 6px; flex: none; margin-top: 4px; padding: 5px 10px; border: 1px solid rgb(62 207 142 / 0.22); border-radius: 999px; background: rgb(62 207 142 / 0.07); color: rgb(102 222 171 / 0.86); font-size: 11px; }

  .chips { display: flex; flex-wrap: wrap; gap: 7px; margin: 17px 0 20px; }
  .status-chip { display: inline-flex; align-items: center; gap: 6px; min-width: 0; height: 27px; padding: 0 9px; border: 1px solid rgb(255 255 255 / 0.065); border-radius: 999px; background: rgb(255 255 255 / 0.025); color: var(--text-3); font-size: 10.5px; }
  .status-chip strong { color: var(--text-2); font-weight: 600; font-variant-numeric: tabular-nums; }
  .status-chip.ok :global(svg) { color: var(--ok); }
  .status-chip.warn :global(svg) { color: #f5b454; }

  .banner,
  .notice { display: flex; align-items: center; gap: 9px; margin-bottom: 12px; padding: 10px 12px; color: var(--text-2); font-size: 11.5px; }
  .banner { border-color: rgb(229 72 77 / 0.35); color: #ffb4b0; }
  .notice.warn { border-color: rgb(245 180 84 / 0.24); color: rgb(255 205 126 / 0.85); }
  .notice :global(svg), .banner :global(svg) { flex: none; }

  .cards { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); align-items: stretch; gap: 12px; }

  .cards:has(:global(.collapse.open)) { align-items: start; }

  @media (max-width: 980px) {
    .cards {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 760px) {
    .top {
      flex-direction: column;
      gap: 8px;
    }

    .mutex {
      margin-top: 0;
    }
  }
</style>
