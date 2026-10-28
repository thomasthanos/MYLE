<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
</script>
<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="m5 3 2 15 4-4 3.5 6 3-1.8-3.5-6 5-1L5 3Z" />
  <path d="M16 3v3m4-1-2 2M3 20l1.5-1.5" />
</IconCanvas>
