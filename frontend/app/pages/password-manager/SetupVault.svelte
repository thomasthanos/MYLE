<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LifeBuoy from "@lucide/svelte/icons/life-buoy";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { passwordsApi as api, type Strength } from "./api";
  import { DEVICE, MOBILE, QUICK_UNLOCK } from "../../../lib/platform";
  import { shell } from "../../../mobile/shell.svelte";
  import { passwords as p } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";

  let master = $state("");
  let confirmation = $state("");
  let strength = $state<Strength>("none");
  let code = $state<string | null>(null);
  let saved = $state(false);
  /** Also open the vault with Windows Hello (or the phone's biometrics) here. */
  let useHello = $state(true);

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

<section class="setup glass">
  {#if code === null}
    <div class="intro">
      <span class="badge"><ShieldCheck size={22} /></span>
      <h2>Create your password vault</h2>
      <p>
        Everything is encrypted on this {DEVICE} with your master password before it is saved or synced. Nobody else can
        read it: not the sync server, and not us.
      </p>
    </div>

    <form onsubmit={create}>
      <label class="field">
        <span>Master password</span>
        <input class="input" type="password" autocomplete="new-password" bind:value={master} placeholder="At least 10 characters" />
      </label>
      <div class="meter-row">
        <StrengthMeter {strength} />
        <small>A few unrelated words make a strong, memorable password.</small>
      </div>
      <label class="field">
        <span>Type it again</span>
        <input class="input" class:bad={mismatch} type="password" autocomplete="new-password" bind:value={confirmation} />
      </label>
      {#if mismatch}<p class="error">The two passwords are not the same.</p>{/if}
      {#if p.error}<p class="error">{p.error}</p>{/if}
      {#if p.sync.kind === "signedOut"}
        {#if MOBILE}
          <p class="note">
            <button type="button" class="link" onclick={() => (shell.accountOpen = true)}>Sign in</button> to keep this vault
            the same here and on your PC. You can do it later too.
          </p>
        {:else}
          <p class="note">Sign in on the Settings page to keep this vault on all your PCs. You can do it later too.</p>
        {/if}
      {/if}
      <button class="btn primary" type="submit" disabled={!ready || p.busy}>
        {#if p.busy}<LoaderCircle size={15} class="spin" /> Creating…{:else}<KeyRound size={15} /> Create vault{/if}
      </button>
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
  .setup {
    display: grid;
    gap: 18px;
    width: min(460px, 100%);
    margin: 12px auto 0;
    padding: 28px;
  }

  .intro {
    display: grid;
    justify-items: center;
    gap: 10px;
    text-align: center;
  }

  .badge {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 15px;
    color: #cfd6ff;
    background: rgb(var(--accent-rgb) / 0.16);
    box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.25), 0 8px 24px -10px var(--accent-glow);
  }

  .badge.rescue {
    color: #ffd79a;
    background: rgb(255 180 84 / 0.14);
    box-shadow: inset 0 0 0 1px rgb(255 180 84 / 0.25);
  }

  h2 {
    font-size: 19px;
  }

  .intro p {
    color: var(--text-2);
    font-size: 12.5px;
    line-height: 1.55;
  }

  form {
    display: grid;
    gap: 12px;
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field span {
    color: var(--text-2);
    font-size: 12px;
    font-weight: 600;
  }

  .input.bad {
    border-color: rgb(255 120 120 / 0.55);
  }

  .meter-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .meter-row small {
    color: var(--text-3);
    font-size: 11px;
    text-align: right;
  }

  .error {
    color: #ff9d9d;
    font-size: 12px;
  }

  .note {
    color: var(--text-3);
    font-size: 11.5px;
  }

  .btn.primary {
    height: 38px;
    margin-top: 4px;
  }

  .code {
    padding: 16px;
    border: 1px dashed rgb(255 180 84 / 0.4);
    border-radius: var(--radius-md);
    background: rgb(0 0 0 / 0.22);
    color: #ffe2b0;
    font-family: var(--font-mono);
    font-size: 14.5px;
    letter-spacing: 0.04em;
    line-height: 1.7;
    text-align: center;
    word-spacing: 0.2em;
  }

  .code-actions {
    display: flex;
    justify-content: center;
    margin-top: -6px;
  }

  .saved {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
    font-size: 12.5px;
  }

  .note .link {
    color: #b7bef5;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
</style>
