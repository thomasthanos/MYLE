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
    <div
      class="toast {t.kind}"
      role={t.kind === "error" ? "alert" : "status"}
      transition:fly={{ y: 16, duration: 220, easing: cubicOut }}
    >
      <div class="icon-badge" aria-hidden="true">
        <Icon size={16} strokeWidth={2.2} />
      </div>

      <div class="toast-body">
        <p class="message selectable">{t.message}</p>
        {#if t.count > 1}<span class="count">×{t.count}</span>{/if}
      </div>

      {#if t.action}
        {@const action = t.action}
        <button
          class="action"
          onclick={() => {
            toast.dismiss(t.id);
            action.run();
          }}
        >
          {action.label}
        </button>
      {/if}

      <button
        class="close"
        aria-label="Dismiss notification"
        title="Dismiss"
        onclick={() => toast.dismiss(t.id)}
      >
        <X size={14} strokeWidth={2} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toaster {
    position: absolute;
    bottom: 20px;
    right: 22px;
    z-index: 120;
    display: flex;
    flex-direction: column-reverse;
    gap: 10px;
    width: min(390px, calc(100% - 44px));
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: 14px;
    background: linear-gradient(135deg, rgba(28, 30, 36, 0.95), rgba(18, 20, 26, 0.98));
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow:
      0 16px 36px -6px rgba(0, 0, 0, 0.65),
      0 4px 12px -2px rgba(0, 0, 0, 0.4),
      inset 0 1px 0 rgba(255, 255, 255, 0.12);
    backdrop-filter: blur(20px) saturate(190%);
    -webkit-backdrop-filter: blur(20px) saturate(190%);
    pointer-events: auto;
    position: relative;
    overflow: hidden;
  }

  /* Status accents */
  .toast.info {
    border-color: rgba(107, 133, 255, 0.28);
    box-shadow:
      0 16px 36px -6px rgba(0, 0, 0, 0.65),
      0 0 22px -4px rgba(107, 133, 255, 0.18),
      inset 0 1px 0 rgba(255, 255, 255, 0.14);
  }

  .toast.success {
    border-color: rgba(16, 185, 129, 0.28);
    box-shadow:
      0 16px 36px -6px rgba(0, 0, 0, 0.65),
      0 0 22px -4px rgba(16, 185, 129, 0.18),
      inset 0 1px 0 rgba(255, 255, 255, 0.14);
  }

  .toast.error {
    border-color: rgba(229, 72, 77, 0.35);
    box-shadow:
      0 16px 36px -6px rgba(0, 0, 0, 0.65),
      0 0 22px -4px rgba(229, 72, 77, 0.22),
      inset 0 1px 0 rgba(255, 255, 255, 0.14);
  }

  /* Icon badges */
  .icon-badge {
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: 9px;
    display: grid;
    place-items: center;
  }

  .info .icon-badge {
    background: radial-gradient(circle at 35% 35%, rgba(107, 133, 255, 0.25), rgba(107, 133, 255, 0.08));
    border: 1px solid rgba(107, 133, 255, 0.35);
    color: #8ba2ff;
    box-shadow: 0 2px 8px rgba(107, 133, 255, 0.2);
  }

  .success .icon-badge {
    background: radial-gradient(circle at 35% 35%, rgba(16, 185, 129, 0.25), rgba(16, 185, 129, 0.08));
    border: 1px solid rgba(16, 185, 129, 0.35);
    color: #34d399;
    box-shadow: 0 2px 8px rgba(16, 185, 129, 0.2);
  }

  .error .icon-badge {
    background: radial-gradient(circle at 35% 35%, rgba(229, 72, 77, 0.28), rgba(229, 72, 77, 0.1));
    border: 1px solid rgba(229, 72, 77, 0.4);
    color: #ff6b70;
    box-shadow: 0 2px 8px rgba(229, 72, 77, 0.2);
  }

  /* Body */
  .toast-body {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 2px;
  }

  .message {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    font-weight: 500;
    line-height: 1.45;
    color: var(--text-1);
    word-break: break-word;
  }

  .count {
    flex: none;
    padding: 2px 7px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: var(--text-2);
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  /* Action button */
  .action {
    flex: none;
    height: 28px;
    padding: 0 12px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 8px;
    background: linear-gradient(135deg, #5369e4, #4459cb);
    color: #ffffff;
    font-size: 12px;
    font-weight: 600;
    box-shadow:
      0 2px 6px rgba(68, 89, 203, 0.35),
      inset 0 1px 0 rgba(255, 255, 255, 0.25);
    cursor: pointer;
    transition: filter 140ms, transform 100ms;
    white-space: nowrap;
  }

  .action:hover {
    filter: brightness(1.12);
    transform: translateY(-0.5px);
  }

  .action:active {
    transform: translateY(0.5px);
    filter: brightness(0.95);
  }

  /* Close */
  .close {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    color: var(--text-3);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background 140ms, color 140ms;
    margin-right: -2px;
  }

  .close:hover {
    background: rgba(255, 255, 255, 0.09);
    color: var(--text-1);
  }
</style>
