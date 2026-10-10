<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Install Apps: Isometric 3D Package Cube with glowing '+' badge -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-top" x1="3.5" y1="2.5" x2="20.5" y2="12" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#E0F2FE" stop-opacity="0.42" />
      <stop offset="100%" stop-color="#38BDF8" stop-opacity="0.18" />
    </linearGradient>
    <linearGradient id="{uid}-left" x1="3.5" y1="7" x2="12" y2="21.5" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.24" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="3" y1="2" x2="21" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#E0F2FE" />
      <stop offset="55%" stop-color="#7DD3FC" />
      <stop offset="100%" stop-color="#818CF8" />
    </linearGradient>
    <linearGradient id="{uid}-badge" x1="14" y1="14" x2="22" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#67E8F9" />
      <stop offset="100%" stop-color="#6366F1" />
    </linearGradient>
    <mask id="{uid}-cutout">
      <rect width="24" height="24" fill="white" />
      <circle cx="18.2" cy="18.2" r="5.1" fill="black" />
    </mask>
  </defs>

  <g mask="url(#{uid}-cutout)">
    <!-- Top & Left Shaded Faces -->
    <path d="M12 2.4L20.4 7.2L12 12L3.6 7.2L12 2.4Z" fill="url(#{uid}-top)" />
    <path d="M3.6 7.2L12 12V21.6L3.6 16.8V7.2Z" fill="url(#{uid}-left)" />

    <!-- Isometric Cube Wireframe -->
    <path
      d="M20.4 13.5V7.95C20.4 7.45 20.13 6.99 19.7 6.74L12.7 2.74C12.27 2.49 11.73 2.49 11.3 2.74L4.3 6.74C3.87 6.99 3.6 7.45 3.6 7.95V16.05C3.6 16.55 3.87 17.01 4.3 17.26L11.3 21.26C11.73 21.51 12.27 21.51 12.7 21.26L14 20.5"
      stroke="url(#{uid}-stroke)"
      stroke-width="1.65"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
    <!-- Inner Y-Edges & Package Tape -->
    <path
      d="M3.8 7.3L12 12L20.2 7.3M12 12V21.3M7.8 4.85L16.2 9.65"
      stroke="url(#{uid}-stroke)"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </g>

  <!-- Glowing '+' Badge -->
  <circle cx="18.2" cy="18.2" r="3.9" fill="url(#{uid}-badge)" />
  <path
    d="M18.2 16.35V20.05M16.35 18.2H20.05"
    stroke="#0B101E"
    stroke-width="1.75"
    stroke-linecap="round"
  />
</IconCanvas>
