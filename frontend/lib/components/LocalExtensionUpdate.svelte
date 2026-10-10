<script lang="ts">
  // "Local extension update available": the side-loaded (unpacked) copy is
  // older than the extension this MYLE carries. One click copies the new
  // files into its folder; reloading the extension picks them up.
  import { onMount } from "svelte";
  import { isTauri } from "@tauri-apps/api/core";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { passwordsApi as api } from "../../app/pages/password-manager/api";
  import { toast } from "../toast.svelte";

  let { setup = null }: { setup?: import("../../app/pages/password-manager/api").BrowserSetup | null } = $props();

  let own = $state<import("../../app/pages/password-manager/api").BrowserSetup | null>(null);
  let busy = $state(false);
  let done = $state(false);
  const info = $derived(setup ?? own);

  onMount(() => {
    if (!setup && isTauri()) void api.browserGet().then((s) => (own = s), () => {});
  });

  async function update() {
    busy = true;
    try {
      const dir = await api.syncExtension();
      done = true;
      toast.success(`Extension ${info?.bundledVersion ?? ""} copied to ${dir}. Reload it in the browser's extensions page.`);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      busy = false;
    }
  }
</script>

{#if info?.localUpdate && !done}
  <div class="local-update" role="status">
    <span>
      <strong>Local extension update available</strong>
      <small>
        Your unpacked copy is {info.lastContact?.version || info.localVersion || "older"}; MYLE has {info.bundledVersion}.
      </small>
    </span>
    <button class="btn small primary" disabled={busy} onclick={update}>
      <RefreshCw size={13} class={busy ? "spin" : ""} /> Update Local Extension
    </button>
  </div>
{:else if done}
  <p class="local-done">Updated. Open the browser's extensions page and press Reload on MYLE Passwords.</p>
{/if}

<style>
  .local-update {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border: 1px solid rgb(var(--accent-rgb) / 0.35);
    border-radius: 10px;
    background: rgb(var(--accent-rgb) / 0.1);
  }

  .local-update span {
    display: grid;
    flex: 1;
    gap: 2px;
    font-size: 12.5px;
  }

  .local-update small,
  .local-done {
    color: var(--text-3);
    font-size: 11.5px;
  }
</style>
