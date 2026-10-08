<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { confirmState } from "../confirm.svelte";

  let confirmButton = $state<HTMLButtonElement>();

  $effect(() => {
    if (confirmState.current) confirmButton?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    if (confirmState.current && e.key === "Escape") confirmState.answer(false);
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if confirmState.current}
  {@const c = confirmState.current}
  <div class="backdrop" transition:fade={{ duration: 150 }}>
    <div
      class="dialog glass glass--3"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
      transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}
    >
      <div class="head">
        {#if c.danger}<span class="warn"><TriangleAlert size={20} /></span>{/if}
        <h2 id="confirm-title">{c.title}</h2>
      </div>
      <p class="message selectable">{c.message}</p>
      <div class="actions">
        <button class="btn" onclick={() => confirmState.answer(false)}>{c.cancelLabel ?? "Cancel"}</button>
        <button bind:this={confirmButton} class="btn primary" class:danger={c.danger} onclick={() => confirmState.answer(true)}>
          {c.confirmLabel ?? "OK"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgb(4 6 12 / 0.55);
  }

  .dialog {
    width: min(460px, 100%);
    padding: 22px 22px 18px;
    border-radius: var(--radius-xl);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .warn {
    display: grid;
    color: #ffb454;
  }

  h2 {
    font-size: 17px;
  }

  .message {
    margin-top: 10px;
    color: var(--text-2);
    max-height: min(52vh, 420px);
    overflow-y: auto;
    overflow-wrap: anywhere;
    scrollbar-width: thin;
    white-space: pre-line;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }
</style>
