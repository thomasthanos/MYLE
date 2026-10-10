<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import Database from "@lucide/svelte/icons/database";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { spotifyHubState as hub } from "./state.svelte";

  const phrase = "REMOVE SPOTIFY";
  let typed = $state("");
  let input = $state<HTMLInputElement>();
  let dialog = $state<HTMLDivElement>();
  let now = $state(Date.now());
  const preview = $derived(hub.purgePreview);
  const expired = $derived(preview ? preview.expiresAt <= now : true);
  const ready = $derived(typed === phrase && !expired && !hub.locked);

  onMount(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusFrame = requestAnimationFrame(() => input?.focus());
    const clock = window.setInterval(() => (now = Date.now()), 1_000);
    return () => {
      cancelAnimationFrame(focusFrame);
      clearInterval(clock);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  });

  function focusableElements(): HTMLElement[] {
    if (!dialog) return [];
    return Array.from(
      dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((element) => !element.hasAttribute("hidden") && element.getAttribute("aria-hidden") !== "true");
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !hub.startingAction) {
      event.preventDefault();
      hub.dismissPurgePreview();
      return;
    }
    if (event.key !== "Tab") return;
    const items = focusableElements();
    if (!items.length) {
      event.preventDefault();
      dialog?.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const activeElement = document.activeElement;
    if (event.shiftKey && (activeElement === first || !dialog?.contains(activeElement))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (activeElement === last || !dialog?.contains(activeElement))) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if preview}
  <div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
    <div
      class="dialog glass glass--3"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="purge-title"
      aria-describedby="purge-scope purge-help"
      tabindex="-1"
      bind:this={dialog}
      transition:scale={{ start: 0.97, duration: 170, easing: cubicOut }}
    >
      <div class="dialog-body">
        <header>
          <span class="warning"><TriangleAlert size={21} /></span>
          <span class="heading">
            <h2 id="purge-title">Fully uninstall Spotify?</h2>
            <p>Review the detected installations and local data before continuing.</p>
          </span>
          <button class="icon-btn" aria-label="Close" onclick={() => hub.dismissPurgePreview()}><X size={16} /></button>
        </header>

        <div class="preview">
          <section class="surface">
            <h3><HardDrive size={14} /> Detected installations</h3>
            {#if preview.detectedVariants.length}
              <ul>
                {#each preview.detectedVariants as variant (variant)}<li>{variant}</li>{/each}
              </ul>
            {:else}
              <p class="muted">No supported Spotify installation was detected.</p>
            {/if}
          </section>
          <section class="surface">
            <h3><Database size={14} /> Data to remove</h3>
            <ul>
              {#each preview.categories as category (category)}<li>{category}</li>{/each}
            </ul>
          </section>
        </div>

        {#if preview.warnings.length}
          <div class="warnings">
            {#each preview.warnings as warning (warning)}<p><TriangleAlert size={13} /> {warning}</p>{/each}
          </div>
        {/if}

        <p id="purge-scope" class="scope">
          This removes offline downloads, sessions and local settings for the active Windows profile. It does not delete
          your Spotify account, playlists or cloud library.
        </p>

        <label>
          <span>Type <strong>{phrase}</strong> to confirm</span>
          <input
            bind:this={input}
            bind:value={typed}
            class="input"
            autocomplete="off"
            spellcheck="false"
            placeholder={phrase}
            aria-describedby="purge-help"
          />
        </label>
        <p id="purge-help" class="best-effort">Stop is best-effort between stages; completed deletions cannot be restored.</p>

        {#if expired}<p class="expired" role="status">This preview expired. Close it and review the current state again.</p>{/if}

        <footer>
          <button class="btn" onclick={() => hub.dismissPurgePreview()}>Cancel</button>
          <button class="btn danger" disabled={!ready} onclick={() => hub.confirmPurge()}>Remove Spotify</button>
      </footer>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 22px; background: rgb(4 6 12 / 0.62); }

  /* No overflow on the glass element: see BiosRestartDialog. */
  .dialog { display: flex; flex-direction: column; width: min(640px, 100%); max-height: calc(100vh - 44px); border-radius: var(--radius-xl); }

  .dialog-body { display: grid; gap: 14px; min-height: 0; padding: 20px; overflow: auto; border-radius: inherit; }

  header { display: flex; align-items: flex-start; gap: 10px; }
  .warning { display: grid; place-items: center; width: 35px; height: 35px; flex: none; border: 1px solid rgb(229 72 77 / 0.25); border-radius: 10px; background: rgb(229 72 77 / 0.1); color: #ff7778; }
  .heading { flex: 1; }
  h2 { font-size: 17px; }
  header p { margin-top: 3px; color: var(--text-3); font-size: 11.5px; }

  .preview { display: grid; grid-template-columns: 1fr 1fr; gap: 9px; }
  section { min-width: 0; padding: 12px; }
  h3 { display: flex; align-items: center; gap: 7px; color: var(--text-2); font-size: 11px; letter-spacing: 0.04em; text-transform: uppercase; }
  ul { display: grid; gap: 4px; margin: 9px 0 0; padding-left: 17px; color: var(--text-2); font-size: 11.5px; }
  li::marker { color: rgb(229 72 77 / 0.8); }
  .muted { margin-top: 9px; color: var(--text-3); font-size: 11.5px; }

  .warnings { display: grid; gap: 5px; padding: 9px 11px; border: 1px solid rgb(245 180 84 / 0.2); border-radius: 10px; background: rgb(245 180 84 / 0.07); }
  .warnings p { display: flex; align-items: flex-start; gap: 7px; color: rgb(255 205 126 / 0.86); font-size: 11px; }
  .warnings :global(svg) { flex: none; margin-top: 1px; }

  .scope { color: var(--text-2); font-size: 11.5px; line-height: 1.5; }
  label { display: grid; gap: 6px; color: var(--text-2); font-size: 11.5px; }
  label strong { color: #ff8b8d; font-family: var(--font-mono); font-size: 11px; }
  label .input { width: 100%; border-color: rgb(229 72 77 / 0.25); font-family: var(--font-mono); letter-spacing: 0.04em; }
  label .input:focus { border-color: rgb(229 72 77 / 0.58); }
  .best-effort { color: var(--text-3); font-size: 10.5px; }
  .expired { color: rgb(255 205 126 / 0.9); font-size: 10.5px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }

  @media (max-width: 620px) {
    .preview { grid-template-columns: 1fr; }
  }
</style>
