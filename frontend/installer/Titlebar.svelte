<script lang="ts">
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Logo from "../lib/components/Logo.svelte";
  import WindowButton from "../lib/components/WindowButton.svelte";

  let {
    label,
    busy = false,
    onclose,
  }: { label: string; busy?: boolean; onclose: () => void } = $props();

  const win = isTauri() ? getCurrentWindow() : null;
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand">
    <Logo size={18} />
    <span class="name">MYLE</span>
    <span class="badge">{label}</span>
  </div>
  <div class="controls">
    <WindowButton kind="minimize" label="Minimize" onclick={() => win?.minimize()} />
    <WindowButton
      kind="close"
      label="Close"
      disabled={busy}
      title={busy ? "Setup is working, please wait" : undefined}
      onclick={onclose}
    />
  </div>
</header>

<style>
  .titlebar { position: relative; z-index: 2; display: flex; align-items: stretch; justify-content: space-between; height: 42px; padding-left: 16px; background: radial-gradient(70% 180% at 0% 0%, rgb(98 111 231 / 0.1), transparent 58%), linear-gradient(180deg, rgb(24 30 45 / 0.5), rgb(13 18 29 / 0.2)); box-shadow: inset 0 -1px 0 var(--titlebar-divider); }

  .brand { display: flex; align-items: center; gap: 9px; font-family: var(--font-titlebar); font-size: 16px; font-weight: 400; font-synthesis: none; line-height: 1.4; pointer-events: none; }

  .name { flex: none; font-weight: 400; letter-spacing: 0.01em; background: linear-gradient(180deg, #fff 30%, rgb(var(--accent-soft-rgb)) 130%); background-clip: text; color: transparent; }

  .badge { padding: 2px 8px; border-radius: 999px; font-family: var(--font-brand); font-size: 11px; font-weight: 600; color: #cfd6ff; background: rgb(var(--accent-rgb) / 0.14); box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.22); }

  .controls { display: flex; align-items: center; gap: 2px; padding-right: 11px; }
</style>
