<script lang="ts">
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import { DEVICE, MOBILE } from "../../../lib/platform";
  import SetupVault from "./SetupVault.svelte";
  import { passwords as p } from "./state.svelte";
  import UnlockVault from "./UnlockVault.svelte";
  import VaultView from "./VaultView.svelte";
  import VerifyPasskey from "./VerifyPasskey.svelte";

  onMount(() => {
    p.error = null;
    p.pageOpened();
    void p.load();
  });
</script>

{#if !MOBILE}
  <PageHeader title="Password Manager" subtitle="Your logins, encrypted on this {DEVICE} before they are saved or synced." />
{/if}

{#if p.status === null || (p.status === "new" && !p.checkedAccount)}
  <div class="loading">
    <LoaderCircle size={20} class="spin" />
    {#if p.status === "new"}<span>Checking your account for a vault…</span>{/if}
  </div>
{:else if p.status === "new"}
  <SetupVault />
{:else if p.status === "locked"}
  <UnlockVault />
{:else}
  <VaultView />
{/if}

<!-- Passkeys sign in through the browser extension, on Windows only. -->
{#if !MOBILE}<VerifyPasskey />{/if}

<style>
  .loading {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 10px;
    height: 200px;
    color: var(--text-3);
    font-size: 12.5px;
  }
</style>
