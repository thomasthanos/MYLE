<script lang="ts">
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Info from "@lucide/svelte/icons/info";
  import Logo from "../lib/components/Logo.svelte";
  import WindowButton from "../lib/components/WindowButton.svelte";
  import type { SetupApi, SetupState } from "./api";

  let { info, api }: { info: SetupState; api: SetupApi } = $props();

  const notice = $derived(info.notice);
  const kind = $derived(notice?.kind ?? "warning");
  const title = $derived(notice?.title ?? "Setup is already running");
  const message = $derived(notice?.message ?? "Another instance of MYLE Setup is already open.");
  const actionLabel = $derived(notice?.actionLabel ?? (kind === "warning" ? "Bring to front" : "OK"));

  function close() {
    void api.exit();
  }

  function handleAction() {
    if (api.focusExisting) {
      void api.focusExisting();
    } else {
      void api.exit();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      handleAction();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="notice-window {kind}">
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand">
      <Logo size={16} />
      <span class="name">MYLE</span>
      <span class="badge">Setup</span>
    </div>
    <div class="controls">
      <WindowButton kind="close" label="Close" onclick={close} />
    </div>
  </header>

  <main class="content">
    <div class="icon-badge" aria-hidden="true">
      {#if kind === "error"}
        <CircleAlert size={22} strokeWidth={2.2} />
      {:else if kind === "info"}
        <Info size={22} strokeWidth={2.2} />
      {:else}
        <TriangleAlert size={22} strokeWidth={2.2} />
      {/if}
    </div>

    <div class="text-body">
      <h2>{title}</h2>
      <p class="message selectable">{message}</p>
    </div>
  </main>

  <footer class="footer">
    <button type="button" class="btn" onclick={close}>Close</button>
    <button type="button" class="btn primary" onclick={handleAction}>{actionLabel}</button>
  </footer>
</div>

<style>
  .notice-window {
    width: 100%;
    height: 100dvh;
    display: flex;
    flex-direction: column;
    background: radial-gradient(ellipse at 50% 0%, #18223c 0%, #0d121e 65%, #080a11 100%);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 12px;
    box-shadow:
      0 18px 45px rgba(0, 0, 0, 0.75),
      inset 0 1px 0 rgba(255, 255, 255, 0.1);
    color: #edf5ff;
    font-family: var(--font-sans, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif);
    overflow: hidden;
    user-select: none;
  }

  .titlebar {
    position: relative;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 38px;
    padding: 0 8px 0 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.04), transparent);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: var(--font-titlebar);
    font-size: 14.5px;
    font-weight: 400;
    pointer-events: none;
  }

  .name {
    font-weight: 500;
    letter-spacing: 0.01em;
    background: linear-gradient(180deg, #ffffff 30%, #8ba2ff 130%);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }

  .badge {
    padding: 1px 7px;
    border-radius: 999px;
    font-family: var(--font-brand);
    font-size: 10.5px;
    font-weight: 600;
    color: #cfd6ff;
    background: rgba(107, 133, 255, 0.14);
    box-shadow: inset 0 0 0 1px rgba(107, 133, 255, 0.25);
  }

  .controls {
    display: flex;
    align-items: center;
  }

  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 18px 20px 10px;
  }

  .icon-badge {
    flex: none;
    width: 44px;
    height: 44px;
    border-radius: 12px;
    display: grid;
    place-items: center;
    margin-top: 2px;
  }

  /* Warning / Already running */
  .warning .icon-badge {
    background: radial-gradient(circle at 35% 35%, rgba(245, 158, 11, 0.28), rgba(245, 158, 11, 0.08));
    border: 1px solid rgba(245, 158, 11, 0.4);
    color: #fbbf24;
    box-shadow: 0 4px 14px rgba(245, 158, 11, 0.22);
  }

  /* Error */
  .error .icon-badge {
    background: radial-gradient(circle at 35% 35%, rgba(239, 68, 68, 0.28), rgba(239, 68, 68, 0.08));
    border: 1px solid rgba(239, 68, 68, 0.4);
    color: #f87171;
    box-shadow: 0 4px 14px rgba(239, 68, 68, 0.22);
  }

  /* Info */
  .info .icon-badge {
    background: radial-gradient(circle at 35% 35%, rgba(59, 130, 246, 0.28), rgba(59, 130, 246, 0.08));
    border: 1px solid rgba(59, 130, 246, 0.4);
    color: #60a5fa;
    box-shadow: 0 4px 14px rgba(59, 130, 246, 0.22);
  }

  .text-body {
    flex: 1;
    min-width: 0;
  }

  h2 {
    font-size: 16px;
    font-weight: 650;
    line-height: 1.3;
    color: #ffffff;
    margin: 0 0 6px;
    letter-spacing: -0.01em;
  }

  .message {
    font-size: 13px;
    line-height: 1.5;
    color: #aab9d0;
    margin: 0;
    word-break: break-word;
  }

  .footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 18px 16px;
  }

  .btn {
    height: 32px;
    padding: 0 14px;
    border-radius: 8px;
    font-size: 12.5px;
    font-weight: 550;
    cursor: pointer;
    transition: filter 140ms, transform 100ms;
  }

  .btn:hover {
    filter: brightness(1.1);
  }

  .btn:active {
    transform: translateY(0.5px);
  }
</style>
