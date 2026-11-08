<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls} locked>
  <path d="M6.5 8.5h11a4 4 0 0 1 3.9 3.2l.6 3.4a2.5 2.5 0 0 1-4.4 2l-1.5-1.6H8l-1.5 1.6a2.5 2.5 0 0 1-4.4-2l.6-3.4a4 4 0 0 1 3.8-3.2Z" />
  <path d="M7.5 11v2.6M6.2 12.3h2.6" stroke-width="1.5" />
  <path d="M12 8.5V5.8M9.8 3.7 12 5.8l2.2-2.1" stroke-width="1.5" />
</IconCanvas>
