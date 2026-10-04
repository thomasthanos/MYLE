<script lang="ts">
  import Cloud from "@lucide/svelte/icons/cloud";
  import CloudAlert from "@lucide/svelte/icons/cloud-alert";
  import CloudOff from "@lucide/svelte/icons/cloud-off";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { nav } from "../../../lib/nav.svelte";
  import { DEVICE, MOBILE } from "../../../lib/platform";
  import { shell } from "../../../mobile/shell.svelte";
  import { passwords as p } from "./state.svelte";

  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  function ago(at: number) {
    const minutes = Math.floor((now - at) / 60_000);
    return minutes < 1 ? "just now" : minutes < 60 ? `${minutes} min ago` : "a while ago";
  }
</script>

{#if p.sync.kind === "signedOut"}
  <button
    class="sync off"
    title={MOBILE ? "Sign in to keep your passwords the same here and on your PC." : "Sign in on the Settings page to keep your passwords on all your PCs."}
    onclick={() => (MOBILE ? (shell.accountOpen = true) : nav.go("settings"))}
  >
    <CloudOff size={14} /> Only on this {DEVICE}
  </button>
{:else if p.sync.kind === "syncing"}
  <span class="sync"><LoaderCircle size={14} class="spin" /> Syncing…</span>
{:else if p.sync.kind === "error"}
  <button class="sync bad" title={p.sync.message} onclick={() => p.syncNow()}><CloudAlert size={14} /> Sync failed · retry</button>
{:else if p.sync.kind === "synced"}
  <button class="sync ok" title="Synced with your account. Click to sync now." onclick={() => p.syncNow()}>
    <Cloud size={14} /> Synced {ago(p.sync.at)}{p.sync.pending ? ` · ${p.sync.pending} waiting` : ""}
  </button>
{/if}

<style>
  .sync {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border-radius: 999px;
    color: var(--text-3);
    font-size: 11.5px;
    white-space: nowrap;
  }

  button.sync:hover {
    background: var(--hover);
    color: var(--text-1);
  }

  .ok {
    color: #8fe3b6;
  }

  .bad {
    color: #ff9d9d;
  }
</style>
