<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Creative Suite: 3D Floating Isometric Layered Stack -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-top-fill" x1="3" y1="3" x2="21" y2="12" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#F5D0FE" stop-opacity="0.38" />
      <stop offset="55%" stop-color="#C084FC" stop-opacity="0.22" />
      <stop offset="100%" stop-color="#60A5FA" stop-opacity="0.12" />
    </linearGradient>
    <linearGradient id="{uid}-mid-fill" x1="3" y1="10" x2="21" y2="17" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C084FC" stop-opacity="0.2" />
      <stop offset="100%" stop-color="#38BDF8" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-top-stroke" x1="2.5" y1="2.5" x2="21.5" y2="12.5" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#FAE8FF" />
      <stop offset="55%" stop-color="#D8B4FE" />
      <stop offset="100%" stop-color="#93C5FD" />
    </linearGradient>
    <linearGradient id="{uid}-mid-stroke" x1="2.5" y1="12" x2="21.5" y2="17" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#E9D5FF" />
      <stop offset="50%" stop-color="#A855F7" />
      <stop offset="100%" stop-color="#60A5FA" />
    </linearGradient>
    <linearGradient id="{uid}-bot-stroke" x1="2.5" y1="16.5" x2="21.5" y2="21.5" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C084FC" />
      <stop offset="50%" stop-color="#818CF8" />
      <stop offset="100%" stop-color="#38BDF8" />
    </linearGradient>
  </defs>

  <!-- Middle Floating Layer Shaded Surface -->
  <path
    d="M12 12.25L21.25 12L12 16.75L2.75 12L12 12.25Z"
    fill="url(#{uid}-mid-fill)"
  />

  <!-- Bottom Floating Layer -->
  <path
    d="M2.75 16.5L12 21.25L21.25 16.5"
    stroke="url(#{uid}-bot-stroke)"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  <!-- Middle Floating Layer -->
  <path
    d="M2.75 12L12 16.75L21.25 12"
    stroke="url(#{uid}-mid-stroke)"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  <!-- Top Isometric Diamond Layer -->
  <path
    d="M12 2.75L21.25 7.5L12 12.25L2.75 7.5L12 2.75Z"
    fill="url(#{uid}-top-fill)"
    stroke="url(#{uid}-top-stroke)"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
</IconCanvas>
