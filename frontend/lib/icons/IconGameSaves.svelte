<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Game Saves: Sculpted DualSense Gamepad with D-Pad & Action Gems -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-fill" x1="3" y1="5" x2="21" y2="20" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C4B5FD" stop-opacity="0.28" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="2" y1="5" x2="22" y2="20" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#EDE9FE" />
      <stop offset="50%" stop-color="#A78BFA" />
      <stop offset="100%" stop-color="#6366F1" />
    </linearGradient>
  </defs>

  <!-- Ergonomic Controller Silhouette -->
  <path
    d="M17.32 5H6.68C4.65 5 2.91 6.47 2.58 8.48L1.74 13.56C1.32 16.12 3.29 18.44 5.88 18.44C7.17 18.44 8.38 17.83 9.16 16.8L10.38 15.17C10.76 14.66 11.36 14.36 12 14.36C12.64 14.36 13.24 14.66 13.62 15.17L14.84 16.8C15.62 17.83 16.83 18.44 18.12 18.44C20.71 18.44 22.68 16.12 22.26 13.56L21.42 8.48C21.09 6.47 19.35 5 17.32 5Z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  <!-- Left D-Pad Cross -->
  <path
    d="M7.5 9.1V12.7M5.7 10.9H9.3"
    stroke="#E0E7FF"
    stroke-width="1.7"
    stroke-linecap="round"
  />

  <!-- Right Action Buttons -->
  <circle cx="15.4" cy="11.7" r="1.05" fill="#38BDF8" />
  <circle cx="17.7" cy="9.9" r="1.05" fill="#F472B6" />
</IconCanvas>
