<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, strokeWidth = 1.75, class: cls = "" }: {
    size?: number;
    strokeWidth?: number;
    class?: string;
  } = $props();
</script>

<IconCanvas {size} {strokeWidth} class={cls} locked>
  <path d="M6.5 7.5h11a4 4 0 0 1 3.9 3.2l.8 4.4a2.6 2.6 0 0 1-4.6 2.1l-1.7-1.7H8.1l-1.7 1.7a2.6 2.6 0 0 1-4.6-2.1l.8-4.4a4 4 0 0 1 3.9-3.2Z" />
  <path d="M7.5 10.2v3M6 11.7h3" stroke-width="1.5" />
  <circle cx="15.3" cy="10.8" r="0.9" stroke-width="1.5" />
  <circle cx="17.5" cy="13" r="0.9" stroke-width="1.5" />
</IconCanvas>
