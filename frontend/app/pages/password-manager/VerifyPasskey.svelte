<script lang="ts">
  // A site asks to confirm it is the user before a passkey is used, and this
  // PC has no Windows Hello: the master password stands in. The app waits for
  // the answer (two minutes at most) and brought this page forward for it.
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import UserKey from "@lucide/svelte/icons/user-key";
  import { portal } from "../../../lib/portal";
  import { passwordsApi as api, type VerifyRequest } from "./api";

  let request = $state<VerifyRequest | null>(null);
  let master = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let input = $state<HTMLInputElement>();

  function show(next: VerifyRequest) {
    request = next;
    master = "";
    error = null;
    queueMicrotask(() => input?.focus());
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    void api.verifyPending().then((pending) => pending && show(pending)).catch(() => {});
    void api.onVerify(show).then((unlisten) => (stop = unlisten));
    return () => stop?.();
  });

  async function answer(confirmed: boolean) {
    if (!request || busy) return;
    busy = true;
    try {
      await api.verifyAnswer(request.id, confirmed ? master : null);
      request = null;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
    } finally {
      master = "";
      busy = false;
    }
  }
</script>

{#if request}
  <div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
    <div
      class="dialog glass glass--3"
      role="dialog"
      aria-modal="true"
      aria-labelledby="verify-title"
      transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}
    >
      <form onsubmit={(event) => (event.preventDefault(), answer(true))}>
        <header>
          <span class="icon" aria-hidden="true"><UserKey size={18} /></span>
          <h2 id="verify-title">Confirm it is you</h2>
        </header>
        <p>
          <b>{request.site}</b> asks to confirm it is you before your passkey signs you in. Windows Hello is not set up on
          this PC, so type your master password.
        </p>
        <input
          class="input"
          type="password"
          bind:this={input}
          bind:value={master}
          placeholder="Master password"
          autocomplete="current-password"
          aria-label="Master password"
        />
        {#if error}<p class="error">{error}</p>{/if}
        <footer>
          <button type="button" class="btn ghost" disabled={busy} onclick={() => answer(false)}>Cancel</button>
          <button type="submit" class="btn primary" disabled={busy || !master}>Confirm</button>
        </footer>
      </form>
    </div>
  </div>
{/if}

<svelte:window onkeydown={(e) => request && e.key === "Escape" && answer(false)} />

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 92;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--scrim);
  }

  .dialog {
    width: min(420px, 100%);
    border-radius: var(--radius-xl);
  }

  form {
    display: grid;
    gap: 12px;
    padding: 20px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 10px;
    background: rgb(var(--accent-rgb) / 0.16);
    color: #c5cafd;
  }

  h2 {
    margin: 0;
    font-size: 17px;
  }

  p {
    margin: 0;
    color: var(--text-2);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .error {
    color: #ffb6a8;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
