<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import BrandIcon from "../app/pages/settings/BrandIcon.svelte";
  import { account } from "./account.svelte";

  const names = { discord: "Discord", google: "Google" } as const;
</script>

{#if account.signingIn}
  <div class="waiting" role="status">
    <LoaderCircle size={18} class="spin" />
    <span>
      <strong>Signing in with {names[account.signingIn]}…</strong>
      <small>Finish on {names[account.signingIn]}'s page; it closes and comes back here by itself.</small>
    </span>
    <button class="btn small" onclick={() => account.cancelSignIn()}>Cancel</button>
  </div>
{:else}
  <div class="providers">
    <button class="provider discord" onclick={() => account.signIn("discord")}>
      <BrandIcon brand="discord" size={19} /> Continue with Discord
    </button>
    <button class="provider google" onclick={() => account.signIn("google")}>
      <BrandIcon brand="google" size={17} /> Continue with Google
    </button>
  </div>
{/if}

<style>
  .providers {
    display: grid;
    gap: 10px;
  }

  .provider {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 48px;
    border-radius: 12px;
    font-size: 15px;
    font-weight: 600;
  }

  .provider:active {
    filter: brightness(0.92);
  }

  .discord {
    border: 1px solid rgb(255 255 255 / 0.14);
    background: linear-gradient(180deg, #6875f5, #5865f2);
    color: #fff;
  }

  .google {
    border: 1px solid rgb(0 0 0 / 0.08);
    background: linear-gradient(180deg, #ffffff, #eef0f5);
    color: #1f2330;
  }

  .waiting {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid rgb(var(--accent-rgb) / 0.22);
    border-radius: 12px;
    background: rgb(var(--accent-rgb) / 0.08);
  }

  .waiting span {
    display: grid;
    flex: 1;
    gap: 2px;
  }

  .waiting small {
    color: var(--text-2);
    font-size: 12.5px;
  }
</style>
