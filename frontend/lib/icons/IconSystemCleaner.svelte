<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- System Cleaner: 3-Star Sparkle Constellation (matching Sparkle.svg) -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-main" x1="10.5" y1="3" x2="10.5" y2="20" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#E8FBFF" />
      <stop offset="50%" stop-color="#38BDF8" />
      <stop offset="100%" stop-color="#0284C7" />
    </linearGradient>
    <linearGradient id="{uid}-fill" x1="3" y1="3" x2="18" y2="20" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#67E8F9" stop-opacity="0.32" />
      <stop offset="100%" stop-color="#0284C7" stop-opacity="0.12" />
    </linearGradient>
    <linearGradient id="{uid}-accent" x1="18.5" y1="2" x2="18.5" y2="9" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#A7F3D0" />
      <stop offset="100%" stop-color="#22D3EE" />
    </linearGradient>
  </defs>

  <!-- Primary 4-Point Sparkle -->
  <path
    d="M10.5 3.2C10.95 7.9 13.35 10.3 18.05 10.75C13.35 11.2 10.95 13.6 10.5 18.3C10.05 13.6 7.65 11.2 2.95 10.75C7.65 10.3 10.05 7.9 10.5 3.2Z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-main)"
    stroke-width="1.65"
    stroke-linejoin="round"
  />

  <!-- Top-Right Companion Sparkle -->
  <path
    d="M18.5 2.2C18.75 4.4 19.6 5.25 21.8 5.5C19.6 5.75 18.75 6.6 18.5 8.8C18.25 6.6 17.4 5.75 15.2 5.5C17.4 5.25 18.25 4.4 18.5 2.2Z"
    fill="url(#{uid}-accent)"
  />

  <!-- Bottom-Right Mini Sparkle -->
  <path
    d="M17.5 15.8C17.68 17.3 18.3 17.92 19.8 18.1C18.3 18.28 17.68 18.9 17.5 20.4C17.32 18.9 16.7 18.28 15.2 18.1C16.7 17.92 17.32 17.3 17.5 15.8Z"
    fill="#A5F3FC"
  />
</IconCanvas>
