<script lang="ts">
  import ConfirmHost from "../lib/components/ConfirmHost.svelte";
  import Toaster from "../lib/components/Toaster.svelte";
  import TooltipHost from "../lib/components/TooltipHost.svelte";
  import ContextMenuHost from "../lib/components/ContextMenuHost.svelte";
  import WhatsNewHost from "../lib/components/WhatsNewHost.svelte";
  import { whatsNew } from "../lib/whats-new.svelte";
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { canOpen, pages, type PageId } from "./pages/registry";
  import { pageForShortcut } from "../lib/sidebar";
  import { nav } from "../lib/nav.svelte";
  import { account } from "./account/account.svelte";
  import { gameSavesState } from "./pages/game-saves/state.svelte";
  import { passwords, type WindowsTarget } from "./pages/password-manager/state.svelte";
  import { preloadApp } from "./preload";
  import ContentArea from "./shell/ContentArea.svelte";
  import Sidebar from "./shell/Sidebar.svelte";
  import Titlebar from "./shell/Titlebar.svelte";

  let maximized = $state(false);

  onMount(() => {
    // Notices game saves changed by playing, whichever page is open.
    gameSavesState.startWatcher();
    // Restores the signed-in account and syncs settings with it.
    void account.init();
    if (!isTauri()) return;
    // Every page's data and pictures, while the splash shows.
    void preloadApp();
    // The notes of the versions an update brought, once per version.
    void whatsNew.checkAfterStart();
    // The browser extension asked for the vault (to unlock it, or when it
    // started the app), or a scheduled backup's notice asked for Game Saves.
    void invoke<PageId | null>("start_page").then((page) => {
      if (!page) return;
      nav.go(page);
      // Started by the extension for a sign-in: it asks to be unlocked.
      if (page === "password-manager") void passwords.wantUnlock();
    });
    const unlisten = listen<PageId>("myle-navigate", (event) => nav.go(event.payload));
    const unlistenUnlock = listen("myle-unlock-wanted", () => {
      nav.go("password-manager");
      void passwords.wantUnlock();
    });
    const unlistenWindows = listen<WindowsTarget>("passwords-windows-target", (event) => {
      passwords.windowsTarget = event.payload;
      nav.go("password-manager");
    });
    return () => {
      gameSavesState.stopWatcher();
      void unlisten.then((off) => off());
      void unlistenUnlock.then((off) => off());
      void unlistenWindows.then((off) => off());
    };
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "b") {
      e.preventDefault();
      nav.toggleSidebar();
      return;
    }
    // Ctrl+1 … Ctrl+9 and Ctrl+, (lib/sidebar.ts), unless a dialog is open.
    const page = pageForShortcut(
      pages.filter((p) => canOpen(p, account.owner)),
      e,
    );
    if (page && !document.querySelector('[aria-modal="true"], dialog[open]')) {
      e.preventDefault();
      nav.go(page.id);
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="shell" class:collapsed={nav.collapsed} class:maximized>
  <div class="app-backdrop" aria-hidden="true"></div>
  <Titlebar bind:maximized />
  <Sidebar />
  <ContentArea />
</div>

<Toaster />
<WhatsNewHost />
<ConfirmHost />
<TooltipHost />
<ContextMenuHost appActions />

<style>
  .shell { --frame-gap: var(--gap); position: relative; isolation: isolate; display: grid; grid-template-areas: "title title" "side main"; grid-template-rows: var(--titlebar-h) minmax(0, 1fr); grid-template-columns: var(--sidebar-w) minmax(0, 1fr); column-gap: var(--gap); row-gap: var(--chrome-gap); width: calc(100vw - var(--window-inset) * 2); height: calc(100vh - var(--window-inset) * 2); margin: var(--window-inset); border-radius: var(--window-radius); padding: 0 var(--frame-gap) var(--frame-gap); overflow: hidden; background: linear-gradient(180deg, var(--window-fill-top), var(--window-fill-bottom)); box-shadow: var(--window-shadow); transition: grid-template-columns var(--dur-med) var(--ease-out); }

  .shell > :global(.app-backdrop) { position: absolute; inset: 0; z-index: 0; border-radius: inherit; }

  .shell > :global(.titlebar),
  .shell > :global(.sidebar),
  .shell > :global(.content) { z-index: 1; }

  /* One continuous masked rim avoids doubled lines and clipped corner highlights. */
  .shell::before { content: ""; position: absolute; z-index: 4; inset: 0; padding: 1px; border-radius: inherit; background: var(--window-border); mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0); mask-composite: exclude; pointer-events: none; }

  .shell.collapsed { grid-template-columns: var(--sidebar-w-collapsed) minmax(0, 1fr); }

  .shell.maximized { --frame-gap: 0px; width: 100vw; height: 100vh; margin: 0; border-radius: 0; box-shadow: none; }

  .shell.maximized::before { opacity: 0; }

  :global(:root.solid) .shell { background: var(--shell-fill); }
</style>
