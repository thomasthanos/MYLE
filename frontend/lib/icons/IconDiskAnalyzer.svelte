<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<!-- A treemap: one big block, then smaller ones beside it. -->
<IconCanvas {size} {strokeWidth} class={cls}>
  <rect x="3.5" y="4" width="17" height="16" rx="2.5" />
  <path d="M11.5 4v16M11.5 12.5h9M16.5 12.5V20M16.5 16.25h4" />
</IconCanvas>
