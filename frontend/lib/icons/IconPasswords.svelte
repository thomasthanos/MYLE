<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="M12 3c2.4 1.7 5 2.5 8 3v6c0 4.3-3.2 7.5-8 9-4.8-1.5-8-4.7-8-9V6c3-.5 5.6-1.3 8-3Z" />
  <rect x="8.5" y="10.5" width="7" height="6" rx="1.3" />
  <path d="M10 10.5V8.7a2 2 0 0 1 4 0v1.8M12 13v1.2" />
</IconCanvas>
