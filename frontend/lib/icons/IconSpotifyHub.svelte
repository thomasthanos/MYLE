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
  <path d="M7 9.5c3.4-1.1 7.1-.8 10 1M8 12.5c2.7-.8 5.5-.6 8 1M9 15.5c2-.6 4-.4 6 .6" />
</IconCanvas>
