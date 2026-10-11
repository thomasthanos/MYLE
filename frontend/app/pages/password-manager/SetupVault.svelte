<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LifeBuoy from "@lucide/svelte/icons/life-buoy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import { passwordsApi as api, type Strength } from "./api";
  import { DEVICE, MOBILE, QUICK_UNLOCK } from "../../../lib/platform";
  import { passwords as p } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";

  let master = $state("");
  let confirmation = $state("");
  let strength = $state<Strength>("none");
  let code = $state<string | null>(null);
  let saved = $state(false);
  /** Also open the vault with Windows Hello (or the phone's biometrics) here. */
  let useHello = $state(true);
  let showMaster = $state(false);
  let showConfirmation = $state(false);

  const mismatch = $derived(confirmation.length > 0 && confirmation !== master);
  const ready = $derived(master.length >= 10 && strength !== "weak" && confirmation === master);

  $effect(() => {
    const value = master;
    void api.strength(value).then((s) => {
      if (value === master) strength = s;
    });
  });

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (!ready || p.busy) return;
    code = await p.create(master);
    if (code) {
      master = "";
      confirmation = "";
    }
  }
</script>

<section class="setup glass" class:mobile={MOBILE}>
  {#if code === null}
    <div class="intro">
      <span class="badge"><ShieldCheck size={22} /></span>
      <h2>{MOBILE ? "Create your vault." : "Create your password vault"}</h2>
      <p>
        {#if MOBILE}
          Choose one strong master password. It protects every password you save in MYLE.
        {:else}
        Everything is encrypted on this {DEVICE} with your master password before it is saved or synced. Nobody else can
        read it: not the sync server, and not us.
        {/if}
      </p>
    </div>

    <form onsubmit={create}>
      <label class="field">
        <span>Master password</span>
        <span class="password-input">
          <input class="input" type={showMaster ? "text" : "password"} autocomplete="new-password" bind:value={master} placeholder="At least 10 characters" aria-label="Master password" aria-describedby="master-hint" />
          {#if MOBILE}<button type="button" class="reveal" aria-label={showMaster ? "Hide master password" : "Show master password"} aria-pressed={showMaster} onclick={() => (showMaster = !showMaster)}>{#if showMaster}<EyeOff size={19} />{:else}<Eye size={19} />{/if}</button>{/if}
        </span>
      </label>
      <div class="meter-row">
        <StrengthMeter {strength} />
        <small id="master-hint">{MOBILE ? "Try a few unrelated words you can remember." : "A few unrelated words make a strong, memorable password."}</small>
      </div>
      <label class="field">
        <span>{MOBILE ? "Confirm master password" : "Type it again"}</span>
        <span class="password-input">
          <input class="input" class:bad={mismatch} type={showConfirmation ? "text" : "password"} autocomplete="new-password" bind:value={confirmation} placeholder={MOBILE ? "Enter it once more" : undefined} aria-label={MOBILE ? "Confirm master password" : "Type it again"} aria-invalid={mismatch} />
          {#if MOBILE}<button type="button" class="reveal" aria-label={showConfirmation ? "Hide confirmation password" : "Show confirmation password"} aria-pressed={showConfirmation} onclick={() => (showConfirmation = !showConfirmation)}>{#if showConfirmation}<EyeOff size={19} />{:else}<Eye size={19} />{/if}</button>{/if}
        </span>
      </label>
      {#if mismatch}<p class="error">The two passwords are not the same.</p>{/if}
      {#if p.error}<p class="error">{p.error}</p>{/if}
      {#if p.sync.kind === "signedOut" && !MOBILE}
          <p class="note">Sign in on the Settings page to keep this vault on all your PCs. You can do it later too.</p>
      {/if}
      <button class="btn primary" type="submit" disabled={!ready || p.busy}>
        {#if p.busy}<LoaderCircle size={15} class="spin" /> Creating…{:else}<KeyRound size={15} /> Create vault{/if}
      </button>
      {#if MOBILE}<p class="security-note"><LockKeyhole size={15} /><span>Encrypted on your phone before it syncs.<br />Only you can unlock your vault.</span></p>{/if}
    </form>
  {:else}
    <div class="intro">
      <span class="badge rescue"><LifeBuoy size={22} /></span>
      <h2>Save your recovery code</h2>
      <p>
        If you ever forget your master password, this code is the <strong>only</strong> way back in. Print it or keep
        it somewhere safe, away from this {DEVICE}. It is shown only now.
      </p>
    </div>
    <div class="code selectable" data-sensitive>{code}</div>
    <div class="code-actions">
      <button class="btn" onclick={() => code && p.copyText(code)}><Copy size={14} /> Copy</button>
    </div>
    <label class="saved">
      <input type="checkbox" class="check" bind:checked={saved} />
      I have saved my recovery code somewhere safe.
    </label>
    {#if p.hello.available}
      <label class="saved">
        <input type="checkbox" class="check" bind:checked={useHello} />
        {#if MOBILE}Also open it with {QUICK_UNLOCK} on this phone.{:else}Also open it with Windows Hello (PIN, fingerprint or face) on this PC.{/if}
      </label>
    {/if}
    <button class="btn primary" disabled={!saved} onclick={() => p.finishSetup(p.hello.available && useHello)}>
      <Check size={15} /> Open my vault
    </button>
  {/if}
</section>

<style>
  .setup { display: grid; gap: 18px; width: min(460px, 100%); margin: 12px auto 0; padding: 28px; }

  .intro { display: grid; justify-items: center; gap: 10px; text-align: center; }

  .badge { display: grid; place-items: center; width: 48px; height: 48px; border-radius: 15px; color: #cfd6ff; background: rgb(var(--accent-rgb) / 0.16); box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.25), 0 8px 24px -10px var(--accent-glow); }

  .badge.rescue { color: #ffd79a; background: rgb(255 180 84 / 0.14); box-shadow: inset 0 0 0 1px rgb(255 180 84 / 0.25); }

  h2 { font-size: 19px; }

  .intro p { color: var(--text-2); font-size: 12.5px; line-height: 1.55; }

  form { display: grid; gap: 12px; }

  .field { display: grid; gap: 6px; }

  .field span { color: var(--text-2); font-size: 12px; font-weight: 600; }
  .password-input { position: relative; display: block; }
  .password-input .input { width: 100%; }
  .reveal { position: absolute; right: 5px; top: 50%; transform: translateY(-50%); display: grid; place-items: center; width: 44px; height: 44px; border-radius: 10px; color: var(--text-3); }
  .reveal:hover { color: var(--text-1); background: var(--hover); }

  .input.bad { border-color: rgb(255 120 120 / 0.55); }

  .meter-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }

  .meter-row small { color: var(--text-3); font-size: 11px; text-align: right; }

  .error { color: #ff9d9d; font-size: 12px; }

  .note { color: var(--text-3); font-size: 11.5px; }

  .btn.primary { height: 38px; margin-top: 4px; }

  .code { padding: 16px; border: 1px dashed rgb(255 180 84 / 0.4); border-radius: var(--radius-md); background: rgb(0 0 0 / 0.22); color: #ffe2b0; font-family: var(--font-mono); font-size: 14.5px; letter-spacing: 0.04em; line-height: 1.7; text-align: center; word-spacing: 0.2em; }

  .code-actions { display: flex; justify-content: center; margin-top: -6px; }

  .saved { display: flex; align-items: center; gap: 10px; color: var(--text-2); font-size: 12.5px; }

  .security-note { display: flex; align-items: flex-start; justify-content: center; gap: 8px; color: var(--text-3); font-size: 12px; line-height: 1.5; }
  .security-note :global(svg) { flex: none; margin-top: 2px; }
  .mobile { width: min(520px, 100%); margin: auto; padding: 28px 24px; gap: 28px; border-radius: 26px; background: linear-gradient(155deg, rgb(255 255 255 / .055), transparent 50%), var(--bg-solid-panel); box-shadow: inset 0 1px 0 rgb(255 255 255 / .07), 0 22px 44px -26px #000; }
  .mobile .intro { justify-items: start; text-align: left; gap: 16px; }
  .mobile .badge { width: 56px; height: 56px; border-radius: 18px; }
  .mobile h2 { max-width: 340px; font-size: clamp(26px, 7vw, 32px); line-height: 1.18; letter-spacing: -.4px; }
  .mobile .intro p { font-size: 14px; line-height: 1.6; }
  .mobile form { gap: 16px; }
  .mobile .field { gap: 9px; }
  .mobile .field > span:first-child { font-family: var(--font-sans); font-size: 13px; }
  .mobile .input { min-height: 56px; padding-right: 52px; border-radius: 14px; font-size: 16px; font-family: var(--font-sans); background: rgb(0 0 0 / .2); }
  .mobile .meter-row { display: grid; gap: 8px; margin-top: -4px; }
  .mobile .meter-row small { text-align: left; font-size: 12px; line-height: 1.5; }
  .mobile .btn.primary { min-height: 52px; margin-top: 6px; border-radius: 14px; font-family: var(--font-sans); font-size: 15px; }
  .mobile .error { font-size: 13px; }
  .mobile .saved { font-size: 14px; line-height: 1.5; }
  @media (max-width: 360px) { .mobile { padding: 24px 18px; gap: 22px; } }
</style>
