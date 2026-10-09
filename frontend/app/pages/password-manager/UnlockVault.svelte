<script lang="ts">
  import { onMount, tick } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Fingerprint from "@lucide/svelte/icons/fingerprint";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LifeBuoy from "@lucide/svelte/icons/life-buoy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import LockKeyholeOpen from "@lucide/svelte/icons/lock-keyhole-open";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import { passwordsApi as api, type Strength } from "./api";
  import { QUICK_UNLOCK } from "../../../lib/platform";
  import { passwords as p } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";

  let recovering = $state(false);
  let master = $state("");
  let code = $state("");
  let newMaster = $state("");
  /** The new master password again: one typo would lock the vault. */
  let newAgain = $state("");
  let showNew = $state(false);
  let showAgain = $state(false);
  let confirmationTouched = $state(false);
  const fieldId = $props.id();
  const mismatch = $derived(newAgain.length > 0 && newAgain !== newMaster);
  let strength = $state<Strength>("none");
  /** What the vault accepts for a master password: 10 characters, not weak. */
  const newReady = $derived(newMaster.length >= 10 && strength !== "weak" && newAgain === newMaster);

  $effect(() => {
    const value = newMaster;
    void api.strength(value).then((s) => {
      if (value === newMaster) strength = s;
    });
  });
  let input = $state<HTMLInputElement>();
  let recoveryInput = $state<HTMLInputElement>();
  let helloButton = $state<HTMLButtonElement>();

  onMount(async () => {
    await tick();
    // Windows Hello first when it is on: one key press opens the vault.
    (p.hello.enabled ? helloButton : input)?.focus();
    // Just came to the page: Windows Hello asks at once.
    if (p.takeHelloOnOpen() && !p.busy) void p.unlockWithHello(true);
  });

  // A website's sign-in waits for the vault: Windows Hello asks at once
  // (once), as soon as this window can show it; else the password field.
  $effect(() => {
    if (!p.helloForSite || p.busy || recovering) return;
    if (!p.hello.enabled) {
      p.helloForSite = false;
      input?.focus();
      return;
    }
    const go = () => {
      if (document.visibilityState !== "visible" || !p.helloForSite) return;
      p.helloForSite = false;
      void p.unlockWithHello(true);
    };
    go();
    document.addEventListener("visibilitychange", go);
    return () => document.removeEventListener("visibilitychange", go);
  });

  async function unlockWithHello() {
    if (p.busy) return;
    await p.unlockWithHello();
  }

  async function setRecovering(value: boolean) {
    if (p.busy) return;
    recovering = value;
    p.error = null;
    showNew = false;
    showAgain = false;
    confirmationTouched = false;
    await tick();
    (value ? recoveryInput : p.hello.enabled ? helloButton : input)?.focus();
  }

  async function unlock(event: SubmitEvent) {
    event.preventDefault();
    if (!master || p.busy) return;
    if (await p.unlock(master)) master = "";
    else input?.select();
  }

  async function recover(event: SubmitEvent) {
    event.preventDefault();
    if (!code || !newReady || p.busy) return;
    if (await p.recover(code, newMaster)) {
      code = "";
      newMaster = "";
      newAgain = "";
    }
  }
</script>

<section class="unlock glass" class:recovering aria-busy={p.busy}>
  {#if !recovering}
    <span class="lock" class:open={p.busy}>
      {#if p.busy}<LockKeyholeOpen size={26} />{:else}<LockKeyhole size={26} />{/if}
    </span>
    <h2>Your vault is locked</h2>
    {#if p.siteWaiting}
      <p class="site-waiting" role="status">A sign-in in your browser is waiting for your passkey. Unlock to carry on: it finishes by itself.</p>
    {/if}
    {#if p.hello.enabled}
      <p>Open it with {QUICK_UNLOCK}, or with your master password.</p>
      <button class="btn primary hello" bind:this={helloButton} disabled={p.busy} onclick={unlockWithHello}>
        {#if p.busy}<LoaderCircle size={16} class="spin" /> Waiting for {QUICK_UNLOCK}…{:else}<Fingerprint size={16} /> Unlock with {QUICK_UNLOCK}{/if}
      </button>
      <div class="or"><span>or</span></div>
    {:else}
      <p>Enter your master password to open it.</p>
    {/if}
    <form onsubmit={unlock}>
      <input
        class="input"
        type="password"
        autocomplete="current-password"
        placeholder="Master password"
        bind:value={master}
        bind:this={input}
      />
      {#if p.error}<p class="error">{p.error}</p>{/if}
      <button class="btn" class:primary={!p.hello.enabled} type="submit" disabled={!master || p.busy}>
        {#if p.busy && !p.hello.enabled}<LoaderCircle size={15} class="spin" /> Unlocking…{:else}Unlock{/if}
      </button>
    </form>
    <button class="link" disabled={p.busy} onclick={() => setRecovering(true)}>
      <LifeBuoy size={13} /> Forgot it? Use your recovery code
    </button>
  {:else}
    <header class="recovery-intro">
      <span class="lock recovery-icon" class:open={p.busy}><LifeBuoy size={25} /></span>
      <div>
        <h2>Let's get you back in</h2>
        <p>Set a new master password. Your saved logins are kept.</p>
      </div>
    </header>
    <form class="recovery-form" onsubmit={recover}>
      <fieldset disabled={p.busy}>
        <legend><span class="step">1</span> Your recovery code</legend>
        <label class="sr-only" for="{fieldId}-code">Recovery code</label>
        <input
          id="{fieldId}-code"
          data-sensitive
          class="input mono"
          placeholder="Paste your recovery code"
          autocomplete="off"
          spellcheck="false"
          aria-describedby="{fieldId}-code-help"
          bind:value={code}
          bind:this={recoveryInput}
        />
        <details class="code-help" id="{fieldId}-code-help">
          <summary>Where do I find my recovery code?</summary>
          <p>You saved it when you created your vault. Check your saved copy or printout.</p>
        </details>
      </fieldset>

      <fieldset class="new-password" disabled={p.busy}>
        <legend><span class="step">2</span> Choose a new password</legend>
        <div class="field">
          <label for="{fieldId}-new">New master password</label>
          <div class="password-input">
            <input id="{fieldId}-new" class="input" type={showNew ? "text" : "password"} autocomplete="new-password" placeholder="Try a few unrelated words" aria-describedby="{fieldId}-password-help" bind:value={newMaster} />
            <button class="reveal" type="button" aria-label={showNew ? "Hide new password" : "Show new password"} aria-pressed={showNew} onclick={() => (showNew = !showNew)}>
              {#if showNew}<EyeOff size={17} />{:else}<Eye size={17} />{/if}
            </button>
          </div>
          <div class="password-feedback" id="{fieldId}-password-help" aria-live="polite">
            <span class="length-hint" class:met={newMaster.length >= 10}>
              {#if newMaster.length >= 10}<Check size={13} />{/if} At least 10 characters
            </span>
            {#if newMaster}<StrengthMeter {strength} />{/if}
          </div>
          {#if newMaster.length >= 10 && strength === "weak"}<p class="error">Try a longer phrase or mix in other characters.</p>{/if}
        </div>
        <div class="field">
          <label for="{fieldId}-again">Confirm new password</label>
          <div class="password-input">
            <input id="{fieldId}-again" class="input" class:bad={mismatch && confirmationTouched} type={showAgain ? "text" : "password"} autocomplete="new-password" placeholder="Enter your new password again" aria-invalid={mismatch && confirmationTouched} aria-describedby={newAgain ? `${fieldId}-match` : undefined} bind:value={newAgain} onblur={() => (confirmationTouched = true)} />
            <button class="reveal" type="button" aria-label={showAgain ? "Hide confirmed password" : "Show confirmed password"} aria-pressed={showAgain} onclick={() => (showAgain = !showAgain)}>
              {#if showAgain}<EyeOff size={17} />{:else}<Eye size={17} />{/if}
            </button>
          </div>
          {#if newAgain}
            <p class="match-feedback" class:met={!mismatch} class:error={mismatch && confirmationTouched} id="{fieldId}-match" aria-live="polite">
              {#if !mismatch}<Check size={13} /> Passwords match{:else if confirmationTouched}These passwords don't match yet.{:else}Enter the same password to confirm.{/if}
            </p>
          {/if}
        </div>
      </fieldset>
      {#if p.error}<p class="error recovery-error" role="alert">{p.error}</p>{/if}
      <button class="btn primary recover-button" type="submit" disabled={!code.trim() || !newReady || p.busy}>
        {#if p.busy}<LoaderCircle size={16} class="spin" /> Opening your vault…{:else}<KeyRound size={16} /> Reset password &amp; open vault{/if}
      </button>
    </form>
    <button class="link back-link" disabled={p.busy} onclick={() => setRecovering(false)}><ArrowLeft size={14} /> Back to unlock</button>
  {/if}
</section>

<style>
  .unlock {
    display: grid;
    justify-items: center;
    gap: 10px;
    width: min(400px, 100%);
    margin: 40px auto 0;
    padding: 30px 28px 22px;
    text-align: center;
  }

  .lock {
    display: grid;
    place-items: center;
    width: 58px;
    height: 58px;
    margin-bottom: 4px;
    border-radius: 18px;
    color: #cfd6ff;
    background:
      radial-gradient(circle at 50% 20%, rgb(var(--accent-rgb) / 0.32), transparent 70%),
      rgb(var(--accent-rgb) / 0.1);
    box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.25), 0 10px 30px -12px var(--accent-glow);
    transition: transform var(--dur-med) var(--ease-out);
  }

  .lock.open {
    transform: scale(1.06);
  }

  h2 {
    font-size: 18px;
  }

  p {
    color: var(--text-2);
    font-size: 12.5px;
  }

  form {
    display: grid;
    gap: 10px;
    width: 100%;
    margin-top: 8px;
  }

  .input {
    height: 38px;
    text-align: center;
  }

  .mono {
    font-family: var(--font-mono);
    letter-spacing: 0.04em;
  }

  .error {
    color: #ff9d9d;
    font-size: 12px;
  }

  .btn.primary {
    height: 38px;
  }

  .hello {
    width: 100%;
    height: 42px;
    margin-top: 8px;
    font-size: 13.5px;
  }

  .or {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    color: var(--text-3);
    font-size: 11.5px;
  }

  .or::before,
  .or::after {
    content: "";
    flex: 1;
    height: 1px;
    background: rgb(255 255 255 / 0.08);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    color: var(--text-3);
    font-size: 12px;
  }

  .link:hover {
    color: var(--text-1);
  }

  .unlock.recovering {
    width: min(470px, 100%);
    margin-top: 12px;
    padding: 20px;
    gap: 12px;
    text-align: left;
  }

  .recovery-intro {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    width: 100%;
  }

  .recovery-icon {
    flex: 0 0 48px;
    width: 48px;
    height: 48px;
    border-radius: 15px;
    margin: 0;
  }

  .recovery-intro h2 {
    margin-bottom: 7px;
    font-size: 21px;
    line-height: 1.2;
    letter-spacing: -0.025em;
  }

  .recovery-intro p {
    line-height: 1.6;
  }

  .recovery-form {
    margin: 0;
    gap: 14px;
  }

  fieldset {
    min-width: 0;
    padding: 0;
    margin: 0;
    border: 0;
  }

  legend {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 0;
    margin-bottom: 10px;
    font-size: 13px;
    font-weight: 600;
  }

  .step {
    display: grid;
    place-items: center;
    width: 23px;
    height: 23px;
    border: 1px solid rgb(var(--accent-rgb) / 0.24);
    border-radius: 7px;
    background: rgb(var(--accent-rgb) / 0.1);
    color: #cfd6ff;
    font-size: 11px;
  }

  .recovery-form .input {
    width: 100%;
    height: 43px;
    text-align: left;
    font-size: 13px;
  }

  .code-help {
    margin-top: 9px;
    color: var(--text-2);
    font-size: 11.5px;
  }

  .code-help summary {
    cursor: pointer;
    width: fit-content;
  }

  .code-help p {
    padding-top: 7px;
    font-size: 11.5px;
    line-height: 1.6;
  }

  .new-password {
    border-top: 1px solid rgb(255 255 255 / 0.07);
    padding-top: 14px;
  }

  .new-password legend {
    float: left;
  }

  .field {
    display: grid;
    gap: 7px;
    clear: both;
  }

  .field + .field {
    margin-top: 14px;
  }

  .field label {
    color: var(--text-1);
    font-size: 12px;
    font-weight: 500;
  }

  .password-input {
    position: relative;
  }

  .password-input .input {
    padding-right: 43px;
  }

  .reveal {
    position: absolute;
    inset: 4px 4px 4px auto;
    display: grid;
    place-items: center;
    width: 35px;
    border-radius: 7px;
    color: var(--text-2);
  }

  .reveal:hover {
    background: var(--hover);
    color: var(--text-1);
  }

  .password-feedback {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 12px;
  }

  .length-hint,
  .match-feedback {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--text-2);
    font-size: 11.5px;
  }

  .met {
    color: #5fd99a;
  }

  .input.bad {
    border-color: rgb(255 120 120 / 0.55);
  }

  .match-feedback.error {
    color: #ff9d9d;
  }

  .recovery-error {
    border: 1px solid rgb(255 120 120 / 0.2);
    border-radius: 9px;
    padding: 10px 12px;
    background: rgb(255 120 120 / 0.06);
    line-height: 1.5;
  }

  .btn.recover-button {
    min-height: 43px;
    height: auto;
    padding: 10px 12px;
    white-space: normal;
    font-size: 13px;
    font-weight: 600;
  }

  .recover-button:disabled {
    opacity: 1;
    color: var(--text-2);
    border-color: rgb(var(--accent-rgb) / 0.18);
    background: rgb(var(--accent-rgb) / 0.1);
    box-shadow: none;
  }

  .back-link {
    justify-content: center;
    width: 100%;
    padding: 6px;
    margin: 0;
    color: var(--text-2);
  }

  .link:focus-visible,
  .reveal:focus-visible,
  summary:focus-visible,
  .recover-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }

  .link:disabled,
  .reveal:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  @media (max-width: 560px) {
    .unlock.recovering {
      padding: 22px 18px 16px;
      margin-top: 12px;
    }

    .recovery-intro {
      gap: 11px;
    }

    .recovery-intro h2 {
      font-size: 19px;
    }
  }

  .site-waiting {
    margin: 0 0 4px;
    padding: 8px 12px;
    border: 1px solid rgb(var(--accent-rgb) / 0.4);
    border-radius: 10px;
    background: rgb(var(--accent-rgb) / 0.12);
    color: var(--text-1);
    font-size: 12.5px;
  }
</style>
