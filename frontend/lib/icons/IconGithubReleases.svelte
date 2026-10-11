<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls} locked>
  <path d="m12 2.5 8 4.5v10l-8 4.5L4 17V7l8-4.5Z" />
  <path d="M8 9.3v4.9M15 9.3c0 3.1-2.5 4.2-7 4.2" stroke-width="1.5" />
  <circle cx="8" cy="8" r="1.3" stroke-width="1.5" />
  <circle cx="8" cy="15.5" r="1.3" stroke-width="1.5" />
  <circle cx="15" cy="8" r="1.3" stroke-width="1.5" />
</IconCanvas>
