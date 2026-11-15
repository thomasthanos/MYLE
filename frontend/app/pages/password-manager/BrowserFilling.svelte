<script lang="ts">
  import LocalExtensionUpdate from "../../../lib/components/LocalExtensionUpdate.svelte";
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleDashed from "@lucide/svelte/icons/circle-dashed";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Wrench from "@lucide/svelte/icons/wrench";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { toast } from "../../../lib/toast.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { passwordsApi as api, type BrowserSetup } from "./api";
  import { EXTENSION_URL, FIREFOX_EXTENSION_URL } from "../../../lib/extension";

  /** The extension's store page (package.json "myle.extensionUrl"). */
  const storeUrl = EXTENSION_URL;
  /** The extension's Firefox Add-ons page (package.json "myle.firefoxExtensionUrl"). */
  const firefoxStoreUrl = FIREFOX_EXTENSION_URL;

  let { onclose, local = false }: { onclose: () => void; local?: boolean } = $props();

  let setup = $state<BrowserSetup | null>(null);
  let saving = $state(false);
  let loadError = $state<string | null>(null);
  /* Loading the extension from MYLE's folder (Firefox, or for testing): shown on request, or at once when there is no store page. */
  let showSteps = $state<boolean | null>(null);
  /** The extensions page of the browser that connected. */
  const extensionsPage = $derived(
    { Edge: "edge://extensions", Brave: "brave://extensions", Vivaldi: "vivaldi://extensions", Opera: "opera://extensions" }[setup?.lastContact?.browser ?? ""] ?? "chrome://extensions",
  );
  const firefox = $derived(setup?.lastContact?.copy === "firefox");
  /** Ticks every half minute, so "2 minutes ago" stays true. */
  let clock = $state(Date.now());
  /** When `setup.now` was the app's time, on this page's clock. */
  let loadedAt = Date.now();

  async function load() {
    try {
      setup = await api.browserGet();
      loadedAt = clock = Date.now();
      loadError = null;
      showSteps ??= local || (!storeUrl.startsWith("https://") && !firefoxStoreUrl.startsWith("https://"));
    } catch (error) {
      loadError = error instanceof Error ? error.message : String(error);
    }
  }

  onMount(() => {
    if (local) showSteps = true;
    void load();
    const tick = setInterval(() => (clock = Date.now()), 30_000);
    let stop: (() => void) | undefined;
    // Closed before the listener was in place: it is removed as soon as it is.
    let closed = false;
    void api
      .onBrowserContact((contact) => {
        if (setup) setup = { ...setup, lastContact: contact, now: contact.at };
        loadedAt = clock = Date.now();
      })
      .then((unlisten) => (closed ? unlisten() : (stop = unlisten)));
    return () => {
      closed = true;
      clearInterval(tick);
      stop?.();
    };
  });

  /** The extension's page in the Chrome Web Store, in the default browser. */
  async function openStore() {
    try {
      await openUrl(storeUrl);
    } catch (error) {
      toast.error(`Could not open the Chrome Web Store: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  /** The extension's page in Firefox Add-ons (AMO), in the default browser. */
  async function openFirefoxStore() {
    try {
      await openUrl(firefoxStoreUrl);
    } catch (error) {
      toast.error(`Could not open Firefox Add-ons: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async function openLocal() {
    try {
      await api.openExtensionDir();
      showSteps = true;
      await load();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  }

  function ago(at: number): string {
    if (!setup) return "";
    const seconds = Math.max(0, setup.now + Math.round((clock - loadedAt) / 1000) - at);
    if (seconds < 60) return "just now";
    const minutes = Math.round(seconds / 60);
    if (minutes < 60) return minutes === 1 ? "a minute ago" : `${minutes} minutes ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 48) return hours === 1 ? "an hour ago" : `${hours} hours ago`;
    return `${Math.round(hours / 24)} days ago`;
  }

  /** MYLE's own end of the bridge is not taking the browsers' requests. */
  const notListening = $derived(!!setup?.enabled && setup.serverListening === false);
  const registered = $derived(!!setup?.enabled && !setup.registrationError);
  const connected = $derived(registered && !!setup?.lastContact && !notListening && !setup?.elevated);
  /** A start MYLE turned away after the last good one: worth saying. */
  const refusal = $derived(
    setup?.lastRefusal && (!setup.lastContact || setup.lastRefusal.at > setup.lastContact.at) ? setup.lastRefusal : null,
  );

  async function toggle(enabled: boolean, input: HTMLInputElement) {
    if (!setup) return;
    saving = true;
    try {
      await api.browserSet(enabled);
      await load();
      toast.success(enabled ? "Browser filling is on." : "Browser filling is off.");
    } catch (error) {
      input.checked = setup.enabled;
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      saving = false;
    }
  }

  /** Writes the browsers' registration again (no setting changes). */
  async function repair() {
    saving = true;
    try {
      await api.browserRepair();
      await load();
      if (setup && !setup.registrationError) toast.success("The browsers know where MYLE is again.");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      saving = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied to clipboard.");
    } catch {
      toast.error("Could not copy. Select the text and copy it instead.");
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div
    class="dialog glass glass--3"
    role="dialog"
    aria-modal="true"
    aria-labelledby="browser-filling-title"
    transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}
  >
    <div class="dialog-body">
      <!-- Header -->
      <header class="dialog-header">
        <div class="title-wrap">
          <h2 id="browser-filling-title">Fill in logins in your browser</h2>
          <span class="header-subtitle">Browser auto-fill integration</span>
        </div>
        <button type="button" class="icon-btn close-btn" aria-label="Close" onclick={onclose}>
          <X size={16} />
        </button>
      </header>

      <!-- Connection Status Card -->
      <div class="status-card" class:good={connected} class:bad={!!setup && setup.enabled && !registered}>
        <div class="status-left">
          <span class="status-icon" aria-hidden="true">
            {#if connected}
              <Check size={17} strokeWidth={2.4} />
            {:else if setup?.enabled && !registered}
              <CircleAlert size={17} />
            {:else}
              <CircleDashed size={17} />
            {/if}
          </span>
          <div class="status-info">
            <div class="status-title-row">
              <strong>
                {#if !setup}Checking connection…
                {:else if !setup.enabled}Browser filling is off
                {:else if !registered}The browsers cannot reach MYLE
                {:else if connected}Connected to {setup.lastContact?.browser}
                {:else}Waiting for extension…{/if}
              </strong>
              {#if connected && setup?.lastContact?.copy === "folder"}
                <span class="badge local-badge">Local files active</span>
              {/if}
            </div>
            <p class="status-desc">
              {#if loadError}{loadError}
              {:else if !setup}&nbsp;
              {:else if !setup.enabled}Turn it on to fill in logins in Chrome, Edge, Brave, Vivaldi, Opera, Firefox, Zen, LibreWolf and other Chromium or Firefox browsers.
              {:else if !registered}{setup.registrationError}
              {:else if connected && setup.lastContact}Last active {ago(setup.lastContact.at)}. Secure end-to-end bridge.
              {:else}Add the extension below, then click any sign-in field.{/if}
            </p>
          </div>
        </div>
        <input
          type="checkbox"
          class="switch"
          role="switch"
          aria-label="Browser filling"
          checked={setup?.enabled ?? false}
          disabled={!setup || saving}
          onchange={(e) => toggle(e.currentTarget.checked, e.currentTarget)}
        />
      </div>

      {#if setup?.elevated}
        <div class="alert-box warning">
          <CircleAlert size={15} />
          <span>MYLE runs as administrator, so browsers cannot connect. Close MYLE and run it normally.</span>
        </div>
      {:else if notListening}
        <div class="alert-box warning">
          <CircleAlert size={15} />
          <span>MYLE is not taking browser requests{setup?.serverError ? ` (${setup.serverError})` : ""}.</span>
          <button type="button" class="btn small" disabled={saving} onclick={repair}><Wrench size={13} /> Repair</button>
        </div>
      {/if}

      {#if setup?.enabled && !connected}
        <ul class="checks">
          <li class:done={registered}>
            {#if registered}<Check size={14} />{:else}<CircleAlert size={14} />{/if}
            <span>Your Chromium and Firefox browsers know where MYLE is</span>
            {#if !registered}
              <button type="button" class="btn small" disabled={saving} onclick={repair}><Wrench size={13} /> Repair</button>
            {/if}
          </li>
          <li class:done={!!setup.lastContact}>
            {#if setup.lastContact}<Check size={14} />{:else}<CircleDashed size={14} />{/if}
            <span>
              {#if setup.lastContact}{setup.lastContact.browser} asked {ago(setup.lastContact.at)}
              {:else}The extension has not asked yet{/if}
            </span>
          </li>
          <li class:done={setup.vault === "unlocked"}>
            {#if setup.vault === "unlocked"}<Check size={14} />{:else}<CircleDashed size={14} />{/if}
            <span>{setup.vault === "unlocked" ? "Your vault is unlocked" : "Your vault opens when you unlock it here"}</span>
          </li>
        </ul>
      {/if}

      {#if refusal}
        <div class="alert-box warning">
          <CircleAlert size={14} />
          <span><code>{refusal.program}</code> tried to connect {ago(refusal.at)}: {refusal.reason}. MYLE works with Chrome, Edge, Brave, Vivaldi, Opera, Arc, Thorium, Firefox, Zen, LibreWolf, Waterfox and Floorp.</span>
        </div>
      {/if}

      <!-- Store Options -->
      {#if storeUrl.startsWith("https://") || firefoxStoreUrl.startsWith("https://")}
        <div class="store-row">
          <div class="store-buttons">
            {#if !firefox && storeUrl.startsWith("https://")}
              <button type="button" class="btn primary store" onclick={openStore}>
                <Download size={15} /> Install from Chrome Web Store
              </button>
            {/if}
            {#if firefoxStoreUrl.startsWith("https://")}
              <button type="button" class="btn primary store" onclick={openFirefoxStore}>
                <Download size={15} /> Install from Firefox Add-ons
              </button>
            {/if}
          </div>
          <p class="or">
            MYLE Passwords for Chrome, Edge, Brave, Vivaldi, Opera and other Chromium browsers; it updates itself. In Edge, allow extensions from other stores when it asks. In Firefox, install from Firefox Add-ons.
          </p>
        </div>
      {/if}

      <!-- Local Extension Update if available -->
      <LocalExtensionUpdate {setup} />

      <!-- Local Extension Setup Section -->
      <section class="local-setup-section">
        <button
          type="button"
          class="steps-toggle"
          aria-expanded={!!showSteps}
          onclick={() => (showSteps = !showSteps)}
        >
          <div class="toggle-title">
            <FolderOpen size={16} />
            <h3>{storeUrl.startsWith("https://") || firefoxStoreUrl.startsWith("https://") ? "Or load unpacked from MYLE's folder" : "Add the extension"}</h3>
          </div>
          <span class="chevron" class:open={showSteps}><ChevronDown size={15} /></span>
        </button>

        {#if showSteps}
          <!-- Path Bar -->
          <div class="folder-card">
            <div class="folder-header">
              <span class="folder-title">Extension Folder (Local Files)</span>
              <span class="folder-badge">Ready</span>
            </div>
            <div class="path-row">
              <code class="path-code" title={setup?.extensionDir ?? undefined}>
                {setup?.extensionDir ?? "The extension's folder was not found."}
              </code>
              {#if setup?.extensionDir}
                <button
                  type="button"
                  class="btn small secondary"
                  title="Copy the folder path"
                  onclick={() => copy(setup!.extensionDir!)}
                >
                  <Copy size={13} /> Copy Path
                </button>
                <button
                  type="button"
                  class="btn small primary"
                  title="Open folder in Windows Explorer"
                  onclick={openLocal}
                >
                  <FolderOpen size={13} /> Open Folder
                </button>
              {/if}
            </div>
          </div>

          <!-- Browser Cards Side by Side -->
          <div class="browser-grid">
            <!-- Chromium Card -->
            <div class="browser-card">
              <div class="browser-head">
                <span class="browser-icon">🌐</span>
                <span class="browser-name">Chrome, Edge, Brave, Vivaldi, Opera</span>
              </div>
              <div class="steps-list">
                <div class="step-item">
                  <span class="step-num">1</span>
                  <div class="step-content">
                    <span>Open extensions page:</span>
                    <div class="url-chips">
                      {#if extensionsPage && extensionsPage !== "chrome://extensions"}
                        <button type="button" class="chip active-browser-chip" onclick={() => copy(extensionsPage)}>
                          <Copy size={11} /> {extensionsPage}
                        </button>
                      {/if}
                      <button type="button" class="chip" onclick={() => copy("chrome://extensions")}>
                        <Copy size={11} /> chrome://extensions
                      </button>
                      <button type="button" class="chip" onclick={() => copy("edge://extensions")}>
                        <Copy size={11} /> edge://extensions
                      </button>
                      <button type="button" class="chip" onclick={() => copy("vivaldi://extensions")}>
                        <Copy size={11} /> vivaldi://extensions
                      </button>
                      <button type="button" class="chip" onclick={() => copy("opera://extensions")}>
                        <Copy size={11} /> opera://extensions
                      </button>
                    </div>
                  </div>
                </div>
                <div class="step-item">
                  <span class="step-num">2</span>
                  <div class="step-content">
                    <span>Turn on <b>Developer mode</b> (toggle at top right).</span>
                  </div>
                </div>
                <div class="step-item">
                  <span class="step-num">3</span>
                  <div class="step-content">
                    <span>Click <b>Load unpacked</b>, and choose the folder above.</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- Firefox Card -->
            <div class="browser-card">
              <div class="browser-head">
                <span class="browser-icon">🦊</span>
                <span class="browser-name">Firefox, Zen, LibreWolf, Waterfox, Floorp</span>
              </div>
              <div class="steps-list">
                <div class="step-item">
                  <span class="step-num">1</span>
                  <div class="step-content">
                    <span>Open debugging page:</span>
                    <div class="url-chips">
                      <button type="button" class="chip" onclick={() => copy("about:debugging#/runtime/this-firefox")}>
                        <Copy size={11} /> about:debugging
                      </button>
                    </div>
                    <small class="step-sub">→ Click "This Firefox" (or This Zen, This LibreWolf).</small>
                  </div>
                </div>
                <div class="step-item">
                  <span class="step-num">2</span>
                  <div class="step-content">
                    <span>Click <b>Load Temporary Add-on</b>.</span>
                  </div>
                </div>
                <div class="step-item">
                  <span class="step-num">3</span>
                  <div class="step-content">
                    <span>Choose <code>firefox/manifest.json</code> in that folder.</span>
                  </div>
                </div>
              </div>
              <p class="firefox-note">Local installation bundled with MYLE. Works seamlessly while browser is open.</p>
            </div>
          </div>
        {/if}
      </section>

      <!-- Discreet Footer Hint -->
      <footer class="dialog-footer">
        <p class="hint">
          Click any user name or password field on a sign-in page to fill logins. Your data is decrypted only locally on secure pages.
        </p>
      </footer>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 91;
    display: grid;
    place-items: center;
    padding: clamp(12px, 3vh, 24px);
    background: var(--scrim, rgba(0, 0, 0, 0.72));
    backdrop-filter: blur(8px);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(620px, 100%);
    max-height: calc(100dvh - clamp(24px, 6vh, 48px));
    border-radius: var(--radius-xl, 18px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: linear-gradient(165deg, rgba(26, 32, 48, 0.88) 0%, rgba(14, 18, 28, 0.96) 100%);
    box-shadow: 0 24px 60px -10px rgba(0, 0, 0, 0.8),
                0 0 35px rgba(56, 189, 248, 0.12);
    overflow: hidden;
  }

  .dialog-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 22px 24px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .dialog-body::-webkit-scrollbar {
    width: 5px;
  }
  .dialog-body::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.15);
    border-radius: 9999px;
  }

  /* Header */
  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .title-wrap {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  h2 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 700;
    color: #f8fafc;
  }

  .header-subtitle {
    font-size: 0.8rem;
    color: var(--text-muted, #94a3b8);
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-muted, #94a3b8);
    cursor: pointer;
    transition: all 160ms ease;
  }
  .close-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.1);
  }

  /* Status Card */
  .status-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 14px 18px;
    border-radius: 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    transition: all 200ms ease;
  }

  .status-card.good {
    border-color: rgba(74, 222, 128, 0.32);
    background: linear-gradient(135deg, rgba(74, 222, 128, 0.08) 0%, rgba(74, 222, 128, 0.02) 100%);
    box-shadow: 0 4px 20px rgba(74, 222, 128, 0.06);
  }

  .status-card.bad {
    border-color: rgba(248, 113, 113, 0.35);
    background: rgba(248, 113, 113, 0.06);
  }

  .status-left {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .status-icon {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.06);
    color: var(--text-muted, #94a3b8);
  }

  .status-card.good .status-icon {
    color: #4ade80;
    background: rgba(74, 222, 128, 0.16);
    box-shadow: 0 0 12px rgba(74, 222, 128, 0.3);
  }

  .status-card.bad .status-icon {
    color: #f87171;
    background: rgba(248, 113, 113, 0.16);
  }

  .status-info {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .status-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .status-title-row strong {
    font-size: 0.95rem;
    font-weight: 650;
    color: #f1f5f9;
  }

  .local-badge {
    padding: 2px 8px;
    border-radius: 9999px;
    font-size: 0.72rem;
    font-weight: 600;
    background: rgba(56, 189, 248, 0.14);
    border: 1px solid rgba(56, 189, 248, 0.3);
    color: #38bdf8;
  }

  .status-desc {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted, #94a3b8);
    line-height: 1.4;
  }

  /* Checks list */
  .checks {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .checks li {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.82rem;
    color: var(--text-muted, #94a3b8);
  }

  .checks li.done {
    color: #cbd5e1;
  }
  .checks li.done :global(svg) {
    color: #4ade80;
  }

  /* Alert box */
  .alert-box {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-radius: 10px;
    font-size: 0.82rem;
    line-height: 1.4;
  }

  .alert-box.warning {
    border: 1px solid rgba(251, 191, 36, 0.3);
    background: rgba(251, 191, 36, 0.08);
    color: #fde68a;
  }

  /* Store Row */
  .store-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 16px;
    border-radius: 12px;
    border: 1px solid rgba(56, 189, 248, 0.2);
    background: rgba(56, 189, 248, 0.04);
  }

  .store-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
  }

  .store-row .store {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    font-size: 0.88rem;
    font-weight: 600;
  }

  .store-row .or {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted, #94a3b8);
    line-height: 1.4;
  }

  /* Local Setup Section */
  .local-setup-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .steps-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-radius: 11px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    color: inherit;
    cursor: pointer;
    transition: background 160ms ease, border-color 160ms ease;
  }

  .steps-toggle:hover {
    background: rgba(255, 255, 255, 0.06);
    border-color: rgba(255, 255, 255, 0.13);
  }

  .toggle-title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #e2e8f0;
  }

  .toggle-title h3 {
    margin: 0;
    font-size: 0.85rem;
    font-weight: 650;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .chevron {
    display: grid;
    place-items: center;
    color: var(--text-muted, #94a3b8);
    transition: transform 180ms ease;
  }
  .chevron.open {
    transform: rotate(180deg);
  }

  /* Folder Card */
  .folder-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 16px;
    border-radius: 12px;
    border: 1px solid rgba(255, 255, 255, 0.09);
    background: rgba(0, 0, 0, 0.25);
  }

  .folder-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .folder-title {
    font-size: 0.78rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted, #94a3b8);
  }

  .folder-badge {
    padding: 1px 7px;
    border-radius: 9999px;
    font-size: 0.7rem;
    font-weight: 600;
    background: rgba(34, 197, 94, 0.15);
    color: #4ade80;
  }

  .path-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .path-code {
    flex: 1 1 200px;
    min-width: 0;
    padding: 7px 10px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.06);
    color: #cbd5e1;
    font-family: var(--font-mono, monospace);
    font-size: 0.8rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Browser Grid */
  .browser-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
    gap: 12px;
  }

  .browser-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px;
    border-radius: 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.025);
  }

  .browser-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .browser-icon {
    font-size: 1.1rem;
  }

  .browser-name {
    font-size: 0.88rem;
    font-weight: 650;
    color: #f1f5f9;
  }

  .steps-list {
    display: flex;
    flex-direction: column;
    gap: 9px;
  }

  .step-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .step-num {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: rgba(56, 189, 248, 0.15);
    border: 1px solid rgba(56, 189, 248, 0.3);
    color: #38bdf8;
    font-size: 0.72rem;
    font-weight: 700;
  }

  .step-content {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 0.82rem;
    color: #cbd5e1;
    line-height: 1.4;
  }

  .step-sub {
    font-size: 0.75rem;
    color: var(--text-muted, #94a3b8);
  }

  .url-chips {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 3px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border-radius: 6px;
    border: 1px solid rgba(56, 189, 248, 0.25);
    background: rgba(56, 189, 248, 0.08);
    color: #38bdf8;
    font-family: var(--font-mono, monospace);
    font-size: 0.75rem;
    cursor: pointer;
    transition: all 140ms ease;
  }
  .chip:hover {
    background: rgba(56, 189, 248, 0.18);
    border-color: rgba(56, 189, 248, 0.45);
    color: #fff;
  }

  .firefox-note {
    margin: 4px 0 0;
    font-size: 0.75rem;
    color: var(--text-muted, #94a3b8);
    line-height: 1.35;
  }

  /* Footer */
  .dialog-footer {
    padding-top: 4px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
  }

  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: var(--text-muted, #94a3b8);
    line-height: 1.45;
  }
</style>
