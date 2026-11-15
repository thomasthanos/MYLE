<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <circle cx="12" cy="12" r="9" />
  <path d="M14.5 9.2a2.8 2.8 0 0 0-2.1 4.7l-2.4 2.4a1.8 1.8 0 0 0-1.7.7l-4.3-1.8" />
  <path d="m16.5 11.3-6.4 4.9" />
  <circle cx="14.5" cy="9.2" r="1.3" />
  <circle cx="8.3" cy="15.8" r="0.8" fill="currentColor" stroke="none" />
</IconCanvas>
