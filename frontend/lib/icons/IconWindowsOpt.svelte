<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls}>
  <path d="M4.93 19.07A10 10 0 1 1 19.07 19.07l-2.83-2.83a6 6 0 1 0-8.48 0l-2.83 2.83Z" stroke="none" />
  <path d="M5 19a9 9 0 1 1 14 0M12 4v2M4 12h2M18 12h2M6.3 6.3l1.4 1.4M12 14l4-5" />
  <circle cx="12" cy="14" r="1.5" />
</IconCanvas>
