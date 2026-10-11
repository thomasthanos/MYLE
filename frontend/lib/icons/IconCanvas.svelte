<script lang="ts">
  import type { Snippet } from "svelte";

  let { size = 20, strokeWidth = 1.75, class: cls = "", locked = false, children }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
    locked?: boolean;
    children: Snippet;
  } = $props();
  const uid = $props.id();
</script>

<!-- Silver glass: a lit rim, translucent face and shallow extrusion. -->
<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="url(#{uid}-rim)"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  class="custom-nav-icon {cls}"
  aria-hidden="true"
  focusable="false"
>
  <defs>
    {#if locked}
      <mask id="{uid}-corner" maskUnits="userSpaceOnUse" x="0" y="0" width="24" height="24">
        <rect width="24" height="24" fill="white" stroke="none" />
        <circle cx="19.25" cy="19" r="5.3" fill="black" stroke="none" />
      </mask>
    {/if}
    <linearGradient id="{uid}-rim" x1="4" y1="2" x2="18" y2="23" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#ffffff" />
      <stop offset=".38" stop-color="#d7dfe9" />
      <stop offset=".72" stop-color="#aab6c7" />
      <stop offset="1" stop-color="#758297" />
    </linearGradient>
    <linearGradient id="{uid}-glass" x1="5" y1="3" x2="18" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#eef3fa" stop-opacity=".38" />
      <stop offset=".38" stop-color="#cbd2dd" stop-opacity=".18" />
      <stop offset=".42" stop-color="#ffffff" stop-opacity=".32" />
      <stop offset=".46" stop-color="#cbd2dd" stop-opacity=".06" />
      <stop offset="1" stop-color="#78869b" stop-opacity=".28" />
    </linearGradient>
  </defs>
  <g transform="translate(0 .75)" stroke="#5c687b" opacity=".55" mask={locked ? `url(#${uid}-corner)` : undefined}>
    {@render children()}
  </g>
  <g class="face" style:--icon-glass-fill={`url(#${uid}-glass)`} mask={locked ? `url(#${uid}-corner)` : undefined}>
    {@render children()}
  </g>
  {#if locked}
    <g stroke-width="1.4">
      <path d="M17.5 17v-1.5a1.75 1.75 0 0 1 3.5 0V17" />
      <rect x="16" y="17" width="6.5" height="5" rx="1" fill="url(#{uid}-glass)" />
      <circle cx="19.25" cy="19.5" r=".55" fill="currentColor" stroke="none" />
    </g>
  {/if}
</svg>

<style>
  svg { display: block; overflow: visible; color: var(--icon-neutral, #cbd2dd); filter: drop-shadow(0 1px 1px rgb(0 0 0 / .5)); }
  .face > :global(:first-child) { fill: var(--icon-glass-fill); }
</style>
