<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="M7 6h10a4 4 0 0 1 4 3.4l.8 5a3.3 3.3 0 0 1-5.6 2.9L14 15h-4l-2.2 2.3a3.3 3.3 0 0 1-5.6-2.9l.8-5A4 4 0 0 1 7 6Z" />
  <path d="M7.5 9v4M5.5 11h4" />
  <circle cx="15" cy="12" r=".8" fill="currentColor" stroke="none" />
  <circle cx="18" cy="10" r=".8" fill="currentColor" stroke="none" />
</IconCanvas>
