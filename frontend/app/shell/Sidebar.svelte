<script lang="ts">
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import { badges } from "../../lib/badges.svelte";
  import { nav } from "../../lib/nav.svelte";
  import { account } from "../account/account.svelte";
  import { canOpen, pages, type PageDef } from "../pages/registry";

  const shown = $derived(pages.filter((p) => canOpen(p, account.owner)));
  const topPages = $derived(shown.filter((p) => !p.bottom));
  const bottomPages = $derived(shown.filter((p) => p.bottom));

  // Collapsed mode shows a tooltip beside the hovered icon. It is rendered
  // outside the glass panel, because backdrop-filter would clip a fixed child.
  let tip = $state<{ text: string; x: number; y: number } | null>(null);

  function showTip(e: Event, text: string) {
    if (!nav.collapsed) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    tip = { text, x: r.right + 12, y: r.top + r.height / 2 };
  }

  function hideTip() {
    tip = null;
  }

  function toggle() {
    hideTip();
    nav.toggleSidebar();
  }
</script>

{#snippet item(label: string, Icon: PageDef["icon"], active: boolean, onclick: () => void, badge = 0)}
  <button
    class="item"
    class:active
    aria-current={active ? "page" : undefined}
    aria-label={badge ? `${label} (${badge})` : label}
    {onclick}
    onpointerenter={(e) => showTip(e, label)}
    onpointerleave={hideTip}
    onfocus={(e) => showTip(e, label)}
    onblur={hideTip}
  >
    <span class="icon"><Icon size={20} strokeWidth={1.75} />{#if badge}<i class="dot" aria-hidden="true">{badge > 9 ? "9+" : badge}</i>{/if}</span>
    <span class="label">{label}</span>
    {#if badge}<span class="badge" aria-hidden="true">{badge > 99 ? "99+" : badge}</span>{/if}
  </button>
{/snippet}

<nav class="sidebar glass" class:collapsed={nav.collapsed} aria-label="Main">
  <div class="group">
    {#each topPages as page (page.id)}
      {@render item(page.label, page.icon, nav.current === page.id, () => nav.go(page.id), badges.of(page.id))}
    {/each}
  </div>

  <div class="group">
    {#each bottomPages as page (page.id)}
      {@render item(page.label, page.icon, nav.current === page.id, () => nav.go(page.id))}
    {/each}
    <div class="divider"></div>
    {@render item(
      nav.collapsed ? "Expand sidebar" : "Collapse",
      nav.collapsed ? PanelLeftOpen : PanelLeftClose,
      false,
      toggle,
    )}
  </div>
</nav>

{#if tip}
  <div class="tooltip" role="tooltip" style:left="{tip.x}px" style:top="{tip.y}px">{tip.text}</div>
{/if}

<style>
  .sidebar {
    grid-area: side;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    min-width: 0;
    padding: 9px;
    border-radius: var(--shell-panel-radius);
  }

  .group {
    display: grid;
    gap: 2px;
  }

  .item {
    position: relative;
    display: flex;
    align-items: center;
    height: 40px;
    border-radius: var(--radius-md);
    color: var(--text-2);
    overflow: hidden;
    transition:
      background var(--dur-fast),
      color var(--dur-fast),
      scale 140ms var(--ease-out);
  }

  .item:hover {
    background: var(--hover);
    color: var(--text-1);
  }

  .item:active {
    scale: 0.96;
    background: var(--press);
    transition-duration: var(--dur-fast), var(--dur-fast), 50ms;
  }

  .item.active {
    background: var(--selected);
    color: var(--text-1);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.06);
  }

  /* Glowing accent pill on the active item */
  .item.active::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    width: 3px;
    height: 18px;
    margin-top: -9px;
    border-radius: 0 3px 3px 0;
    background: var(--accent-grad);
    box-shadow: 0 0 10px var(--accent-glow);
  }

  /* The icon column is exactly as wide as the collapsed item, so icons never move. */
  .icon {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 48px;
  }

  .icon :global(.custom-nav-icon) {
    opacity: 0.78;
    filter: saturate(0.72);
    transition:
      opacity var(--dur-fast),
      filter var(--dur-fast),
      transform var(--dur-fast) var(--ease-out);
  }

  .item:hover .icon :global(.custom-nav-icon),
  .item.active .icon :global(.custom-nav-icon) {
    opacity: 1;
    filter: saturate(1) drop-shadow(0 2px 6px rgb(var(--accent-rgb) / 0.3));
  }

  .item.active .icon :global(.custom-nav-icon) {
    transform: scale(1.05);
  }

  /* Count on the right while expanded; on the icon's corner while collapsed. */
  .badge {
    flex: none;
    min-width: 19px;
    margin: 0 10px 0 auto;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent-grad);
    color: #fff;
    font-size: 10.5px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    text-align: center;
    box-shadow: 0 0 10px var(--accent-glow);
    transition: opacity var(--dur-med) var(--ease-out);
  }

  /* The icon is 20px, centered in 48 x 40: the count sits on its top-right
     corner, cut out of it by a ring in the sidebar's colour, the way badges
     on icons look everywhere else, instead of a dot lying over the glyph. */
  .dot {
    position: absolute;
    top: 4px;
    left: 28px;
    display: grid;
    place-items: center;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--accent-grad);
    color: #fff;
    font-size: 9px;
    font-style: normal;
    font-weight: 700;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    box-shadow:
      0 0 0 2px var(--bg-1),
      0 0 8px var(--accent-glow);
    opacity: 0;
    transform: scale(0.6);
    transition:
      opacity var(--dur-med) var(--ease-out),
      transform var(--dur-med) var(--ease-out);
  }

  .collapsed .badge {
    opacity: 0;
  }

  .collapsed .dot {
    opacity: 1;
    transform: scale(1);
  }

  .label {
    white-space: nowrap;
    font-weight: 500;
    transition:
      opacity var(--dur-med) var(--ease-out),
      transform var(--dur-med) var(--ease-out);
  }

  .collapsed .label {
    opacity: 0;
    transform: translateX(-6px);
  }

  .divider {
    height: 1px;
    margin: 6px 8px;
    background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.08), transparent);
  }

  .tooltip {
    position: fixed;
    z-index: 50;
    transform: translateY(-50%);
    padding: 6px 10px;
    border: 1px solid rgb(255 255 255 / 0.1);
    border-radius: var(--radius-sm);
    background: rgb(32 37 54 / 0.97);
    box-shadow: var(--elev-1);
    font-size: 12.5px;
    white-space: nowrap;
    pointer-events: none;
    animation: tip-in var(--dur-fast) var(--ease-out);
  }

  @keyframes tip-in {
    from {
      opacity: 0;
      transform: translate(-4px, -50%);
    }
  }
</style>
