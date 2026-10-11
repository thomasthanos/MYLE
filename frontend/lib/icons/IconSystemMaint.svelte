<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="m14.7 6.3 2.4 2.4 3.4-3.4a5.5 5.5 0 0 1-7.3 7.3l-7.4 7.4a2.1 2.1 0 0 1-3-3l7.4-7.4a5.5 5.5 0 0 1 7.3-7.3l-2.8 2.8v1.2Z" />
  <path d="m4 3 4 4M3 4l2-2M15 16l4 4a1.4 1.4 0 0 0 2-2l-4-4M4.5 18.5h.01" />
</IconCanvas>
