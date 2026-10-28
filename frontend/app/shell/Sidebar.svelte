<script lang="ts">
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { badges } from "../../lib/badges.svelte";
  import { nav } from "../../lib/nav.svelte";
  import { GROUP_LABELS, SIDEBAR_SHORTCUT, nextFocus, sections, shortcutOf, tooltipPosition } from "../../lib/sidebar";
  import { account } from "../account/account.svelte";
  import { canOpen, pages, type PageDef } from "../pages/registry";

  const shown = $derived(pages.filter((p) => canOpen(p, account.owner)));
  const topSections = $derived(sections(shown));
  const bottomPages = $derived(shown.filter((p) => p.group === "bottom"));

  /* Shown after a pause while the labels are visible; at once when collapsed (the tooltip is then the only place the name is) or on keyboard focus. */
  const TIP_DELAY_MS = 550;
  const OWNER_NOTE = "Only visible to the owner account.";
  const COLLAPSE = "collapse";

  interface Tip { title: string; description?: string; shortcut?: string | null; owner?: boolean; x: number; y: number }

  // Rendered outside the sidebar panel, so nothing in it can clip the tooltip.
  let tip = $state<Tip | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;
  let tipTarget: HTMLElement | undefined;
  let sidebar = $state<HTMLElement>();
  let list = $state<HTMLElement>();
  let tooltip = $state<HTMLDivElement>();
  let tooltipHeight = $state(0);
  let viewportHeight = $state(0);
  const tipPlacement = $derived(tooltipPosition(tip?.y ?? 0, tooltipHeight, viewportHeight));
  /** Which ends of the page list have more pages scrolled out of view. */
  let more = $state({ above: false, below: false });

  function measure() {
    if (!list) return;
    const { scrollTop, scrollHeight, clientHeight } = list;
    more = { above: scrollTop > 1, below: scrollTop + clientHeight < scrollHeight - 1 };
  }

  $effect(() => {
    if (!list || !topSections.length) return;
    const observer = new ResizeObserver(measure);
    observer.observe(list);
    for (const child of list.children) observer.observe(child);
    return () => observer.disconnect();
  });

  $effect(() => {
    if (!tooltip) return;
    const element = tooltip;
    const measureTooltip = () => { tooltipHeight = element.offsetHeight; };
    measureTooltip();
    const observer = new ResizeObserver(measureTooltip);
    observer.observe(element);
    return () => observer.disconnect();
  });

  function place(target: HTMLElement): { x: number; y: number } {
    const item = target.getBoundingClientRect();
    // The 8px arrow reaches the button's edge in either sidebar mode.
    return { x: item.right + 8, y: item.top + item.height / 2 };
  }

  function showTip(target: HTMLElement, content: Omit<Tip, "x" | "y">, immediate: boolean) {
    clearTimeout(tipTimer);
    const open = () => {
      tipTarget = target;
      tip = { ...content, ...place(target) };
    };
    if (immediate) open();
    else {
      tip = null;
      tipTimer = setTimeout(open, TIP_DELAY_MS);
    }
  }

  function hideTip() {
    clearTimeout(tipTimer);
    tipTarget = undefined;
    tip = null;
  }

  function repositionTip() {
    if (tip && tipTarget?.isConnected) tip = { ...tip, ...place(tipTarget) };
  }

  /** The page's shortcut; the toggle's is App.svelte's Ctrl+B. */
  function shortcutFor(page: PageDef): string | null {
    return page.id === COLLAPSE ? SIDEBAR_SHORTCUT : shortcutOf(shown, page.id);
  }

  function pageTip(page: PageDef) {
    return { title: page.label, description: page.description, shortcut: shortcutFor(page), owner: page.ownerOnly ?? false };
  }

  function toggle() {
    hideTip();
    nav.toggleSidebar();
  }

  /** Up and Down (and Home, End) move between the sidebar's buttons. */
  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      hideTip();
      return;
    }
    if (!sidebar || event.altKey || event.ctrlKey || event.metaKey) return;
    const items = [...sidebar.querySelectorAll<HTMLButtonElement>("button.item")];
    const next = nextFocus(items.indexOf(document.activeElement as HTMLButtonElement), items.length, event.key);
    if (next === null) return;
    event.preventDefault();
    items[next].focus();
  }

  /** Keyboard focus shows the tooltip at once; a click's focus does not. */
  function focusedByKeyboard(event: FocusEvent): boolean {
    return (event.currentTarget as HTMLElement).matches(":focus-visible");
  }
</script>

<svelte:window bind:innerHeight={viewportHeight} onresize={repositionTip} />

{#snippet item(page: PageDef, active: boolean, onclick: () => void, badge = 0)}
  {@const shortcut = shortcutFor(page)}
  <button
    class="item"
    class:active
    class:owner={page.ownerOnly}
    data-no-tooltip
    aria-current={active ? "page" : undefined}
    aria-label={badge ? `${page.label} (${badge})` : page.label}
    aria-describedby="nav-about-{page.id}"
    aria-keyshortcuts={shortcut ? shortcut.replace("Ctrl", "Control") : undefined}
    {onclick}
    onpointerenter={(e) => showTip(e.currentTarget, pageTip(page), nav.collapsed)}
    onpointerleave={hideTip}
    onpointerdown={hideTip}
    onfocus={(e) => focusedByKeyboard(e) && showTip(e.currentTarget, pageTip(page), true)}
    onblur={hideTip}
  >
    <span class="icon">
      <span class="glyph"><page.icon size={24} strokeWidth={1.65} /></span>
      {#if badge}<i class="dot" aria-hidden="true">{badge > 9 ? "9+" : badge}</i>{/if}
    </span>
    <span class="label">{page.label}</span>
    <span id="nav-about-{page.id}" hidden>{page.description}{page.ownerOnly ? ` ${OWNER_NOTE}` : ""}</span>
    {#if badge}<span class="badge" aria-hidden="true">{badge > 99 ? "99+" : badge}</span>{/if}
  </button>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<nav class="sidebar glass" class:collapsed={nav.collapsed} aria-label="Main" bind:this={sidebar} onkeydown={onKeydown}>
  <div
    class="top"
    class:more-above={more.above}
    class:more-below={more.below}
    bind:this={list}
    onscroll={() => {
      hideTip();
      measure();
    }}
  >
    {#each topSections as section (section.group)}
      <div class="section" class:owner-section={section.group === "owner"}>
        <h2 class="section-heading" id="sidebar-section-{section.group}">
          {#if section.group === "owner"}<ShieldCheck size={14} strokeWidth={2} />{/if}
          {GROUP_LABELS[section.group]}
        </h2>
        <div class="group" role="group" aria-labelledby="sidebar-section-{section.group}">
          {#each section.pages as page (page.id)}
            {@render item(page, nav.current === page.id, () => nav.go(page.id), badges.of(page.id))}
          {/each}
        </div>
      </div>
    {/each}
  </div>

  <div class="group bottom">
    {#each bottomPages as page (page.id)}
      {@render item(page, nav.current === page.id, () => nav.go(page.id))}
    {/each}
    <div class="divider" aria-hidden="true"></div>
    {@render item(
      { id: COLLAPSE, label: nav.collapsed ? "Expand sidebar" : "Collapse sidebar", description: nav.collapsed ? "Show the page names." : "Show only the icons.", icon: nav.collapsed ? PanelLeftOpen : PanelLeftClose, group: "bottom" },
      false,
      toggle,
    )}
  </div>
</nav>

{#if tip}
  <div class="tooltip" role="tooltip" bind:this={tooltip} style:left="{tip.x}px" style:top="{tipPlacement.top}px" style:--tip-arrow-y="{tipPlacement.arrow}px">
    <div class="tip-head">
      <strong>{tip.title}</strong>
      {#if tip.shortcut}
        <span class="tip-shortcut" aria-label={tip.shortcut}>
          {#each tip.shortcut.split("+") as key}<kbd>{key}</kbd>{/each}
        </span>
      {/if}
    </div>
    {#if tip.description}<p>{tip.description}</p>{/if}
    {#if tip.owner}<p class="tip-owner"><ShieldCheck size={12} strokeWidth={2.25} /> {OWNER_NOTE}</p>{/if}
  </div>
{/if}

<style>
  .sidebar { container: navigation / size; grid-area: side; display: flex; flex-direction: column; justify-content: space-between; gap: 10px; min-width: 0; min-height: 0; padding: 12px 9px; border-radius: var(--shell-panel-radius); background: var(--sidebar-fill); }
  /* On a short window the page list scrolls; Settings and the toggle stay. */
  .top { min-height: 0; overflow-x: hidden; overflow-y: auto; --fade-top: 0px; --fade-bottom: 0px; mask-image: linear-gradient(to bottom, transparent 0, #000 var(--fade-top), #000 calc(100% - var(--fade-bottom)), transparent 100%); }
  .top.more-above { --fade-top: 22px; }
  .top.more-below { --fade-bottom: 22px; }
  /* The actual sidebar height sets the rhythm, including window scaling. */
  .section + .section { margin-top: clamp(8px, calc(7.5cqh - 33px), 30px); }
  .section-heading { display: flex; align-items: center; gap: 8px; min-height: 22px; margin: 0 0 clamp(3px, calc(2.5cqh - 12px), 10px); padding: 0 12px; color: var(--text-1); font-size: clamp(13px, 1.6cqh, 15px); font-weight: 700; line-height: 1.35; white-space: nowrap; }
  .section-heading::after { content: ""; flex: 1; min-width: 16px; height: 1px; background: linear-gradient(90deg, rgb(var(--accent-rgb) / 0.35), transparent); }
  .section-heading :global(svg) { flex: none; }
  .collapsed .section-heading { display: none; }
  .collapsed .section + .section { margin-top: 6px; padding-top: 6px; border-top: 1px solid rgb(255 255 255 / 0.08); }
  .group { display: grid; gap: 2px; }
  .section .group { gap: clamp(0px, calc(1cqh - 5px), 5px); }
  .collapsed .section .group { gap: clamp(0px, calc(1cqh - 5px), 2px); }
  .item { position: relative; display: flex; align-items: center; width: 100%; height: clamp(26px, 5cqh, 40px); border-radius: var(--radius-md); color: var(--text-2); overflow: hidden; transition: background var(--dur-fast), color var(--dur-fast), scale 140ms var(--ease-out); }
  .item:hover { background: var(--hover); color: var(--text-1); }
  .item:active { scale: 0.96; background: var(--press); transition-duration: var(--dur-fast), var(--dur-fast), 50ms; }
  /* Inside the rounded item, so the ring is never cut by the panel's edge. */
  .item:focus-visible { outline: 2px solid rgb(var(--accent-rgb) / 0.75); outline-offset: -2px; }
  .item.active { background: var(--selected); color: var(--text-1); font-weight: 600; box-shadow: var(--btn-sheen); }
  /* Glowing accent pill on the active item. */
  .item.active::before { content: ""; position: absolute; left: 0; top: 50%; width: 3px; height: 18px; margin-top: -9px; border-radius: 0 3px 3px 0; background: var(--accent); }
  /* The icon column is exactly as wide as the collapsed item, so icons never move. */
  .icon { position: relative; flex: none; display: grid; place-items: center; width: 48px; }
  .glyph { display: grid; place-items: center; width: 32px; height: clamp(26px, 5cqh, 32px); color: var(--icon-neutral); }
  /* Count on the right while expanded; on the icon's corner while collapsed. */
  .badge { flex: none; min-width: 19px; margin: 0 10px 0 auto; padding: 1px 6px; border-radius: 999px; background: var(--accent-grad); color: #fff; font-size: 10.5px; font-weight: 700; font-variant-numeric: tabular-nums; text-align: center; box-shadow: 0 0 10px var(--accent-glow); transition: opacity var(--dur-med) var(--ease-out); }
  /* The count sits on the icon's upper-right corner. */
  .dot { position: absolute; top: 0; left: 30px; display: grid; place-items: center; min-width: 15px; height: 15px; padding: 0 4px; border-radius: 999px; background: var(--accent-grad); color: #fff; font-size: 9px; font-style: normal; font-weight: 700; line-height: 1; font-variant-numeric: tabular-nums; box-shadow: 0 0 0 2px var(--bg-1), 0 0 8px var(--accent-glow); opacity: 0; transform: scale(0.6); transition: opacity var(--dur-med) var(--ease-out), transform var(--dur-med) var(--ease-out); }
  .collapsed .badge { opacity: 0; }
  .collapsed .dot { opacity: 1; transform: scale(1); }
  .label { white-space: nowrap; font-weight: 500; transition: opacity var(--dur-med) var(--ease-out), transform var(--dur-med) var(--ease-out); }
  .item.active .label { font-weight: 600; }
  .collapsed .label { opacity: 0; transform: translateX(-6px); }
  .divider { height: 1px; margin: 6px 8px; background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.08), transparent); }
  .tooltip { position: fixed; z-index: 50; width: 312px; max-width: calc(100vw - 24px); padding: 12px 14px 14px; border: 1px solid rgb(255 255 255 / 0.1); border-radius: 14px; background: linear-gradient(180deg, rgb(255 255 255 / 0.035), transparent), var(--tooltip-bg); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.045), 0 4px 10px rgb(0 0 0 / 0.24), 0 10px 24px -10px rgb(0 0 0 / 0.6); font-size: 13px; pointer-events: none; animation: tip-in var(--dur-fast) var(--ease-out); }
  .tooltip::before { content: ""; position: absolute; left: -6px; top: var(--tip-arrow-y); width: 10px; height: 10px; transform: translateY(-50%) rotate(45deg); border-left: 1px solid rgb(255 255 255 / 0.1); border-bottom: 1px solid rgb(255 255 255 / 0.1); border-radius: 1px; background: linear-gradient(rgb(255 255 255 / 0.018), rgb(255 255 255 / 0.018)), var(--tooltip-bg); }
  .tip-head { display: flex; align-items: center; gap: 9px; }
  .tip-head strong { min-width: 0; color: var(--text-1); font-size: 14px; font-weight: 700; line-height: 1.25; }
  .tip-shortcut { flex: none; display: flex; gap: 3px; margin-left: auto; padding-left: 3px; }
  .tooltip kbd { display: grid; place-items: center; min-width: 19px; height: 22px; padding: 0 5px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 5px; background: rgb(255 255 255 / 0.035); box-shadow: 0 1px 0 rgb(0 0 0 / 0.3); color: var(--text-3); font-family: var(--font-sans); font-size: 10px; font-weight: 700; line-height: 1; }
  .tooltip p { margin: 7px 0 0; color: var(--text-2); line-height: 1.5; text-wrap: pretty; }
  .tooltip .tip-owner { display: flex; align-items: center; gap: 6px; margin-top: 10px; padding: 9px 0 0; border-top: 1px solid rgb(255 255 255 / 0.06); color: var(--text-3); font-size: 11px; }
  .tip-owner :global(svg) { flex: none; }
  @container navigation (max-height: 480px) {
    .section + .section { margin-top: 4px; }
    .section-heading { min-height: 18px; margin-bottom: 2px; }
    .bottom .divider { margin-block: 2px; }
  }
  @keyframes tip-in { from { opacity: 0; transform: translateX(-4px); } }
  @media (prefers-reduced-motion: reduce) { .tooltip { animation: none; } }
</style>
