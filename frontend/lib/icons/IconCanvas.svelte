<script lang="ts">
  import type { Snippet } from "svelte";

  let { size = 20, class: cls = "", children }: {
    size?: number;
    class?: string;
    children: Snippet;
  } = $props();
</script>

<!-- Shared canvas; each icon keeps its own colors, lighting and geometry. -->
<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  class="custom-nav-icon {cls}"
  aria-hidden="true"
  focusable="false"
>
  {@render children()}
</svg>
