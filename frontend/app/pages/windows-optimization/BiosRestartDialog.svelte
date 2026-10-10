<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Cpu from "@lucide/svelte/icons/cpu";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import { portal } from "../../../lib/portal";
  import { windowsOptimizationState as tools } from "./state.svelte";

  let dialog = $state<HTMLDivElement>();
  let cancelButton = $state<HTMLButtonElement>();

  $effect(() => {
    if (!tools.firmwareRestartBusy) return;
    const focusFrame = requestAnimationFrame(() => dialog?.focus());
    return () => cancelAnimationFrame(focusFrame);
  });

  onMount(() => {
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusFrame = requestAnimationFrame(() => cancelButton?.focus());
    return () => {
      cancelAnimationFrame(focusFrame);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  });

  function focusableElements(): HTMLElement[] {
    if (!dialog) return [];
    return Array.from(
      dialog.querySelectorAll<HTMLElement>(
        'button:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
      ),
    ).filter(
      (element) =>
        !element.hasAttribute("hidden") &&
        element.getAttribute("aria-hidden") !== "true",
    );
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !tools.firmwareRestartBusy) {
      event.preventDefault();
      tools.dismissBiosDialog();
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
    if (
      event.shiftKey &&
      (activeElement === first || !dialog?.contains(activeElement))
    ) {
      event.preventDefault();
      last.focus();
    } else if (
      !event.shiftKey &&
      (activeElement === last || !dialog?.contains(activeElement))
    ) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" {@attach portal}>
  <div
    class="dialog glass glass--3"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="bios-restart-title"
    aria-describedby="bios-restart-description bios-restart-notice bios-bitlocker-note"
    aria-busy={tools.firmwareRestartBusy}
    tabindex="-1"
    bind:this={dialog}
  >
    <span class="top-rim" aria-hidden="true"></span>

    <!-- Scrolling lives here, never on the glass element itself: its 1px rim
         (.glass::before, inset -1px) would make that element scroll by 1px
         and draw a second, offset border on the right and bottom. -->
    <div class="dialog-body">
      <header>
        <span class="chip-icon"><Cpu size={21} /></span>
        <div class="heading">
          <div class="title-row">
            <h2 id="bios-restart-title">BIOS / UEFI Settings</h2>
            <span class="firmware-badge">BIOS / UEFI</span>
          </div>
          <p id="bios-restart-description">
            Restart this PC and open its firmware settings automatically during startup.
          </p>
        </div>
      </header>

      <section class="steps surface" aria-labelledby="bios-steps-title">
        <h3 id="bios-steps-title">What will happen</h3>
        <ol>
          <li><span>1</span><p>Save your work and close any open applications.</p></li>
          <li><span>2</span><p>Windows will restart automatically.</p></li>
          <li><span>3</span><p>The BIOS / UEFI settings menu will open during startup.</p></li>
        </ol>
      </section>

      <div id="bios-restart-notice" class="notice" role="note">
        <CircleAlert size={16} />
        <div>
          <strong>Important notice</strong>
          <p>
            Administrator approval is required. After approval, Windows will request an
            immediate restart without a forced application close.
          </p>
        </div>
      </div>

      <div id="bios-bitlocker-note" class="bitlocker" role="note">
        <KeyRound size={15} />
        <p>
          Keep your BitLocker recovery key available before changing TPM, Secure Boot or
          boot settings. BitLocker protection will not be suspended automatically.
        </p>
      </div>

      <footer>
        <button
          bind:this={cancelButton}
          class="btn"
          disabled={tools.firmwareRestartBusy}
          onclick={() => tools.dismissBiosDialog()}
        >Cancel</button>
        <button
          class="btn danger restart"
          disabled={tools.firmwareRestartBusy}
          onclick={() => tools.restartToFirmware()}
        >
          {#if tools.firmwareRestartBusy}
            <LoaderCircle size={15} class="spin" /> Processing…
          {:else}
            <RotateCw size={15} /> Restart to BIOS
          {/if}
        </button>
      </footer>
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 22px; background: rgb(4 6 12 / 0.64); }

  .dialog { position: relative; display: flex; flex-direction: column; width: min(590px, 100%); max-height: calc(100vh - 44px); border-color: rgb(112 147 232 / 0.2); border-radius: var(--radius-xl); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.07), 0 24px 60px -24px rgb(0 0 0 / 0.88); }

  .dialog-body { display: grid; gap: 14px; min-height: 0; padding: 20px; overflow: auto; border-radius: inherit; }

  .top-rim { position: absolute; inset: 0 15% auto; height: 1px; background: linear-gradient(90deg, transparent, rgb(103 164 255 / 0.68), transparent); box-shadow: 0 0 16px rgb(88 145 255 / 0.2); pointer-events: none; }

  header { display: flex; align-items: flex-start; gap: 11px; }
  .chip-icon { display: grid; place-items: center; width: 38px; height: 38px; flex: none; border: 1px solid rgb(107 166 255 / 0.2); border-radius: 11px; background: rgb(93 148 255 / 0.09); color: rgb(151 190 255 / 0.94); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.06); }
  .heading { min-width: 0; flex: 1; }
  .title-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  h2 { font-size: 17px; }
  header p { margin-top: 4px; color: var(--text-2); font-size: 11.5px; line-height: 1.5; }
  .firmware-badge { display: inline-flex; padding: 2px 7px; border: 1px solid rgb(106 165 255 / 0.18); border-radius: 999px; background: rgb(93 148 255 / 0.07); color: rgb(151 190 255 / 0.78); font-size: 9px; font-weight: 700; letter-spacing: 0.055em; }

  .steps { padding: 13px; }
  h3 { color: var(--text-2); font-size: 10px; letter-spacing: 0.06em; text-transform: uppercase; }
  ol { display: grid; gap: 8px; margin: 10px 0 0; padding: 0; list-style: none; }
  li { display: flex; align-items: center; gap: 9px; color: var(--text-2); font-size: 11.5px; }
  li > span { display: grid; place-items: center; width: 22px; height: 22px; flex: none; border: 1px solid rgb(116 159 235 / 0.16); border-radius: 7px; background: rgb(91 136 216 / 0.07); color: rgb(159 190 245 / 0.8); font-family: var(--font-mono); font-size: 9px; }

  .notice, .bitlocker { display: flex; align-items: flex-start; gap: 9px; padding: 10px 11px; border-radius: 10px; font-size: 10.8px; line-height: 1.48; }
  .notice { border: 1px solid rgb(241 177 71 / 0.2); background: rgb(241 177 71 / 0.065); color: rgb(237 210 164 / 0.82); }
  .notice :global(svg) { flex: none; margin-top: 1px; color: rgb(244 184 83 / 0.9); }
  .notice strong { color: rgb(250 221 172 / 0.94); font-size: 11px; }
  .notice p { margin-top: 2px; }
  .bitlocker { border: 1px solid rgb(255 255 255 / 0.055); background: rgb(255 255 255 / 0.02); color: var(--text-3); }
  .bitlocker :global(svg) { flex: none; margin-top: 1px; color: rgb(150 181 237 / 0.72); }

  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .restart { min-width: 154px; }

  :global(:root.solid) .top-rim { box-shadow: none; }

  @media (max-width: 520px) {
    .backdrop { padding: 12px; }
    .dialog { max-height: calc(100vh - 24px); }
    .dialog-body { padding: 16px; }
    footer { align-items: stretch; flex-direction: column-reverse; }
    footer .btn { width: 100%; }
  }

  @media (prefers-reduced-motion: reduce) {
    .dialog-body { scroll-behavior: auto; }
  }
</style>
