<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import X from "@lucide/svelte/icons/x";
  import { passwords as p } from "../app/pages/password-manager/state.svelte";
  import { shell } from "./shell.svelte";
  import AboutCard from "./settings/AboutCard.svelte";
  import FillingCard from "./settings/FillingCard.svelte";
  import SecurityCard from "./settings/SecurityCard.svelte";
  import SyncCard from "./settings/SyncCard.svelte";

  const close = () => (shell.settingsOpen = false);
  /** A dialog opened from here (Change master password) sits over the sheet
   *  and closes first. */
  const dialogOver = () => !!document.querySelector('.backdrop > [role="dialog"]');

  function focusSheet(node: HTMLElement) {
    const previous = document.activeElement as HTMLElement | null;
    const controls = () => Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input:not(:disabled), [tabindex="0"]'));
    controls()[0]?.focus({ preventScroll: true });
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !dialogOver()) { event.preventDefault(); close(); }
      if (event.key !== "Tab") return;
      const items = controls();
      const first = items[0], last = items.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    node.addEventListener("keydown", keydown);
    return { destroy() { node.removeEventListener("keydown", keydown); previous?.focus({ preventScroll: true }); } };
  }
</script>

{#if shell.settingsOpen}
  <div class="backdrop" role="presentation" onclick={close} transition:fade={{ duration: 140 }}></div>
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="settings-title" tabindex="-1" use:focusSheet transition:fly={{ y: 40, duration: 200, easing: cubicOut }}>
    <div class="sheet-heading"><h2 id="settings-title">Settings</h2><button class="icon-btn" aria-label="Close settings" onclick={close}><X size={20} /></button></div>
    {#if p.status === "locked" || p.status === "unlocked"}<SecurityCard />{/if}
    <FillingCard />
    <SyncCard />
    <AboutCard />
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 60; background: rgb(0 0 0 / 0.5); }

  .sheet { position: fixed; right: 0; bottom: 0; left: 0; z-index: 61; display: grid; gap: 14px; max-height: calc(100dvh - env(safe-area-inset-top) - 16px); overflow-y: auto; overscroll-behavior: contain; scroll-padding-top: 72px; padding: 10px max(14px, env(safe-area-inset-right)) calc(env(safe-area-inset-bottom) + 20px) max(14px, env(safe-area-inset-left)); border-top: 1px solid rgb(255 255 255 / 0.1); border-radius: 20px 20px 0 0; background: var(--bg-1); }

  .sheet-heading { position: sticky; top: -10px; z-index: 1; display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: -10px -2px 0; padding: 10px 2px 6px; background: var(--bg-1); }

  .sheet-heading h2 { margin: 0; font-family: var(--font-display); font-size: 18px; }

  .sheet-heading .icon-btn { width: 48px; height: 48px; flex: none; }
</style>
