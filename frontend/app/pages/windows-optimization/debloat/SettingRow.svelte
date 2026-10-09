<script lang="ts">
  // One choice on one line: its switch (or checkbox), its name and short
  // tags. What it does, and what to know first, show on hover over the
  // arrow, or under the line once the arrow is clicked.
  import type { Snippet } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Star from "@lucide/svelte/icons/star";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";

  let {
    title,
    subtitle = "",
    keep = null,
    summary = "",
    note = null,
    checked,
    pending = false,
    disabled = false,
    control = "switch",
    recommended = false,
    caution = false,
    restart = false,
    partial = false,
    onchange,
    aside,
  }: {
    title: string;
    /** A few words after the name, on the same line (what an app is). */
    subtitle?: string;
    /** Why someone might keep it: a "Keep?" tag, explained on hover and under the line. */
    keep?: string | null;
    /** What it does. */
    summary?: string;
    /** What to know first: what stops working or works differently. */
    note?: string | null;
    checked: boolean;
    /** Chosen, and not how things are yet. */
    pending?: boolean;
    disabled?: boolean;
    control?: "switch" | "check";
    recommended?: boolean;
    caution?: boolean;
    restart?: boolean;
    partial?: boolean;
    onchange: (checked: boolean) => void;
    /** Anything else on the line, before the arrow (a button). */
    aside?: Snippet;
  } = $props();

  const id = $props.id();
  let open = $state(false);
  const hint = $derived([summary, note ?? keep].filter(Boolean).join("\n\n"));
</script>

<div class="setting" class:pending class:open>
  <label class="main">
    <input
      type="checkbox"
      class={control}
      role={control === "switch" ? "switch" : undefined}
      {checked}
      {disabled}
      aria-describedby={hint ? `${id}-about` : undefined}
      onchange={(event) => {
        const want = event.currentTarget.checked;
        // The state decides; the box only asks.
        event.currentTarget.checked = checked;
        onchange(want);
      }}
    />
    <span class="label">
      <span class="title">{title}{#if subtitle}<small>{subtitle}</small>{/if}</span>
      {#if summary && !open}<span class="summary">{summary}</span>{/if}
    </span>
  </label>
  <span class="tags">
    {#if pending}<span class="tag pending">Pending</span>{/if}
    {#if keep}<span class="tag keep" title={keep}>Keep?</span>{/if}
    {#if partial}<span class="tag partial" title="Part of it is in place already.">Partly</span>{/if}
    {#if recommended}<span class="tag star" title="Recommended for most people."><Star size={10} /></span>{/if}
    {#if caution}<span class="tag caution" title="Read what it does before you turn it on."><TriangleAlert size={11} /> Read first</span>{/if}
    {#if restart}<span class="tag restart" title="Takes full effect after Windows restarts."><RotateCcw size={10} /></span>{/if}
  </span>
  {@render aside?.()}
  {#if hint}
    <button
      type="button"
      class="more"
      title={hint}
      aria-label={open ? "Hide what it does" : "What it does"}
      aria-expanded={open}
      aria-controls="{id}-about"
      onclick={() => (open = !open)}
    >
      <ChevronRight size={14} />
    </button>
  {/if}
  {#if hint}
    <div class="about" id="{id}-about" hidden={!open}>
      {#if summary}<p>{summary}</p>{/if}
      {#if note ?? keep}<p class="note"><TriangleAlert size={11} />{note ?? keep}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .setting {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto auto;
    align-items: center;
    gap: 6px;
    min-height: 36px;
    padding: 3px 4px 3px 8px;
    border-radius: 8px;
    transition: background var(--dur-fast);
  }
  .setting:hover { background: rgb(255 255 255 / 0.03); }
  .setting.pending { background: rgb(var(--accent-rgb) / 0.08); }
  .setting.open { background: rgb(255 255 255 / 0.035); }

  .main { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 4px 0; cursor: pointer; }
  .main:has(input:disabled) { cursor: default; }
  .label { display: grid; gap: 1px; min-width: 0; }
  .summary { overflow: hidden; color: var(--text-3); font-size: 11px; line-height: 1.4; text-overflow: ellipsis; white-space: nowrap; }
  .title { overflow: hidden; color: var(--text-1); font-size: 12.5px; font-weight: 520; line-height: 1.35; text-overflow: ellipsis; white-space: nowrap; }
  .title small { margin-left: 7px; color: var(--text-3); font-size: 11.5px; font-weight: 400; }

  .tags { display: flex; align-items: center; gap: 4px; }
  .tag { display: inline-flex; align-items: center; gap: 3px; height: 18px; padding: 0 5px; border-radius: 5px; font-size: 10px; font-weight: 600; white-space: nowrap; }
  .tag.pending { background: rgb(var(--accent-rgb) / 0.18); color: rgb(var(--accent-soft-rgb)); }
  .tag.partial { background: rgb(245 188 95 / 0.09); color: #efc38a; }
  .tag.keep { background: rgb(245 188 95 / 0.09); color: #efc38a; cursor: help; }
  .tag.star { padding: 0 4px; color: rgb(var(--accent-soft-rgb)); }
  .tag.caution { padding: 0 5px; background: rgb(245 188 95 / 0.09); color: #efc38a; }
  .tag.restart { padding: 0 4px; color: var(--text-3); }

  .more { display: grid; place-items: center; width: 24px; height: 24px; border-radius: 6px; color: var(--text-3); }
  .more:hover { background: var(--hover); color: var(--text-1); }
  .more :global(svg) { transition: transform var(--dur-fast) var(--ease-out); }
  .open .more :global(svg) { transform: rotate(90deg); }
  .more:focus-visible { outline: 2px solid rgb(var(--accent-rgb) / 0.75); outline-offset: 1px; }

  .about { grid-column: 1 / -1; display: grid; gap: 4px; padding: 0 6px 6px 50px; }
  .about[hidden] { display: none; }
  .about p { color: var(--text-2); font-size: 11.5px; line-height: 1.5; }
  .about .note { display: flex; align-items: flex-start; gap: 6px; color: #e6c48f; }
  .about .note :global(svg) { flex: none; margin-top: 3px; }
  .setting:has(.check) .about { padding-left: 34px; }

  @media (prefers-reduced-motion: reduce) {
    .setting, .more :global(svg) { transition: none; }
  }
</style>
