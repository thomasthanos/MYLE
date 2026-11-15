<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls} locked>
  <circle cx="12" cy="12" r="7.8" />
  <path d="M12 2.2v2M12 19.8v2M2.2 12h2M19.8 12h2M5.1 5.1l1.4 1.4M17.5 17.5l1.4 1.4M5.1 18.9l1.4-1.4M17.5 6.5l1.4-1.4" stroke-width="1.6" />
  <path d="M14.2 9.4a2.6 2.6 0 0 0-1.9 4.3l-2.2 2.2a1.6 1.6 0 0 0-1.5.6l-3.8-1.6" />
  <path d="m16 11.3-5.8 4.4" />
  <circle cx="14.2" cy="9.4" r="1.1" />
  <circle cx="8.6" cy="15.3" r="0.75" fill="currentColor" stroke="none" />
</IconCanvas>
