<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Pin from "@lucide/svelte/icons/pin";
  import X from "@lucide/svelte/icons/x";
  import { toast } from "../toast.svelte";

  type Offer = { pending: boolean; supported: boolean; requestAfterSetup: boolean };
  type PinResult = { status: "pinned" | "notPinned" } | { status: "notAllowed"; reason: "pinConsentNotificationsDisabled" | "unknown" };
  let offer = $state<Offer | null>(null);
  let busy = $state(false);
  let manual = $state(false);
  let notificationsDisabled = $state(false);
  let autoRequested = false;

  function afterFocus(focused: boolean) {
    // The setup's checked taskbar option is the explicit request. Wait until MYLE is in front; regular launches and passive updates never auto-prompt.
    if (focused && offer?.pending && offer.supported && offer.requestAfterSetup && !autoRequested) {
      autoRequested = true;
      void pin();
    }
  }

  onMount(() => {
    if (!isTauri()) return;
    let disposed = false;
    const window = getCurrentWindow();
    const unlisten = window.onFocusChanged((event) => afterFocus(event.payload));
    void invoke<Offer>("taskbar_pin_offer").then(async (value) => {
      if (disposed) return;
      offer = value;
      afterFocus(await window.isFocused());
    }).catch(() => {});
    return () => {
      disposed = true;
      void unlisten.then((off) => off()).catch(() => {});
    };
  });

  async function pin() {
    if (busy) return;
    busy = true;
    try {
      const result = await invoke<PinResult>("taskbar_pin");
      if (result.status === "pinned") {
        offer = null;
        toast.success("MYLE is pinned to your taskbar.");
      } else if (result.status === "notAllowed") {
        manual = true;
        notificationsDisabled = result.reason === "pinConsentNotificationsDisabled";
      } else {
        manual = false;
        notificationsDisabled = false;
        toast.info("Windows did not pin MYLE. You can retry or dismiss this suggestion.");
      }
    } catch (error) {
      toast.error(String(error));
    } finally { busy = false; }
  }

  async function openNotificationSettings() {
    try {
      await invoke("taskbar_open_notification_settings");
    } catch (error) { toast.error(String(error)); }
  }

  async function dismiss() {
    try {
      await invoke("taskbar_pin_dismiss");
      offer = null;
    } catch (error) { toast.error(String(error)); }
  }
</script>

{#if offer?.pending}
  <aside class="pin-offer" aria-label="Pin MYLE to the taskbar">
    <Pin size={18} />
    <div>
      <strong>Keep MYLE on your taskbar</strong>
      {#if offer.supported && !manual}
        <p>Pin it for quick access. Windows will ask you to confirm.</p>
        <button class="btn small" disabled={busy} onclick={pin}>{busy ? "Waiting for Windows…" : "Pin to taskbar"}</button>
      {:else}
        {#if manual}
          {#if notificationsDisabled}
            <p>Windows app pinning notifications are switched off. In System → Notifications, turn on “Apps” and try again.</p>
            <button class="btn small" disabled={busy} onclick={openNotificationSettings}>Open notification settings</button>
          {:else}
            <p>Windows is not allowing its pin confirmation right now.</p>
          {/if}
        {/if}
        <p>Open the Windows Start menu, right-click MYLE and choose More → Pin to taskbar.</p>
        {#if offer.supported}
          <button class="btn small" disabled={busy} onclick={pin}>{busy ? "Waiting for Windows…" : "Try again"}</button>
        {/if}
      {/if}
    </div>
    <button class="dismiss" aria-label="Dismiss taskbar suggestion" disabled={busy} onclick={dismiss}><X size={15} /></button>
  </aside>
{/if}

<style>
  .pin-offer { position: fixed; right: 24px; bottom: 24px; z-index: 30; display: flex; align-items: flex-start; gap: 12px; width: min(380px, calc(100vw - 48px)); padding: 16px; border: 1px solid var(--glass-border); border-radius: var(--radius-lg); background: var(--float-bg); box-shadow: var(--elev-2); color: var(--text-2); }
  .pin-offer > :global(svg) { flex: none; margin-top: 2px; color: var(--accent); }
  strong { color: var(--text-1); font-size: 13px; }
  p { margin: 5px 0 10px; font-size: 12px; line-height: 1.5; }
  .dismiss { flex: none; margin-left: auto; color: var(--text-3); }
  .dismiss:hover { color: var(--text-1); }
</style>
