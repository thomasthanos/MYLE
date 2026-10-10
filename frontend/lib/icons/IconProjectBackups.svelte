<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Project Backups: a project folder with code brackets, carried up to the cloud -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-fill" x1="2" y1="5" x2="20" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#7DD3FC" stop-opacity="0.26" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="2" y1="4" x2="21" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#E0F2FE" />
      <stop offset="55%" stop-color="#38BDF8" />
      <stop offset="100%" stop-color="#6366F1" />
    </linearGradient>
  </defs>

  <!-- Folder -->
  <path
    d="M2.8 7.4C2.8 6.2 3.8 5.2 5 5.2H8.6C9.2 5.2 9.7 5.5 10.1 5.9L11.2 7.1C11.5 7.4 11.9 7.6 12.4 7.6H18.6C19.8 7.6 20.8 8.6 20.8 9.8V16.8C20.8 18 19.8 19 18.6 19H5C3.8 19 2.8 18 2.8 16.8Z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.6"
    stroke-linejoin="round"
  />

  <!-- Code brackets -->
  <path
    d="M8.7 11.2L6.9 13.1L8.7 15M15 11.2L16.8 13.1L15 15"
    stroke="#E0E7FF"
    stroke-width="1.55"
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  <!-- Upload arrow -->
  <path d="M11.85 15.6V10.9M10.3 12.3L11.85 10.75L13.4 12.3" stroke="#5EEAD4" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
</IconCanvas>
