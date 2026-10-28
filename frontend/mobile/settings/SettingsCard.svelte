<script lang="ts">
  import type { Component, Snippet } from "svelte";

  /* One group of settings: a glass card with its name and, in plain words, what the group is for. */
  let {
    icon: Icon,
    title,
    intro,
    children,
  }: {
    icon: Component<{ size?: number | string; strokeWidth?: number | string }>;
    title: string;
    intro?: string;
    children: Snippet;
  } = $props();

  const uid = $props.id();
</script>

<section class="card glass" aria-labelledby="{uid}-title">
  <header>
    <span class="mark" aria-hidden="true"><Icon size={17} /></span>
    <h3 id="{uid}-title">{title}</h3>
  </header>
  {#if intro}<p class="intro">{intro}</p>{/if}
  {@render children()}
</section>

<style>
  .card { display: grid; gap: 12px; padding: 16px; }

  header { display: flex; align-items: center; gap: 10px; min-width: 0; }

  .mark { display: grid; flex: none; place-items: center; width: 32px; height: 32px; border: 1px solid rgb(var(--accent-rgb) / 0.3); border-radius: 10px; background: rgb(var(--accent-rgb) / 0.14); color: rgb(var(--accent-soft-rgb)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08); }

  h3 { margin: 0; font-family: var(--font-display); font-size: 15.5px; font-weight: 650; overflow-wrap: anywhere; }

  .intro { margin: 0; color: var(--text-2); font-size: 13px; line-height: 1.45; }
</style>
