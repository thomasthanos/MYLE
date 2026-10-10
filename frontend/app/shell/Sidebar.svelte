<script lang="ts">
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import { badges } from "../../lib/badges.svelte";
  import { nav } from "../../lib/nav.svelte";
  import { GROUP_LABELS, nextFocus, sections, shortcutOf } from "../../lib/sidebar";
  import { account } from "../account/account.svelte";
  import { canOpen, pages, type PageDef } from "../pages/registry";

  const shown = $derived(pages.filter((p) => canOpen(p, account.owner)));
  const topSections = $derived(sections(shown));
  const bottomPages = $derived(shown.filter((p) => p.group === "bottom"));

  /** Shown after a pause while the labels are visible; at once when collapsed
   *  (the tooltip is then the only place the name is) or on keyboard focus. */
  const TIP_DELAY_MS = 550;
  const OWNER_NOTE = "Only visible to the owner account.";
  const COLLAPSE = "collapse";

  interface Tip { title: string; description?: string; shortcut?: string | null; owner?: boolean; x: number; y: number }

  // Rendered outside the sidebar panel, so nothing in it can clip the tooltip.
  let tip = $state<Tip | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;
  let sidebar = $state<HTMLElement>();
  let list = $state<HTMLElement>();
  /** Which ends of the page list have more pages scrolled out of view. */
  let more = $state({ above: false, below: false });

  function measure() {
    if (!list) return;
    const { scrollTop, scrollHeight, clientHeight } = list;
    more = { above: scrollTop > 1, below: scrollTop + clientHeight < scrollHeight - 1 };
  }

  $effect(() => {
    if (!list) return;
    const observer = new ResizeObserver(measure);
    observer.observe(list);
    for (const child of list.children) observer.observe(child);
    return () => observer.disconnect();
  });

  function place(target: HTMLElement): { x: number; y: number } {
    const item = target.getBoundingClientRect();
    const edge = sidebar?.getBoundingClientRect().right ?? item.right;
    return { x: edge + 10, y: item.top + item.height / 2 };
  }

  function showTip(target: HTMLElement, content: Omit<Tip, "x" | "y">, immediate: boolean) {
    clearTimeout(tipTimer);
    const open = () => (tip = { ...content, ...place(target) });
    if (immediate) open();
    else {
      tip = null;
      tipTimer = setTimeout(open, TIP_DELAY_MS);
    }
  }

  function hideTip() {
    clearTimeout(tipTimer);
    tip = null;
  }

  /** The page's shortcut; the toggle's is App.svelte's Ctrl+B. */
  function shortcutFor(page: PageDef): string | null {
    return page.id === COLLAPSE ? "Ctrl+B" : shortcutOf(shown, page.id);
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
      <span class="glyph"><page.icon size={22} strokeWidth={1.75} /></span>
      {#if badge}<i class="dot" aria-hidden="true">{badge > 9 ? "9+" : badge}</i>{/if}
      {#if page.ownerOnly}<i class="lock" aria-hidden="true"><ShieldCheck size={9} strokeWidth={2.5} /></i>{/if}
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
    {#each topSections as section, index (section.group)}
      {#if section.group === "owner"}
        <div class="owner-head" aria-hidden="true">
          <span class="rule"></span>
          <span class="owner-label"><ShieldCheck size={11} strokeWidth={2.25} /> Owner</span>
          <span class="rule"></span>
        </div>
      {:else if index > 0}
        <div class="divider" aria-hidden="true"></div>
      {/if}
      <div class="group" class:owner-group={section.group === "owner"} role="group" aria-label={GROUP_LABELS[section.group]}>
        {#each section.pages as page (page.id)}
          {@render item(page, nav.current === page.id, () => nav.go(page.id), badges.of(page.id))}
        {/each}
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
  <div class="tooltip" role="tooltip" style:left="{tip.x}px" style:top="{tip.y}px">
    <div class="tip-head">
      <strong>{tip.title}</strong>
      {#if tip.shortcut}<kbd>{tip.shortcut}</kbd>{/if}
    </div>
    {#if tip.description}<p>{tip.description}</p>{/if}
    {#if tip.owner}<p class="tip-owner"><ShieldCheck size={12} strokeWidth={2.25} /> {OWNER_NOTE}</p>{/if}
  </div>
{/if}

<style>
  .sidebar { --owner-rgb: 214 186 128; grid-area: side; display: flex; flex-direction: column; justify-content: space-between; gap: 6px; min-width: 0; min-height: 0; padding: 9px; border-radius: var(--shell-panel-radius); }
  :global(:root.dark) .sidebar { background: var(--sidebar-fill); }
  /* On a short window the page list scrolls; Settings and the toggle stay. */
  .top { min-height: 0; overflow-x: hidden; overflow-y: auto; --fade-top: 0px; --fade-bottom: 0px; mask-image: linear-gradient(to bottom, transparent 0, #000 var(--fade-top), #000 calc(100% - var(--fade-bottom)), transparent 100%); }
  .top.more-above { --fade-top: 22px; }
  .top.more-below { --fade-bottom: 22px; }
  .group { display: grid; gap: 2px; }
  .item { position: relative; display: flex; align-items: center; width: 100%; height: 40px; border-radius: var(--radius-md); color: var(--text-2); overflow: hidden; transition: background var(--dur-fast), color var(--dur-fast), scale 140ms var(--ease-out); }
  .item:hover { background: var(--hover); color: var(--text-1); }
  .item:active { scale: 0.96; background: var(--press); transition-duration: var(--dur-fast), var(--dur-fast), 50ms; }
  /* Inside the rounded item, so the ring is never cut by the panel's edge. */
  .item:focus-visible { outline: 2px solid rgb(var(--accent-rgb) / 0.75); outline-offset: -2px; }
  .item.active { background: var(--selected); color: var(--text-1); font-weight: 600; box-shadow: var(--btn-sheen); }
  /* Glowing accent pill on the active item. */
  .item.active::before { content: ""; position: absolute; left: 0; top: 50%; width: 3px; height: 18px; margin-top: -9px; border-radius: 0 3px 3px 0; background: var(--accent); }
  /* The owner's pages: the same items with a faint warm tint, so they read as "mine, not everyone's". */
  .item.owner:hover { background: rgb(var(--owner-rgb) / 0.07); }
  .item.owner.active { background: rgb(var(--owner-rgb) / 0.13); }
  .item.owner.active::before { background: rgb(var(--owner-rgb)); }
  .item.owner:focus-visible { outline-color: rgb(var(--owner-rgb) / 0.7); }
  /* The icon column is exactly as wide as the collapsed item, so icons never move. */
  .icon { position: relative; flex: none; display: grid; place-items: center; width: 48px; }
  .glyph { display: grid; place-items: center; width: 32px; height: 32px; }
  .glyph :global(.custom-nav-icon) { filter: drop-shadow(0 1px 1px rgb(0 0 0 / 0.35)); }
  /* A small shield on the icon's lower corner marks an owner-only page. */
  .lock { position: absolute; bottom: -2px; left: 29px; display: grid; place-items: center; width: 13px; height: 13px; border-radius: 999px; background: rgb(var(--owner-rgb) / 0.22); color: rgb(var(--owner-rgb)); box-shadow: 0 0 0 2px var(--bg-1); opacity: 0.85; }
  .item:hover .lock, .item.active .lock { opacity: 1; }
  /* Count on the right while expanded; on the icon's corner while collapsed. */
  .badge { flex: none; min-width: 19px; margin: 0 10px 0 auto; padding: 1px 6px; border-radius: 999px; background: var(--accent-grad); color: #fff; font-size: 10.5px; font-weight: 700; font-variant-numeric: tabular-nums; text-align: center; box-shadow: 0 0 10px var(--accent-glow); transition: opacity var(--dur-med) var(--ease-out); }
  /* The icon is 20px in 48 x 40: the count sits on its top-right corner, cut out by a ring in the sidebar's colour. */
  .dot { position: absolute; top: -2px; left: 30px; display: grid; place-items: center; min-width: 15px; height: 15px; padding: 0 4px; border-radius: 999px; background: var(--accent-grad); color: #fff; font-size: 9px; font-style: normal; font-weight: 700; line-height: 1; font-variant-numeric: tabular-nums; box-shadow: 0 0 0 2px var(--bg-1), 0 0 8px var(--accent-glow); opacity: 0; transform: scale(0.6); transition: opacity var(--dur-med) var(--ease-out), transform var(--dur-med) var(--ease-out); }
  .collapsed .badge { opacity: 0; }
  .collapsed .dot { opacity: 1; transform: scale(1); }
  .label { white-space: nowrap; font-weight: 500; transition: opacity var(--dur-med) var(--ease-out), transform var(--dur-med) var(--ease-out); }
  .item.active .label { font-weight: 600; }
  .collapsed .label { opacity: 0; transform: translateX(-6px); }
  .divider { height: 1px; margin: 6px 8px; background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.08), transparent); }
  /* "Owner" between two hairlines; only the hairline when collapsed. */
  .owner-head { display: flex; align-items: center; gap: 6px; height: 13px; margin: 6px 8px; }
  .owner-head .rule { flex: 1; height: 1px; background: linear-gradient(90deg, transparent, rgb(var(--owner-rgb) / 0.22)); }
  .owner-head .rule:last-child { background: linear-gradient(90deg, rgb(var(--owner-rgb) / 0.22), transparent); }
  .owner-label { display: inline-flex; align-items: center; gap: 4px; color: rgb(var(--owner-rgb) / 0.72); font-size: 10px; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; white-space: nowrap; transition: opacity var(--dur-med) var(--ease-out); }
  .collapsed .owner-label { display: none; }
  .tooltip { position: fixed; z-index: 50; max-width: 280px; transform: translateY(-50%); padding: 7px 10px 8px; border: 1px solid rgb(255 255 255 / 0.1); border-radius: var(--radius-sm); background: var(--tooltip-bg); box-shadow: var(--elev-1); font-size: 12.5px; pointer-events: none; animation: tip-in var(--dur-fast) var(--ease-out); }
  .tip-head { display: flex; align-items: center; justify-content: space-between; gap: 14px; white-space: nowrap; }
  .tip-head strong { color: var(--text-1); font-weight: 600; }
  .tooltip kbd { padding: 1px 5px; border: 1px solid rgb(255 255 255 / 0.12); border-radius: 5px; background: rgb(255 255 255 / 0.05); color: var(--text-2); font-family: var(--font-sans); font-size: 10.5px; }
  .tooltip p { margin: 3px 0 0; color: var(--text-2); line-height: 1.35; }
  .tooltip .tip-owner { display: flex; align-items: center; gap: 5px; color: rgb(var(--owner-rgb) / 0.9); }
  @keyframes tip-in { from { opacity: 0; transform: translate(-4px, -50%); } }
  @media (prefers-reduced-motion: reduce) { .tooltip { animation: none; } }
</style>
