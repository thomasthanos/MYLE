<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import SettingRow from "./SettingRow.svelte";
  import SettingsCard from "./SettingsCard.svelte";

  interface Status {
    sdk: number;
    passkeysAvailable: boolean;
    autofillAvailable: boolean;
    passkeysEnabled: boolean;
    autofillEnabled: boolean;
    biometryAvailable: boolean;
    biometryEnabled: boolean;
  }
  let status = $state<Status | null>(null);
  let busy = $state(false);
  let error = $state("");

  /** In a plain browser (`npx vite --mode mobile`): `?android=30` looks like
   *  Android 11, `?android=34` like Android 14. Without it, no card (as on iOS). */
  function previewStatus(): Status | null {
    const sdk = Number(new URLSearchParams(location.search).get("android"));
    if (!sdk) return null;
    return { sdk, passkeysAvailable: sdk >= 34, autofillAvailable: sdk >= 26, passkeysEnabled: false, autofillEnabled: sdk >= 26, biometryAvailable: true, biometryEnabled: false };
  }

  async function refresh() {
    if (!isTauri()) {
      status = previewStatus();
      return;
    }
    try { status = await invoke<Status>("mobile_provider_status"); error = ""; }
    catch { error = "Android's filling settings could not be checked. Close Settings and open it again to retry."; }
  }
  async function settings(target: "passkeys" | "autofill") {
    await action(() => invoke("mobile_provider_settings", { target }));
  }
  async function fingerprint(on: boolean) {
    await action(() => invoke("mobile_provider_biometry", { enabled: on }));
  }
  async function action(run: () => Promise<unknown>) {
    if (busy || !isTauri()) return;
    busy = true; error = "";
    try { await run(); await refresh(); }
    catch (cause) { error = String(cause); }
    finally { busy = false; }
  }
  onMount(() => {
    void refresh();
    // Back from Android's settings: show what changed there.
    const resumed = () => { if (document.visibilityState === "visible") void refresh(); };
    document.addEventListener("visibilitychange", resumed);
    return () => document.removeEventListener("visibilitychange", resumed);
  });
</script>

{#if status && status.sdk > 0}
  <SettingsCard
    icon={KeyRound}
    title="Autofill & passkeys"
    intro="Fill your logins and 2FA codes in other apps and websites. Every fill asks for your fingerprint or master password first."
  >
    <SettingRow
      label="Passwords & 2FA codes"
      hint={!status.autofillAvailable ? "Needs Android 8 or later." : status.autofillEnabled ? "MYLE fills logins and the 2FA codes you saved." : "Choose MYLE as Android's autofill service."}
      badge={status.autofillAvailable ? { text: status.autofillEnabled ? "On" : "Off", on: status.autofillEnabled } : { text: "Android 8+" }}
      disabled={busy || !status.autofillAvailable}
      onclick={() => settings("autofill")}
    />
    <SettingRow
      label="Passkeys"
      hint={!status.passkeysAvailable
        ? "Passkeys from MYLE need Android 14 or later. On this phone, MYLE fills passwords and 2FA codes."
        : status.passkeysEnabled ? "MYLE saves new passkeys and signs you in with them." : "Turn on MYLE in Android's passkey providers."}
      badge={status.passkeysAvailable ? { text: status.passkeysEnabled ? "On" : "Off", on: status.passkeysEnabled } : { text: "Android 14+" }}
      disabled={busy || !status.passkeysAvailable}
      onclick={() => settings("passkeys")}
    />
    <SettingRow
      label="Fingerprint for filling"
      hint={status.biometryEnabled
        ? "Your fingerprint confirms each fill and passkey."
        : status.biometryAvailable ? "Confirm fills with your fingerprint instead of typing your master password." : "Set up a fingerprint in Android's settings first."}
      checked={status.biometryEnabled}
      disabled={busy || (!status.biometryAvailable && !status.biometryEnabled)}
      onchange={(on) => fingerprint(on)}
    />
    <p class="tip">Using Chrome? Also turn on “Autofill using another service” in Chrome's settings, under Autofill services.</p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </SettingsCard>
{/if}

{#if error && !status}<p class="error" role="alert">{error}</p>{/if}

<style>
  p { margin: 0; font-size: 12.5px; line-height: 1.45; }

  .tip { color: var(--text-3); }

  .error { color: #ff9d9d; }
</style>
