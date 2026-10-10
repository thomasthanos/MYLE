<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Spotify Hub: Acoustic Wave Emblem in Emerald & Mint Gradient -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-disc" x1="3" y1="2.5" x2="21" y2="21.5" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#34D399" stop-opacity="0.26" />
      <stop offset="100%" stop-color="#10B981" stop-opacity="0.06" />
    </linearGradient>
    <linearGradient id="{uid}-ring" x1="3" y1="2" x2="21" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#A7F3D0" />
      <stop offset="50%" stop-color="#34D399" />
      <stop offset="100%" stop-color="#10B981" />
    </linearGradient>
    <linearGradient id="{uid}-waves" x1="6" y1="8" x2="18" y2="17" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#ECFDF5" />
      <stop offset="100%" stop-color="#34D399" />
    </linearGradient>
  </defs>

  <!-- Outer Glass Disc -->
  <circle cx="12" cy="12" r="9.5" fill="url(#{uid}-disc)" stroke="url(#{uid}-ring)" stroke-width="1.65" />

  <!-- Three Precision Curved Acoustic Waves -->
  <path
    d="M7.1 9.65C10.5 8.6 14.3 8.85 17.35 10.6"
    stroke="url(#{uid}-waves)"
    stroke-width="1.85"
    stroke-linecap="round"
  />
  <path
    d="M7.95 12.65C10.7 11.8 13.75 12.02 16.2 13.45"
    stroke="url(#{uid}-waves)"
    stroke-width="1.65"
    stroke-linecap="round"
  />
  <path
    d="M8.85 15.45C11 14.8 13.35 14.98 15.15 16.05"
    stroke="url(#{uid}-waves)"
    stroke-width="1.45"
    stroke-linecap="round"
  />
</IconCanvas>
