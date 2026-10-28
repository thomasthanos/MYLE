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

  // A website's sign-in waits for the vault: Windows Hello asks at once (once), as soon as this window can show it; else the password field.
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

<div class="unlock-stage">
  <div class="ambient-glow" aria-hidden="true"></div>
  <section class="unlock glass" class:recovering aria-busy={p.busy}>
    {#if !recovering}
      <span class="lock" class:open={p.busy}>
        {#if p.busy}<LockKeyholeOpen size={28} strokeWidth={1.8} />{:else}<LockKeyhole size={28} strokeWidth={1.8} />{/if}
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
</div>

<style>
  .unlock-stage {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: 100%;
    min-height: clamp(480px, calc(100vh - 200px), 940px);
    padding: clamp(16px, 3vh, 48px) clamp(16px, 2.5vw, 36px) clamp(24px, 5vh, 64px);
  }

  .ambient-glow {
    position: absolute;
    width: min(580px, 88vw);
    height: min(440px, 65vh);
    border-radius: 50%;
    background: radial-gradient(
      circle at 50% 45%,
      rgb(var(--accent-rgb) / 0.16) 0%,
      rgb(111 179 198 / 0.05) 45%,
      transparent 72%
    );
    filter: blur(52px);
    pointer-events: none;
    z-index: 0;
  }

  .unlock {
    position: relative;
    z-index: 1;
    display: grid;
    justify-items: center;
    gap: clamp(10px, 1.4vh, 16px);
    width: min(clamp(400px, 28vw, 470px), 100%);
    margin: 0 auto;
    padding: clamp(30px, 3.5vh, 44px) clamp(24px, 2.5vw, 38px) clamp(24px, 2.8vh, 32px);
    text-align: center;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: clamp(18px, 1.6vw, 24px);
    background:
      radial-gradient(circle at 50% 0%, rgb(var(--accent-rgb) / 0.12), transparent 60%),
      linear-gradient(180deg, var(--solid-fill-top), var(--solid-fill-bottom));
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.12),
      inset 0 -1px 0 rgb(0 0 0 / 0.35),
      0 24px 60px -20px rgb(0 0 0 / 0.75),
      0 10px 30px -10px rgb(var(--accent-rgb) / 0.3);
    transition: width var(--dur-med) var(--ease-out), padding var(--dur-med) var(--ease-out);
  }

  .lock {
    display: grid;
    place-items: center;
    width: clamp(62px, 4.4vw, 74px);
    height: clamp(62px, 4.4vw, 74px);
    margin-bottom: 2px;
    border: 1px solid rgb(var(--accent-rgb) / 0.35);
    border-radius: clamp(18px, 1.3vw, 22px);
    color: #d6dcff;
    background:
      radial-gradient(circle at 35% 25%, rgb(255 255 255 / 0.18), transparent 55%),
      linear-gradient(150deg, rgb(var(--accent-rgb) / 0.28) 0%, rgb(18 22 36 / 0.9) 100%);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.2),
      inset 0 -1px 0 rgb(0 0 0 / 0.4),
      0 14px 32px -10px rgb(var(--accent-rgb) / 0.5),
      0 4px 10px rgb(0 0 0 / 0.3);
    transition: transform var(--dur-med) var(--ease-out), box-shadow var(--dur-med) var(--ease-out);
  }

  .lock.open {
    transform: scale(1.08);
    box-shadow: 0 18px 40px -8px rgb(var(--accent-rgb) / 0.65);
  }

  h2 {
    margin: 4px 0 0;
    font-family: var(--font-brand);
    font-size: clamp(20px, 1.4vw, 24px);
    font-weight: 700;
    letter-spacing: -0.025em;
    color: var(--text-1);
  }

  p {
    max-width: 35ch;
    margin: 0;
    color: var(--text-2);
    font-size: clamp(12.5px, 0.9vw, 13.5px);
    line-height: 1.5;
  }

  form {
    display: grid;
    gap: 11px;
    width: 100%;
    margin-top: 6px;
  }

  .input {
    height: clamp(40px, 2.9vw, 46px);
    font-size: clamp(13px, 0.9vw, 14.5px);
    border-radius: 11px;
    text-align: center;
    border: 1px solid rgb(255 255 255 / 0.1);
    background: rgb(0 0 0 / 0.24);
    color: var(--text-1);
    transition: border-color var(--dur-fast), box-shadow var(--dur-fast);
  }

  .input:focus {
    border-color: rgb(var(--accent-rgb) / 0.6);
    box-shadow: 0 0 0 3px rgb(var(--accent-rgb) / 0.22);
  }

  .mono { font-family: var(--font-mono); letter-spacing: 0.04em; }

  .error { color: #ff9d9d; font-size: 12px; }

  .btn {
    height: clamp(40px, 2.9vw, 46px);
    font-size: clamp(13px, 0.9vw, 14.5px);
    font-weight: 600;
    border-radius: 11px;
  }

  .btn.primary {
    box-shadow: 0 6px 20px -5px rgb(var(--accent-rgb) / 0.45);
  }

  .hello {
    width: 100%;
    height: clamp(44px, 3.1vw, 50px);
    margin-top: 4px;
    font-size: clamp(13.5px, 0.95vw, 15px);
    box-shadow: 0 8px 24px -6px rgb(var(--accent-rgb) / 0.5);
  }

  .or {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    color: var(--text-3);
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .or::before,
  .or::after {
    content: "";
    flex: 1;
    height: 1px;
    background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.12), transparent);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin-top: 4px;
    padding: 6px 12px;
    border-radius: 8px;
    color: var(--text-3);
    font-size: clamp(12px, 0.85vw, 13px);
    transition: color var(--dur-fast), background var(--dur-fast);
  }

  .link:hover {
    color: var(--text-1);
    background: rgb(255 255 255 / 0.04);
  }

  /* Recovery view styling */
  .unlock.recovering {
    width: min(clamp(480px, 34vw, 560px), 100%);
    padding: clamp(24px, 3vh, 32px) clamp(22px, 2.4vw, 34px);
    gap: 14px;
    text-align: left;
  }

  .recovery-intro { display: flex; align-items: flex-start; gap: 14px; width: 100%; }

  .recovery-icon { flex: 0 0 48px; width: 48px; height: 48px; border-radius: 15px; margin: 0; }

  .recovery-intro h2 { margin-bottom: 7px; font-size: 21px; line-height: 1.2; letter-spacing: -0.025em; }

  .recovery-intro p { line-height: 1.6; }

  .recovery-form { margin: 0; gap: 14px; }

  fieldset { min-width: 0; padding: 0; margin: 0; border: 0; }

  legend { display: flex; align-items: center; gap: 9px; width: 100%; padding: 0; margin-bottom: 10px; font-size: 13px; font-weight: 600; }

  .step { display: grid; place-items: center; width: 23px; height: 23px; border: 1px solid rgb(var(--accent-rgb) / 0.24); border-radius: 7px; background: rgb(var(--accent-rgb) / 0.1); color: #cfd6ff; font-size: 11px; }

  .recovery-form .input { width: 100%; height: 43px; text-align: left; font-size: 13px; }

  .code-help { margin-top: 9px; color: var(--text-2); font-size: 11.5px; }

  .code-help summary { cursor: pointer; width: fit-content; }

  .code-help p { padding-top: 7px; font-size: 11.5px; line-height: 1.6; }

  .new-password { border-top: 1px solid rgb(255 255 255 / 0.07); padding-top: 14px; }

  .new-password legend { float: left; }

  .field { display: grid; gap: 7px; clear: both; }

  .field + .field { margin-top: 14px; }

  .field label { color: var(--text-1); font-size: 12px; font-weight: 500; }

  .password-input { position: relative; }

  .password-input .input { padding-right: 43px; }

  .reveal { position: absolute; inset: 4px 4px 4px auto; display: grid; place-items: center; width: 35px; border-radius: 7px; color: var(--text-2); }

  .reveal:hover { background: var(--hover); color: var(--text-1); }

  .password-feedback { display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 6px 12px; }

  .length-hint,
  .match-feedback { display: inline-flex; align-items: center; gap: 5px; color: var(--text-2); font-size: 11.5px; }

  .met { color: #5fd99a; }

  .input.bad { border-color: rgb(255 120 120 / 0.55); }

  .match-feedback.error { color: #ff9d9d; }

  .recovery-error { border: 1px solid rgb(255 120 120 / 0.2); border-radius: 9px; padding: 10px 12px; background: rgb(255 120 120 / 0.06); line-height: 1.5; }

  .btn.recover-button { min-height: 43px; height: auto; padding: 10px 12px; white-space: normal; font-size: 13px; font-weight: 600; }

  .recover-button:disabled { opacity: 1; color: var(--text-2); border-color: rgb(var(--accent-rgb) / 0.18); background: rgb(var(--accent-rgb) / 0.1); box-shadow: none; }

  .back-link { justify-content: center; width: 100%; padding: 6px; margin: 0; color: var(--text-2); }

  .link:focus-visible,
  .reveal:focus-visible,
  summary:focus-visible,
  .recover-button:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }

  .link:disabled,
  .reveal:disabled { opacity: 0.5; cursor: default; }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }

  .site-waiting { margin: 0 0 4px; padding: 8px 12px; border: 1px solid rgb(var(--accent-rgb) / 0.4); border-radius: 10px; background: rgb(var(--accent-rgb) / 0.12); color: var(--text-1); font-size: 12.5px; }

  /* Tier 1: 4K / Ultrawide / Large screens (> 1700px or > 1050px height) */
  @media (min-width: 1700px), (min-height: 1050px) {
    .unlock {
      width: min(480px, 100%);
      padding: 44px 38px 32px;
      gap: 16px;
    }

    .unlock.recovering {
      width: min(560px, 100%);
    }

    .ambient-glow {
      width: 640px;
      height: 500px;
    }
  }

  /* Tier 3: Compact laptops / tablets / low height (< 820px height or < 600px width) */
  @media (max-height: 820px), (max-width: 600px) {
    .unlock-stage {
      min-height: auto;
      padding: 10px 14px 24px;
    }

    .unlock {
      width: min(390px, 100%);
      padding: 24px 20px 18px;
      gap: 10px;
      border-radius: 18px;
    }

    .lock {
      width: 54px;
      height: 54px;
    }

    .hello {
      height: 40px;
      font-size: 13px;
    }

    .input,
    .btn {
      height: 38px;
      font-size: 13px;
    }

    .unlock.recovering {
      padding: 20px 16px 16px;
      margin-top: 10px;
    }

    .recovery-intro {
      gap: 11px;
    }

    .recovery-intro h2 {
      font-size: 19px;
    }
  }
</style>
