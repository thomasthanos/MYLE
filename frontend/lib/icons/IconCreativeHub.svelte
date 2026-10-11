<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="M12 3a9 9 0 1 0 0 18h1.4a2.1 2.1 0 0 0 1.5-3.6 1.8 1.8 0 0 1 1.3-3.1H18a3 3 0 0 0 3-3A8.7 8.7 0 0 0 12 3Z" />
  <circle cx="7.5" cy="10" r=".75" fill="currentColor" stroke="none" />
  <circle cx="11" cy="6.5" r=".75" fill="currentColor" stroke="none" />
  <circle cx="15.5" cy="7.5" r=".75" fill="currentColor" stroke="none" />
  <circle cx="7.5" cy="14.5" r=".75" fill="currentColor" stroke="none" />
</IconCanvas>
