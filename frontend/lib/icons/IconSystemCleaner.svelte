<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="m9 10 5 4-4.5 6.5-6-4.5L9 10Z" />
  <path d="m11.5 12 7.5-8.5M6.5 15l-1.2 1.7M9 16.5l-1.2 1.7M11.5 18l-1.2 1.7" />
  <path d="M19 10v4m-2-2h4" stroke-width="1.3" />
</IconCanvas>
