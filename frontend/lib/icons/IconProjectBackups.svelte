<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls} locked>
  <path d="M20 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l3 3h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2Z" />
  <path d="M11 17v-7m-3 3 3-3 3 3" />
</IconCanvas>
