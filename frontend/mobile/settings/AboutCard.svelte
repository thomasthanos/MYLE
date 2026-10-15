<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import Info from "@lucide/svelte/icons/info";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { IOS } from "../../lib/platform";
  import { shell } from "../shell.svelte";
  import { updater } from "../updater.svelte";
  import SettingsCard from "./SettingsCard.svelte";

  let version = $state<string | null>(null);
  let checked = $state<"idle" | "checking" | "latest" | "offline">("idle");

  onMount(() => {
    getVersion().then((found) => (version = found)).catch(() => {});
  });

  async function check() {
    checked = "checking";
    const answered = await updater.check();
    checked = !answered ? "offline" : updater.update ? "idle" : "latest";
  }

  /** The update's own banner shows the download and Android's installer. */
  function update() {
    updater.dismissed = false;
    shell.settingsOpen = false;
    void updater.start();
  }
</script>

<SettingsCard icon={Info} title="About">
  <div class="app">
    <b>MYLE Passwords{version ? ` ${version}` : ""}</b>
    <small>Your master password never leaves this phone.</small>
  </div>
  {#if IOS}
    <small>Updates come through SideStore.</small>
  {:else if updater.update}
    <div class="available">
      <span>Version {updater.update.version} is ready to download.</span>
      <button class="btn primary" onclick={update}>Update</button>
    </div>
  {:else}
    <button class="btn" disabled={checked === "checking"} onclick={check}>
      {#if checked === "checking"}<LoaderCircle size={15} class="spin" /> Checking…{:else}Check for updates{/if}
    </button>
    {#if checked === "latest"}
      <small role="status">You have the latest version.</small>
    {:else if checked === "offline"}
      <small role="status">Could not check. Connect to the internet and try again.</small>
    {/if}
  {/if}
</SettingsCard>

<style>
  .app { display: grid; gap: 3px; }

  b { font-size: 14px; font-weight: 600; }

  small { color: var(--text-2); font-size: 12.5px; line-height: 1.4; }

  .available { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 12px; font-size: 13.5px; }

  .btn { min-height: 48px; }
</style>
