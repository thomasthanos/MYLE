<script lang="ts">
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import VaultDialog, { type DialogKind } from "../../app/pages/password-manager/VaultDialog.svelte";
  import { passwords as p } from "../../app/pages/password-manager/state.svelte";
  import { QUICK_UNLOCK } from "../../lib/platform";
  import SettingRow from "./SettingRow.svelte";
  import SettingsCard from "./SettingsCard.svelte";

  const lockTimes = [1, 5, 15, 60, 0];
  const uid = $props.id();
  let dialog = $state<DialogKind | null>(null);
  /** Where the keyboard was, to go back there when the dialog closes. */
  let opener: HTMLElement | null = null;
  /** Turning quick unlock on and changing the master password need the vault open. */
  const unlocked = $derived(p.status === "unlocked");

  function openDialog(kind: DialogKind) {
    opener = document.activeElement as HTMLElement | null;
    dialog = kind;
  }
  function closeDialog() {
    dialog = null;
    opener?.focus({ preventScroll: true });
    opener = null;
  }
</script>

<SettingsCard
  icon={ShieldCheck}
  title="Security & vault"
  intro="Your passwords are encrypted on this phone with your master password. If you forget it, only your recovery code opens the vault."
>
  <div class="lock" role="group" aria-labelledby="{uid}-lock">
    <span class="label" id="{uid}-lock">Lock after</span>
    <small>MYLE locks itself when you have not used it for this long.</small>
    <div class="times">
      {#each lockTimes as minutes (minutes)}
        <button class="chip" class:active={p.autoLockMinutes === minutes} aria-pressed={p.autoLockMinutes === minutes} onclick={() => p.setAutoLock(minutes)}>
          {minutes === 0 ? "Never" : minutes === 60 ? "1 hour" : `${minutes} min`}
        </button>
      {/each}
    </div>
  </div>

  {#if p.hello.available}
    <SettingRow
      label="Open with {QUICK_UNLOCK}"
      hint={unlocked ? "Open MYLE without typing your master password." : "Unlock your vault to change this."}
      checked={p.hello.enabled}
      disabled={!unlocked}
      onchange={(on) => p.setHello(on)}
    />
  {/if}

  <SettingRow
    label="Change master password"
    hint={unlocked ? "Your saved logins stay as they are." : "Unlock your vault first."}
    disabled={!unlocked}
    onclick={() => openDialog({ kind: "master" })}
  />
</SettingsCard>

{#if dialog}
  <VaultDialog {dialog} onclose={closeDialog} />
{/if}

<style>
  .lock { display: grid; gap: 4px; }

  .label { font-size: 14px; font-weight: 600; }

  small { color: var(--text-2); font-size: 12.5px; line-height: 1.4; }

  /* One row of five, 48 px to hit (Android's touch target); a narrow phone
     wraps the words inside a button, never the row. */
  .times { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 6px; margin-top: 8px; }

  .times .chip { justify-content: center; height: 48px; padding: 0 4px; border-radius: 12px; font-size: 13px; line-height: 1.15; text-align: center; white-space: normal; }

  .times .chip.active { border-color: rgb(var(--accent-rgb) / 0.75); background: rgb(var(--accent-rgb) / 0.3); color: #fff; font-weight: 650; }
</style>
