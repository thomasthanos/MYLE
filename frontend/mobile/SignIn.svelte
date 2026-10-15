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
    <button class="provider discord" disabled={account.signingOut} onclick={() => account.signIn("discord")}>
      <span class="provider-icon"><BrandIcon brand="discord" size={22} /></span><span>Continue with Discord</span>
    </button>
    <button class="provider google" disabled={account.signingOut} onclick={() => account.signIn("google")}>
      <span class="provider-icon"><BrandIcon brand="google" size={21} /></span><span>Continue with Google</span>
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
    justify-content: flex-start;
    gap: 14px;
    min-width: 0;
    min-height: 52px;
    padding: 9px 14px;
    border-radius: 16px;
    font-family: var(--font-sans);
    font-size: 14px;
    font-weight: 600;
    text-align: left;
    overflow-wrap: anywhere;
  }

  .provider:active:not(:disabled) {
    filter: brightness(0.92);
  }

  .discord {
    border: 1px solid rgb(255 255 255 / 0.14);
    background: linear-gradient(145deg, #626fef, #4b58d4);
    color: #fff;
    box-shadow: inset 0 1px 0 rgb(255 255 255 / .16), 0 6px 16px -10px #5865f2;
  }

  .google {
    border: 1px solid rgb(255 255 255 / .13);
    background: linear-gradient(145deg, rgb(255 255 255 / .06), rgb(255 255 255 / .025));
    color: var(--text-1);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / .05);
  }

  .provider-icon { display: grid; place-items: center; flex: none; width: 34px; height: 34px; border-radius: 10px; background: rgb(255 255 255 / .12); }
  .google .provider-icon { background: #fff; }
  .provider:hover:not(:disabled) { filter: brightness(1.08); }
  .provider:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  .provider:disabled { opacity: .5; }

  .waiting {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid rgb(var(--accent-rgb) / 0.22);
    border-radius: 12px;
    background: rgb(var(--accent-rgb) / 0.08);
  }

  .waiting span {
    display: grid;
    min-width: 0;
    flex: 1;
    gap: 2px;
  }

  .waiting small {
    color: var(--text-2);
    font-size: 12.5px;
  }
  .waiting .btn { grid-column: 2; justify-self: start; min-height: 36px; }
</style>
