<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import Fingerprint from "@lucide/svelte/icons/fingerprint";

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

  async function refresh() {
    if (!isTauri()) return;
    try { status = await invoke<Status>("mobile_provider_status"); error = ""; }
    catch { error = "Android filling settings could not be checked. Reopen Account to retry."; }
  }
  async function settings(target: "passkeys" | "autofill") {
    await action(() => invoke("mobile_provider_settings", { target }));
  }
  async function fingerprint() {
    if (!status) return;
    await action(() => invoke("mobile_provider_biometry", { enabled: !status!.biometryEnabled }));
  }
  async function action(run: () => Promise<unknown>) {
    if (busy) return;
    busy = true; error = "";
    try { await run(); await refresh(); }
    catch (cause) { error = String(cause); }
    finally { busy = false; }
  }
  onMount(() => {
    void refresh();
    const resumed = () => { if (document.visibilityState === "visible") void refresh(); };
    document.addEventListener("visibilitychange", resumed);
    return () => document.removeEventListener("visibilitychange", resumed);
  });
</script>

{#if status && status.sdk > 0}
  <section aria-label="Android password filling">
    <h3>Use MYLE in other apps</h3>
    <div class="setting">
      <span><b>Passwords and 2FA codes</b><small>{status.autofillEnabled ? "MYLE is your autofill service" : "Choose MYLE as your autofill service"}</small></span>
      <button class="btn small" disabled={busy || !status.autofillAvailable} onclick={() => settings("autofill")}>{status.autofillEnabled ? "Settings" : "Set up"}</button>
    </div>
    <div class="setting">
      <span><b>Passkeys</b><small>{!status.passkeysAvailable ? "Passkey providers need Android 14 or later" : status.passkeysEnabled ? "MYLE is enabled as a credential provider" : "Enable MYLE in Android's credential providers"}</small></span>
      {#if status.passkeysAvailable}
        <button class="btn small" disabled={busy} onclick={() => settings("passkeys")}>{status.passkeysEnabled ? "Settings" : "Set up"}</button>
      {:else}<span class="requirement">Android 14+</span>{/if}
    </div>
    <div class="setting">
      <span><b><Fingerprint size={14} /> Fingerprint for filling</b><small>{status.biometryEnabled ? "Verify each fill or passkey operation" : status.biometryAvailable ? "Protect filling with your fingerprint" : "Set up a strong fingerprint in Android settings"}</small></span>
      <button class="btn small" disabled={busy || (!status.biometryAvailable && !status.biometryEnabled)} onclick={fingerprint}>{status.biometryEnabled ? "Turn off" : "Enable"}</button>
    </div>
    <p>Your master password is always available. 2FA codes come from your saved TOTP entries.</p>
    <p>Chrome may also need “Autofill using another service” enabled in its settings.</p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

{#if error && !status}<p class="error" role="alert">{error}</p>{/if}

<style>
  section { display: grid; gap: 12px; padding-top: 14px; border-top: 1px solid rgb(255 255 255 / .1); }
  h3 { margin: 0; font-size: 15px; }
  .setting { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 12px; padding-block: 5px; }
  .setting > span { display: grid; min-width: 0; gap: 4px; overflow-wrap: anywhere; }
  b { display: flex; align-items: center; gap: 5px; font-size: 13px; font-weight: 600; }
  small, p { color: var(--text-2); font-size: 12px; line-height: 1.4; }
  p { margin: 0; }
  button { min-height: 44px; }
  .setting > .requirement { display: block; color: var(--text-3); font-size: 11px; white-space: nowrap; padding: 6px 8px; border: 1px solid var(--glass-border); border-radius: 8px; }
  .error { color: var(--danger, #f87171); }
</style>
