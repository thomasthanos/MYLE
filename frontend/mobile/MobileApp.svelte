<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { onBackButtonPress } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Lock from "@lucide/svelte/icons/lock";
  import UserRound from "@lucide/svelte/icons/user-round";
  import X from "@lucide/svelte/icons/x";
  import ConfirmHost from "../lib/components/ConfirmHost.svelte";
  import Logo from "../lib/components/Logo.svelte";
  import Toaster from "../lib/components/Toaster.svelte";
  import { toast } from "../lib/toast.svelte";
  import { confirmState } from "../lib/confirm.svelte";
  import { passwordsApi as api, type MobileUpdate } from "../app/pages/password-manager/api";
  import PasswordManager from "../app/pages/password-manager/PasswordManager.svelte";
  import { passwords as p } from "../app/pages/password-manager/state.svelte";
  import AccountSheet from "./AccountSheet.svelte";
  import { account } from "./account.svelte";
  import Scanner from "./Scanner.svelte";
  import { scanner } from "./camera.svelte";
  import { shell } from "./shell.svelte";
  import Welcome from "./Welcome.svelte";

  /** This phone has a vault: null until known. */
  let hasVault = $state<boolean | null>(null);
  /** The user chose a vault on this phone only, without an account. */
  let local = $state(false);
  let update = $state<MobileUpdate | null>(null);
  let updateState = $state<"available" | "downloading" | "ready" | "error">("available");
  let downloadPercent = $state(0);
  let downloadedPath = $state<string | null>(null);
  let updateErrorMsg = $state<string | null>(null);
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
    void checkForUpdates();

    const onVisibility = () => void visibilityChanged(document.visibilityState === "hidden");
    document.addEventListener("visibilitychange", onVisibility);
    const back = onBackButtonPress(goBack).catch(() => null);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      void back.then((listener) => listener?.unregister());
    };
  });

  async function checkForUpdates() {
    try {
      const found = await api.updateCheck();
      if (found) {
        update = found;
        if (updateState !== "ready") {
          updateState = "available";
        }
      }
    } catch {}
  }

  async function startUpdateDownload() {
    if (!update) return;
    updateState = "downloading";
    downloadPercent = 0;
    updateErrorMsg = null;

    let unlisten: (() => void) | undefined;
    try {
      unlisten = await api.onUpdateProgress((p) => {
        if (p.total > 0) {
          downloadPercent = Math.min(100, Math.round((p.downloaded / p.total) * 100));
        }
      });
      const path = await api.updateDownload(update.url, update.sha256);
      downloadedPath = path;
      downloadPercent = 100;
      updateState = "ready";
      await triggerInstall(path);
    } catch (err: unknown) {
      updateState = "error";
      updateErrorMsg = err instanceof Error ? err.message : String(err);
      toast.error("Update download failed. You can retry or download via browser.");
    } finally {
      unlisten?.();
    }
  }

  async function triggerInstall(path: string | null) {
    if (!path) return;
    try {
      await api.updateInstall(path);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(`Install error: ${msg}`);
    }
  }

  async function visibilityChanged(hidden: boolean) {
    if (hidden) {
      away = true;
      void api.appHidden(true).catch(() => {});
      return;
    }
    const locked = await api.appHidden(false).catch(() => false);
    if (locked) {
      // Face ID or the fingerprint asks at once, as on opening the app.
      p.pageOpened();
      await p.lock();
    }
    away = false;
    if (p.status === "unlocked" || locked) void p.syncNow();
    if (!update || updateState === "available" || updateState === "error") {
      void checkForUpdates();
    }
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

  {#if update}
    <div class="update" role="status">
      <div class="update-row">
        <span class="update-msg">
          {#if updateState === "downloading"}
            Downloading v{update.version}... {downloadPercent}%
          {:else if updateState === "ready"}
            v{update.version} ready to install.
          {:else if updateState === "error"}
            {updateErrorMsg || "Update download failed."}
          {:else}
            MYLE Passwords {update.version} is out.
          {/if}
        </span>

        {#if updateState === "available"}
          <button class="btn small primary" onclick={startUpdateDownload}>
            <Download size={14} /> Update
          </button>
        {:else if updateState === "downloading"}
          <span class="spinner"><LoaderCircle size={15} class="spin" /></span>
        {:else if updateState === "ready"}
          <button class="btn small primary" onclick={() => void triggerInstall(downloadedPath)}>
            <CircleCheck size={14} /> Install
          </button>
        {:else if updateState === "error"}
          <button class="btn small secondary" onclick={startUpdateDownload}>Retry</button>
          <button class="btn small ghost" title="Download via browser" onclick={() => void openUrl(update!.url)}>
            Browser
          </button>
        {/if}

        <button class="icon-btn" aria-label="Later" onclick={() => (update = null)}><X size={15} /></button>
      </div>

      {#if updateState === "downloading"}
        <div class="progress-track" aria-hidden="true">
          <div class="progress-fill" style="width: {downloadPercent}%"></div>
        </div>
      {/if}
    </div>
  {/if}

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

  .update {
    display: flex;
    flex-direction: column;
    flex: none;
    gap: 8px;
    padding: 8px 12px 8px 16px;
    background: rgb(var(--accent-rgb) / 0.14);
    border-bottom: 1px solid rgb(var(--accent-rgb) / 0.2);
    font-size: 13.5px;
  }

  .update-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .update-msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }

  .progress-track {
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.12);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s ease-out;
  }

  .spinner {
    display: inline-flex;
    align-items: center;
    color: var(--accent);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
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
