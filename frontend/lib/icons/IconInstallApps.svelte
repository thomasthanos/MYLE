<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="m12 3 8 4.5v9L12 21l-8-4.5v-9L12 3Z" />
  <path d="m4 7.5 8 4.5 8-4.5M12 12v9M8 5.25l8 4.5" />
</IconCanvas>
