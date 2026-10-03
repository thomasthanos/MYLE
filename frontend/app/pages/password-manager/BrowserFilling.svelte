<script lang="ts">
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

  /** The extension's store page, once it is published (package.json). */
  const storeUrl: string = import.meta.env.VITE_EXTENSION_URL ?? "";

  let { onclose }: { onclose: () => void } = $props();

  let setup = $state<BrowserSetup | null>(null);
  let saving = $state(false);
  let loadError = $state<string | null>(null);
  /** The steps to add the extension, shown until a browser has connected. */
  let showSteps = $state<boolean | null>(null);
  /** Loading it from a folder (for testing), shown on request. */
  let showFolder = $state(false);
  /** The extensions page of the browser that connected. */
  const extensionsPage = $derived(
    { Edge: "edge://extensions", Brave: "brave://extensions" }[setup?.lastContact?.browser ?? ""] ?? "chrome://extensions",
  );
  /** Ticks every half minute, so "2 minutes ago" stays true. */
  let clock = $state(Date.now());
  /** When `setup.now` was the app's time, on this page's clock. */
  let loadedAt = Date.now();

  async function load() {
    try {
      setup = await api.browserGet();
      loadedAt = clock = Date.now();
      loadError = null;
      showSteps ??= !setup.lastContact;
    } catch (error) {
      loadError = error instanceof Error ? error.message : String(error);
    }
  }

  onMount(() => {
    void load();
    const tick = setInterval(() => (clock = Date.now()), 30_000);
    let stop: (() => void) | undefined;
    void api
      .onBrowserContact((contact) => {
        if (setup) setup = { ...setup, lastContact: contact, now: contact.at };
        loadedAt = clock = Date.now();
      })
      .then((unlisten) => (stop = unlisten));
    return () => {
      clearInterval(tick);
      stop?.();
    };
  });

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

  const registered = $derived(!!setup?.enabled && !setup.registrationError);
  const connected = $derived(registered && !!setup?.lastContact);
  /** A start MYLE turned away after the last good one: worth saying. */
  const refusal = $derived(
    setup?.lastRefusal && (!setup.lastContact || setup.lastRefusal.at > setup.lastContact.at) ? setup.lastRefusal : null,
  );

  async function toggle(enabled: boolean) {
    if (!setup) return;
    saving = true;
    try {
      await api.browserSet(enabled);
      await load();
      toast.success(enabled ? "Browser filling is on." : "Browser filling is off.");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      saving = false;
    }
  }

  /** Writes the browsers' registration again. */
  async function repair() {
    saving = true;
    try {
      await api.browserSet(true);
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
      toast.success("Copied.");
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
      <header>
        <h2 id="browser-filling-title">Fill in logins in your browser</h2>
        <button type="button" class="icon-btn" aria-label="Close" onclick={onclose}><X size={16} /></button>
      </header>

      <div class="status" class:good={connected} class:bad={!!setup && setup.enabled && !registered}>
        <span class="status-icon" aria-hidden="true">
          {#if connected}<Check size={16} />{:else if setup?.enabled && !registered}<CircleAlert size={16} />{:else}<CircleDashed size={16} />{/if}
        </span>
        <span class="status-text">
          <strong>
            {#if !setup}Checking…
            {:else if !setup.enabled}Browser filling is off
            {:else if !registered}The browsers cannot reach MYLE
            {:else if connected}Connected to {setup.lastContact?.browser}
            {:else}Waiting for the extension{/if}
          </strong>
          <small>
            {#if loadError}{loadError}
            {:else if !setup}&nbsp;
            {:else if !setup.enabled}Turn it on to fill in logins in Chrome, Edge, Brave and Firefox.
            {:else if !registered}{setup.registrationError}
            {:else if connected && setup.lastContact}Last asked {ago(setup.lastContact.at)}.
            {:else}Add the extension below, then click a sign-in field.{/if}
          </small>
        </span>
        <input
          type="checkbox"
          class="switch"
          role="switch"
          aria-label="Browser filling"
          checked={setup?.enabled ?? false}
          disabled={!setup || saving}
          onchange={(e) => toggle(e.currentTarget.checked)}
        />
      </div>

      {#if setup?.enabled && !connected}
        <ul class="checks">
          <li class:done={registered}>
            {#if registered}<Check size={14} />{:else}<CircleAlert size={14} />{/if}
            <span>Chrome, Edge, Brave and Firefox know where MYLE is</span>
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

      {#if setup?.lastContact?.copy === "folder" && storeUrl.startsWith("https://")}
        <div class="folder-copy">
          <CircleAlert size={14} />
          <span>
            {setup.lastContact.browser} uses the copy loaded from a folder, which never updates. Install the one from the store, then
            remove the folder copy in <button type="button" class="link" onclick={() => copy(extensionsPage)}><code>{extensionsPage}</code></button>.
          </span>
          <button type="button" class="btn small primary" onclick={() => void openUrl(storeUrl).catch(() => {})}><Download size={13} /> Store</button>
        </div>
      {/if}

      {#if refusal}
        <p class="refusal">
          <CircleAlert size={13} />
          <span><code>{refusal.program}</code> tried to connect {ago(refusal.at)}: {refusal.reason}. MYLE works with Chrome, Edge, Brave and Firefox.</span>
        </p>
      {/if}

      <section>
        <button type="button" class="steps-toggle" aria-expanded={!!showSteps} onclick={() => (showSteps = !showSteps)}>
          <h3>Add the extension</h3>
          <span class="chevron" class:open={showSteps}><ChevronDown size={14} /></span>
        </button>
        {#if showSteps}
          {#if storeUrl.startsWith("https://")}
            <div class="store-row">
              <button type="button" class="btn primary store" onclick={() => void openUrl(storeUrl).catch(() => {})}>
                <Download size={14} /> Get MYLE Passwords
              </button>
              <p class="or">
                From the Chrome Web Store, for Chrome, Edge and Brave; it updates itself. In Edge, allow extensions from
                other stores when it asks.
              </p>
            </div>
            <button type="button" class="more" aria-expanded={showFolder} onclick={() => (showFolder = !showFolder)}>
              <span class="chevron" class:open={showFolder}><ChevronDown size={12} /></span>
              Firefox, or a copy from MYLE's folder (for testing)
            </button>
          {/if}
          {#if showFolder || !storeUrl.startsWith("https://")}
          <div class="path">
            <code class="selectable" title={setup?.extensionDir ?? undefined}>{setup?.extensionDir ?? "The extension's folder was not found."}</code>
            {#if setup?.extensionDir}
              <button type="button" class="icon-btn" title="Copy the folder" aria-label="Copy the folder" onclick={() => copy(setup!.extensionDir!)}><Copy size={14} /></button>
              <button type="button" class="btn small" onclick={() => api.openExtensionDir()}><FolderOpen size={13} /> Open</button>
            {/if}
          </div>

          <div class="browsers">
            <div class="browser">
              <strong>Chrome, Edge, Brave</strong>
              <ol>
                <li>
                  Open
                  <button type="button" class="link" onclick={() => copy("chrome://extensions")}><code>chrome://extensions</code></button>
                  (Edge: <button type="button" class="link" onclick={() => copy("edge://extensions")}><code>edge://extensions</code></button>).
                </li>
                <li>Turn on <b>Developer mode</b>.</li>
                <li><b>Load unpacked</b>, and choose the folder above.</li>
              </ol>
            </div>

            <div class="browser">
              <strong>Firefox</strong>
              <ol>
                <li>
                  Open
                  <button type="button" class="link" onclick={() => copy("about:debugging#/runtime/this-firefox")}><code>about:debugging</code></button>
                  → This Firefox.
                </li>
                <li><b>Load Temporary Add-on</b>, and choose <code>firefox/manifest.json</code> in that folder.</li>
              </ol>
              <p class="hint">Firefox keeps it until it restarts; a signed version that stays comes later.</p>
            </div>
          </div>
          {/if}
        {/if}
      </section>

      <p class="hint">
        Click a user name or password field on a sign-in page, and pick the login. Nothing is filled in without your
        click, only on secure (https) pages, and only with logins saved for that site.
      </p>
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
    background: rgb(4 6 12 / 0.58);
  }

  /* Never set overflow on a .glass element: .glass::before sits at inset: -1px
     and would cause a permanent 1px scrollbar. */
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(560px, 100%);
    max-height: calc(100dvh - clamp(24px, 6vh, 48px));
    border-radius: var(--radius-xl);
  }

  .dialog-body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: clamp(10px, 1.6vh, 14px);
    min-height: 0;
    padding: clamp(14px, 2.2vh, 20px);
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none;
    border-radius: inherit;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  header :global(.icon-btn) {
    flex: none;
  }

  h2 {
    min-width: 0;
    margin: 0;
    font-size: 17px;
  }

  h3 {
    margin: 0;
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    padding: 12px 14px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 12px;
    background: rgb(0 0 0 / 0.16);
  }

  .status.good {
    border-color: rgb(74 222 128 / 0.28);
    background: rgb(74 222 128 / 0.07);
  }

  .status.bad {
    border-color: rgb(255 140 120 / 0.3);
    background: rgb(255 140 120 / 0.07);
  }

  .status-icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    color: var(--text-2);
    background: rgb(255 255 255 / 0.06);
  }

  .good .status-icon {
    color: #4ade80;
    background: rgb(74 222 128 / 0.14);
  }

  .bad .status-icon {
    color: #ffb6a8;
    background: rgb(255 140 120 / 0.14);
  }

  .status-text {
    display: grid;
    flex: 1 1 0%;
    min-width: 0;
    gap: 2px;
  }

  .status-text small {
    color: var(--text-3);
    font-size: 12px;
    line-height: 1.4;
    overflow-wrap: break-word;
  }

  .status .switch {
    flex: none;
  }

  .checks {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .checks li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: 26px;
    color: var(--text-3);
    font-size: 12.5px;
  }

  .checks li.done {
    color: var(--text-2);
  }

  .checks li.done :global(svg) {
    color: #4ade80;
  }

  .checks li :global(svg) {
    flex: none;
  }

  .checks li span {
    flex: 1 1 0%;
    min-width: 0;
  }

  .checks li .btn {
    flex: none;
  }

  .refusal {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0;
    color: #ffcf9e;
    font-size: 12px;
    line-height: 1.45;
  }

  .refusal :global(svg) {
    flex: none;
    margin-top: 2px;
  }

  section {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: clamp(8px, 1.4vh, 12px);
    min-width: 0;
  }

  .steps-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 2px 0;
  }

  .chevron {
    display: grid;
    color: var(--text-3);
    transition: transform 160ms ease;
  }

  .chevron.open {
    transform: rotate(180deg);
  }

  .store-row {
    display: grid;
    gap: 6px;
    justify-items: start;
  }

  .store {
    height: 36px;
  }

  .more {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    justify-self: start;
    color: var(--text-3);
    font-size: 12px;
  }

  .more:hover {
    color: var(--text-1);
  }

  .folder-copy {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    border: 1px solid rgb(255 196 102 / 0.3);
    border-radius: 10px;
    background: rgb(255 196 102 / 0.08);
    color: #ffd18f;
    font-size: 12px;
    line-height: 1.45;
  }

  .folder-copy span {
    flex: 1;
    color: var(--text-2);
  }

  .folder-copy :global(svg),
  .folder-copy .btn {
    flex: none;
  }

  .or {
    margin: 0;
    color: var(--text-3);
    font-size: 12px;
  }

  .path {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    padding: 6px 6px 6px 10px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 10px;
    background: rgb(0 0 0 / 0.22);
  }

  .path code {
    flex: 1 1 0%;
    min-width: 0;
    overflow: hidden;
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .path button {
    flex: none;
  }

  /* Side by side when the dialog is wide enough: no scrolling at 1080p. */
  .browsers {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 10px 16px;
    min-width: 0;
  }

  .browser {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-content: start;
    gap: 4px;
    min-width: 0;
  }

  .browser strong {
    font-size: 13px;
  }

  ol {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 4px;
    min-width: 0;
    margin: 2px 0 0;
    padding-left: 20px;
    color: var(--text-2);
    font-size: 12.5px;
    line-height: 1.45;
  }

  li {
    min-width: 0;
    overflow-wrap: break-word;
  }

  code {
    font-family: var(--font-mono);
    font-size: 11.5px;
  }

  .link {
    display: inline;
    padding: 0;
    color: #b9c2ff;
  }

  .link:hover {
    text-decoration: underline;
  }

  .hint {
    min-width: 0;
    margin: 0;
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.5;
    overflow-wrap: break-word;
  }

  @media (max-width: 480px) {
    .path {
      flex-wrap: wrap;
    }

    .path code {
      flex-basis: 100%;
    }
  }
</style>
