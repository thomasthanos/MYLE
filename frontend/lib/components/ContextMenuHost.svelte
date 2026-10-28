<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import type { Component } from "svelte";
  import Copy from "@lucide/svelte/icons/copy";
  import Scissors from "@lucide/svelte/icons/scissors";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import TextSelect from "@lucide/svelte/icons/text-select";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import { nav } from "../nav.svelte";
  import { toast } from "../toast.svelte";
  import { actionsAt, menuPosition, textActions } from "../context-menu";
  import { portal } from "../portal";

  let { appActions = false }: { appActions?: boolean } = $props();
  type TextField = HTMLInputElement | HTMLTextAreaElement;
  interface Item { id: string; label: string; icon: Component<{ size?: number }>; shortcut?: string; disabled?: boolean; group: number; run: () => void | Promise<void> }
  interface Context { target: HTMLElement; field: TextField | null; range: Range | null; start: number | null; end: number | null; value: string; selectedText: string }
  let context = $state.raw<Context | null>(null);
  let items = $state.raw<Item[]>([]);
  let menu = $state<HTMLDivElement>();
  let left = $state(0); let top = $state(0); let placed = $state(false);
  let active = $state(0);
  let previous: HTMLElement | null = null;
  let opening = 0;

  function restoreSelection(current: Context) {
    if (!current.target.isConnected) return false;
    if (current.field) {
      current.field.focus({ preventScroll: true });
      if (current.start !== null && current.end !== null) current.field.setSelectionRange(current.start, current.end);
    } else if (current.range && current.range.startContainer.isConnected && current.range.endContainer.isConnected) {
      const selection = window.getSelection();
      selection?.removeAllRanges(); selection?.addRange(current.range);
    }
    return true;
  }

  function close(restoreFocus = true) {
    opening++;
    context = null;
    items = [];
    if (restoreFocus && previous?.isConnected) previous.focus({ preventScroll: true });
    previous = null;
  }

  async function copy(current: Context) {
    if (!current.selectedText) return;
    await navigator.clipboard.writeText(current.selectedText);
  }

  function replaceSelection(current: Context, text: string) {
    const field = current.field;
    if (!field || !field.isConnected || field.disabled || field.readOnly) return;
    // An async clipboard read must not overwrite typing that happened meanwhile.
    if (field.value !== current.value) throw new Error("The field changed. Open the menu and try again.");
    restoreSelection(current);
    // Chromium's edit command preserves native undo and emits the normal input event, so Svelte bindings and the app's validation stay in sync.
    if (!document.execCommand("insertText", false, text)) {
      field.setRangeText(text, current.start ?? field.value.length, current.end ?? field.value.length, "end");
      field.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
    }
  }

  async function open(target: HTMLElement, x: number, y: number) {
    if (target.closest("[inert], [data-no-context-menu]")) return;
    const candidate = target.closest("input, textarea");
    const field = (candidate instanceof HTMLInputElement && ["text", "search", "email", "url", "tel", "password"].includes(candidate.type)) || candidate instanceof HTMLTextAreaElement ? candidate as TextField : null;
    const selection = window.getSelection();
    const range = selection?.rangeCount ? selection.getRangeAt(0).cloneRange() : null;
    const start = field?.selectionStart ?? null; const end = field?.selectionEnd ?? null;
    const value = field?.value ?? "";
    const selectedText = field ? value.slice(start ?? 0, end ?? 0) : selection?.toString() ?? "";
    const sensitiveSelection = !field && !!range && [...document.querySelectorAll("[data-sensitive], .secret")].some((element) => range.intersectsNode(element));
    const sensitive = !!target.closest("[data-sensitive], .secret") || field?.type === "password" || !!field?.autocomplete.includes("password") || sensitiveSelection;
    const permissions = textActions({ editable: !!field, disabled: !!field?.disabled, readOnly: !!field?.readOnly, sensitive, noCopy: !!target.closest("[data-no-copy]"), selectedText, hasText: !!value });
    const current: Context = { target: field ?? target, field, range, start, end, value, selectedText };
    // What the page offers here (a file's Open in Explorer, say) comes first.
    const page = field ? [] : actionsAt(target, x, y);
    const next: Item[] = page.map((action) => ({ ...action, id: `page-${action.id}`, group: -1 }));
    if (field) next.push({ id: "cut", label: "Cut", icon: Scissors, shortcut: "Ctrl+X", disabled: !permissions.cut, group: 0, run: async () => { await copy(current); replaceSelection(current, ""); } });
    if (permissions.copy || !page.length) next.push({ id: "copy", label: "Copy", icon: Copy, shortcut: "Ctrl+C", disabled: !permissions.copy, group: 0, run: () => copy(current) });
    if (field) {
      next.push({ id: "paste", label: "Paste", icon: ClipboardPaste, shortcut: "Ctrl+V", disabled: !permissions.paste, group: 0, run: async () => replaceSelection(current, await navigator.clipboard.readText()) });
      next.push({ id: "select", label: "Select all", icon: TextSelect, shortcut: "Ctrl+A", disabled: !permissions.selectAll, group: 0, run: () => { if (field.isConnected) { field.focus({ preventScroll: true }); field.select(); } } });
    }
    const link = target.closest<HTMLAnchorElement>("a[href]");
    if (link && !target.closest("[data-no-copy], [data-sensitive]")) {
      next.push({ id: "link-copy", label: "Copy link address", icon: Copy, group: 1, run: () => navigator.clipboard.writeText(link.href) });
      next.push({ id: "link-open", label: "Open link", icon: ExternalLink, group: 1, run: () => { if (link.isConnected) link.click(); } });
    }
    if (appActions && !target.closest("[role='dialog'], [role='alertdialog']")) {
      next.push({ id: "settings", label: "App settings", icon: Settings2, group: 2, run: () => nav.go("settings") });
      next.push({ id: "sidebar", label: nav.collapsed ? "Show sidebar" : "Hide sidebar", icon: PanelLeft, shortcut: "Ctrl+B", group: 2, run: () => nav.toggleSidebar() });
    }
    previous = field ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    context = current; items = next; placed = false;
    active = Math.max(0, next.findIndex((item) => !item.disabled));
    left = x; top = y;
    const revision = ++opening;
    await tick();
    if (!menu || revision !== opening) return;
    const bounds = menu.getBoundingClientRect();
    ({ left, top } = menuPosition(x, y, bounds.width, bounds.height, window.innerWidth, window.innerHeight));
    placed = true;
    // A hidden menu cannot receive focus until Svelte applies its visible class.
    await tick();
    if (!menu || revision !== opening) return;
    const first = menu.querySelectorAll<HTMLButtonElement>("[role='menuitem']")[active];
    (first && !first.disabled ? first : menu).focus({ preventScroll: true });
  }

  function contextmenu(event: MouseEvent) {
    event.preventDefault();
    const target = event.target instanceof HTMLElement ? event.target : event.target instanceof Element ? event.target.closest<HTMLElement>("button, a, div, span, label") : null;
    if (target) void open(target, event.clientX, event.clientY);
  }

  function step(direction: number) {
    for (let offset = 1; offset <= items.length; offset++) {
      const next = (active + direction * offset + items.length) % items.length;
      if (!items[next].disabled) { active = next; menu?.querySelectorAll<HTMLButtonElement>("[role='menuitem']")[next]?.focus(); break; }
    }
  }

  async function run(item: Item) {
    if (item.disabled) return;
    close();
    try { await item.run(); }
    catch (error) { toast.error(error instanceof Error ? error.message : "This action is unavailable. Try its keyboard shortcut."); }
  }

  function keydown(event: KeyboardEvent) {
    if (context) {
      if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); event.stopPropagation(); close(); }
      else if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); event.stopPropagation(); step(event.key === "ArrowDown" ? 1 : -1); }
      else if (event.key === "Home" || event.key === "End") { event.preventDefault(); active = event.key === "Home" ? items.length - 1 : 0; step(event.key === "Home" ? 1 : -1); }
      return;
    }
    if ((event.shiftKey && event.key === "F10") || event.key === "ContextMenu") {
      const target = document.activeElement;
      if (!(target instanceof HTMLElement)) return;
      event.preventDefault();
      const bounds = target.getBoundingClientRect();
      void open(target, bounds.left + 12, bounds.bottom);
    }
  }

  onMount(() => {
    const listeners: [EventTarget, string, EventListener, boolean?][] = [
      [document, "contextmenu", contextmenu as EventListener],
      [document, "keydown", keydown as EventListener, true],
      [document, "pointerdown", ((event: PointerEvent) => { if (context && event.target instanceof Node && !menu?.contains(event.target)) close(false); }) as EventListener, true],
      [document, "scroll", ((event: Event) => { if (context && event.target instanceof Node && !menu?.contains(event.target)) close(false); }) as EventListener, true],
      [window, "blur", () => close(false)], [window, "resize", () => close(false)],
    ];
    const cleanup = listeners.map(([target, name, listener, capture]) => {
      const handle: EventListener = (event) => untrack(() => listener(event));
      target.addEventListener(name, handle, capture);
      return () => target.removeEventListener(name, handle, capture);
    });
    return () => { opening++; for (const remove of cleanup) remove(); };
  });
</script>

{#if context}
  <div class="context-menu" class:placed bind:this={menu} style:left={`${left}px`} style:top={`${top}px`} role="menu" tabindex="-1" aria-label="MYLE context menu" {@attach portal}>
    <div class="caption" aria-hidden="true">MYLE</div>
    {#each items as item, index (item.id)}
      {#if index > 0 && item.group !== items[index - 1].group}<div class="divider" role="separator"></div>{/if}
      <button role="menuitem" disabled={item.disabled} tabindex={index === active ? 0 : -1} onfocus={() => (active = index)} onclick={() => run(item)}>
        <item.icon size={15} /><span>{item.label}</span>{#if item.shortcut}<kbd>{item.shortcut}</kbd>{/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .context-menu { position: fixed; z-index: 250; width: min(244px, calc(100vw - 16px)); max-height: calc(100vh - 16px); overflow: auto; padding: 6px; border: 1px solid rgb(var(--accent-rgb) / .18); border-radius: 12px; background: var(--bg-solid-panel, #1c2130); box-shadow: 0 16px 44px rgb(0 0 0 / .45), inset 0 1px 0 rgb(255 255 255 / .055); visibility: hidden; }
  .context-menu.placed { visibility: visible; }
  .caption { padding: 4px 9px 6px; color: var(--text-3); font-size: 10px; font-weight: 600; letter-spacing: .1em; }
  button { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 34px; padding: 7px 9px; border-radius: 7px; color: var(--text-2); text-align: left; font-size: 12px; }
  button :global(svg) { flex: none; color: var(--text-3); }
  button:hover:not(:disabled), button:focus-visible { background: rgb(var(--accent-rgb) / .13); color: var(--text-1); outline: none; }
  button:focus-visible { box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / .38); }
  button:disabled { opacity: .38; cursor: default; }
  kbd { margin-left: auto; color: var(--text-3); font: 10px var(--font-sans); }
  .divider { height: 1px; margin: 5px 3px; background: rgb(255 255 255 / .07); }
</style>
