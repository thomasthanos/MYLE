<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { onBackButtonPress } from "@tauri-apps/api/app";
  import Lock from "@lucide/svelte/icons/lock";
  import UserRound from "@lucide/svelte/icons/user-round";
  import ConfirmHost from "../lib/components/ConfirmHost.svelte";
  import Logo from "../lib/components/Logo.svelte";
  import Toaster from "../lib/components/Toaster.svelte";
  import { confirmState } from "../lib/confirm.svelte";
  import { passwordsApi as api } from "../app/pages/password-manager/api";
  import PasswordManager from "../app/pages/password-manager/PasswordManager.svelte";
  import { passwords as p } from "../app/pages/password-manager/state.svelte";
  import AccountSheet from "./AccountSheet.svelte";
  import { account } from "./account.svelte";
  import Scanner from "./Scanner.svelte";
  import { scanner } from "./camera.svelte";
  import { shell } from "./shell.svelte";
  import UpdateBanner from "./UpdateBanner.svelte";
  import { updater } from "./updater.svelte";
  import Welcome from "./Welcome.svelte";

  /** This phone has a vault: null until known. */
  let hasVault = $state<boolean | null>(null);
  /** The user chose a vault on this phone only, without an account. */
  let local = $state(false);
  /** Out of sight: the page is covered, so the app switcher's picture of
   *  it shows no passwords. */
  let away = $state(false);

  const welcome = $derived(account.loaded && !account.profile && hasVault === false && !local);

  onMount(() => {
    void account.init();
    void api
      .status()
      .then((info) => {
        hasVault = info.status !== "new";
        p.pageOpened();
      })
      .catch(() => (hasVault = true));
    void updater.check();

    const onVisibility = () => void visibilityChanged(document.visibilityState === "hidden");
    document.addEventListener("visibilitychange", onVisibility);
    const back = onBackButtonPress(goBack).catch(() => null);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      void back.then((listener) => listener?.unregister());
    };
  });

  async function visibilityChanged(hidden: boolean) {
    if (hidden) {
      away = true;
      void api.appHidden(true).catch(() => {});
      return;
    }
    // Back from Android's sign-in tab without signing in (closed).
    account.resumed();
    const locked = await api.appHidden(false).catch(() => false);
    if (locked) {
      // Face ID or the fingerprint asks at once, as on opening the app.
      p.pageOpened();
      await p.lock();
    }
    away = false;
    if (p.status === "unlocked" || locked) void p.syncNow();
    updater.resumed();
  }

  /** Android's back button: closes what is open, then goes back a step;
   *  on the list, it leaves the app (the vault locks with it). */
  function goBack() {
    if (scanner.active) {
      scanner.stop();
    } else if (confirmState.current) {
      confirmState.answer(false);
    } else if (shell.accountOpen) {
      shell.accountOpen = false;
    } else if (document.querySelector('[role="dialog"], .popover > .panel')) {
      // Dialogs and menus close on Escape.
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    } else if (p.panel.kind === "edit" && p.panel.id) {
      p.panel = { kind: "view", id: p.panel.id };
    } else if (p.panel.kind !== "none") {
      p.panel = { kind: "none" };
    } else if (isTauri()) {
      void invoke("mobile_leave");
    }
  }
</script>

<div class="shell" class:away>
  <header class="bar">
    <span class="brand"><Logo size={26} /> MYLE Passwords</span>
    {#if p.status === "unlocked"}
      <button class="icon-btn" title="Lock now" aria-label="Lock the vault" onclick={() => p.lock()}><Lock size={18} /></button>
    {/if}
    {#if !welcome}
      <button class="icon-btn account" class:signed-in={!!account.profile} aria-label="Account" onclick={() => (shell.accountOpen = true)}>
        <UserRound size={18} />
      </button>
    {/if}
  </header>

  <UpdateBanner />

  <main>
    {#if !account.loaded || hasVault === null}
      <!-- A moment: whether this phone has a vault and an account. -->
    {:else if welcome}
      <Welcome onlocal={() => (local = true)} />
    {:else}
      <PasswordManager />
    {/if}
  </main>
</div>

<AccountSheet />
<Toaster />
<ConfirmHost />
<Scanner />

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding-right: env(safe-area-inset-right);
    padding-left: env(safe-area-inset-left);
  }

  .bar {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    padding: calc(env(safe-area-inset-top) + 8px) 12px 8px 16px;
    border-bottom: 1px solid rgb(255 255 255 / 0.06);
    background: var(--bg-1);
  }

  .brand {
    display: inline-flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    font-family: var(--font-display);
    font-size: 17px;
    font-weight: 650;
  }

  .account.signed-in {
    color: #8fe3b6;
  }

  main {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 14px 14px calc(env(safe-area-inset-bottom) + 12px);
    overflow: auto;
  }

  /* Out of sight: nothing readable in the app switcher. */
  .away main {
    filter: blur(18px);
  }
</style>
