<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Info from "@lucide/svelte/icons/info";
  import X from "@lucide/svelte/icons/x";
  import { toast } from "../toast.svelte";

  const icons = { success: CircleCheck, error: CircleAlert, info: Info };
</script>

<div class="toaster" aria-live="polite">
  {#each toast.visible as t (t.id)}
    {@const Icon = icons[t.kind]}
    <div class="toast {t.kind}" role={t.kind === "error" ? "alert" : "status"} transition:fly={{ x: 24, duration: 200, easing: cubicOut }}>
      <span class="icon"><Icon size={17} strokeWidth={2} /></span>
      <p class="message selectable">{t.message}</p>
      {#if t.count > 1}<span class="count">×{t.count}</span>{/if}
      {#if t.action}
        {@const action = t.action}
        <button
          class="action"
          onclick={() => {
            toast.dismiss(t.id);
            action.run();
          }}>{action.label}</button
        >
      {/if}
      <button class="close" aria-label="Dismiss" onclick={() => toast.dismiss(t.id)}>
        <X size={14} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toaster {
    position: fixed;
    top: calc(var(--titlebar-h) + 12px);
    right: calc(var(--gap) + 14px);
    z-index: 120;
    display: grid;
    gap: 8px;
    width: min(360px, calc(100vw - 48px));
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 11px 12px;
    border: 1px solid rgb(255 255 255 / 0.1);
    border-radius: var(--radius-md);
    background: var(--toast-bg);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.07), var(--elev-2);
    pointer-events: auto;
  }

  .icon {
    flex: none;
    display: grid;
    margin-top: 1px;
  }

  .action {
    flex: none;
    align-self: center;
    padding: 3px 9px;
    border: 1px solid rgb(var(--accent-rgb) / 0.3);
    border-radius: 999px;
    background: rgb(var(--accent-rgb) / 0.12);
    color: var(--text-1);
    font-size: 11.5px;
    font-weight: 600;
  }

  .action:hover {
    background: rgb(var(--accent-rgb) / 0.22);
  }

  .success .icon {
    color: var(--ok);
  }

  .info .icon {
    color: var(--accent);
  }

  .error .icon {
    color: var(--danger);
  }

  .error {
    border-color: rgb(229 72 77 / 0.35);
  }

  .message {
    flex: 1;
    font-size: 13px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }

  .count {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.1);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  .close {
    flex: none;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin: -2px -4px 0 0;
    border-radius: 6px;
    color: var(--text-3);
  }

  .close:hover {
    background: var(--hover);
    color: var(--text-1);
  }
</style>
