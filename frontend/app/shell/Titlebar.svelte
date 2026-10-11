<script lang="ts">
  import { onMount } from "svelte";
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Logo from "../../lib/components/Logo.svelte";
  import WindowButton from "../../lib/components/WindowButton.svelte";
  import { nav } from "../../lib/nav.svelte";
  import { pages } from "../pages/registry";

  let { maximized = $bindable(false) }: { maximized?: boolean } = $props();
  const win = isTauri() ? getCurrentWindow() : null;
  const pageTitle = $derived(pages.find((p) => p.id === nav.current)?.label ?? "");
  let syncVersion = 0;

  async function syncMaximized() {
    if (!win) return;
    const version = ++syncVersion;
    const next = await win.isMaximized();
    if (version === syncVersion) maximized = next;
  }

  async function toggleMaximized() {
    if (!win) return;
    await win.toggleMaximize();
    await syncMaximized();
  }

  onMount(() => {
    if (!win) return;
    void syncMaximized();
    // Dragging an edge sends a resize event per mouse move: ask once it settles.
    let settle: ReturnType<typeof setTimeout> | undefined;
    const unlisten = win.onResized(() => {
      clearTimeout(settle);
      settle = setTimeout(() => void syncMaximized(), 80);
    });
    return () => {
      clearTimeout(settle);
      void unlisten.then((off) => off());
    };
  });
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand">
    <Logo size={20} />
    <span class="name">MYLE</span>
    {#if pageTitle}
      <span class="breadcrumb" aria-label={`Current page: ${pageTitle}`}>
        <span class="sep" aria-hidden="true">/</span>
        <span class="page">{pageTitle}</span>
      </span>
    {/if}
  </div>

  <div class="controls">
    <WindowButton kind="minimize" label="Minimize" onclick={() => win?.minimize()} />
    <WindowButton
      kind="maximize"
      label={maximized ? "Restore" : "Maximize"}
      onclick={() => void toggleMaximized()}
    />
    <WindowButton kind="close" label="Close" onclick={() => win?.close()} />
  </div>
</header>

<style>
  .titlebar { position: relative; grid-area: title; display: flex; align-items: stretch; justify-content: space-between; height: var(--titlebar-h); margin-inline: calc(var(--frame-gap) * -1); padding-left: 17px; background: radial-gradient(70% 180% at 0% 0%, rgb(98 111 231 / 0.11), transparent 58%), radial-gradient(58% 170% at 100% 0%, rgb(43 172 205 / 0.085), transparent 62%), var(--titlebar-fill); }

  .titlebar::after { content: ""; position: absolute; right: 16px; bottom: 0; left: 16px; height: 1px; background: linear-gradient(90deg, transparent, var(--titlebar-divider) 18%, var(--titlebar-divider) 82%, transparent); pointer-events: none; }

  .brand { display: flex; align-items: center; gap: 10px; min-width: 0; font-family: var(--font-titlebar); font-size: 16px; font-weight: 400; font-synthesis: none; line-height: 1.4; white-space: nowrap; /* Clicks fall through to the drag region. */ pointer-events: none; }

  .name { flex: none; font-weight: 400; letter-spacing: 0.01em; background: linear-gradient(180deg, #fff 30%, rgb(var(--accent-soft-rgb)) 130%); background-clip: text; color: transparent; text-shadow: none; }

  .breadcrumb { display: flex; align-items: center; gap: 9px; min-width: 0; }

  .sep { flex: none; color: rgb(205 215 242 / 0.28); }

  .page { overflow: hidden; color: rgb(214 222 245 / 0.75); font-weight: 400; letter-spacing: 0.01em; text-overflow: ellipsis; }

  .controls { display: flex; flex: none; align-items: center; gap: 2px; height: 100%; padding-right: 12px; }
</style>
