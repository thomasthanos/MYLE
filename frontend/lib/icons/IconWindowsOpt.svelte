<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Windows Optimization: Precision Performance Tachometer Gauge -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-fill" x1="3" y1="4" x2="21" y2="20" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#38BDF8" stop-opacity="0.24" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.06" />
    </linearGradient>
    <linearGradient id="{uid}-arc" x1="2.5" y1="19" x2="21.5" y2="6" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#93C5FD" />
      <stop offset="55%" stop-color="#38BDF8" />
      <stop offset="100%" stop-color="#67E8F9" />
    </linearGradient>
    <linearGradient id="{uid}-needle" x1="12" y1="15" x2="17" y2="8" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#A5B4FC" />
      <stop offset="100%" stop-color="#FDE047" />
    </linearGradient>
  </defs>

  <!-- Soft Dial Backdrop -->
  <path
    d="M4.93 19.07A10 10 0 1 1 19.07 19.07L16.24 16.24A6 6 0 1 0 7.76 16.24L4.93 19.07Z"
    fill="url(#{uid}-fill)"
  />

  <!-- Outer Speedometer Arc -->
  <path
    d="M4.93 19.07A10 10 0 1 1 19.07 19.07"
    stroke="url(#{uid}-arc)"
    stroke-width="1.75"
    stroke-linecap="round"
  />

  <!-- Inner Scale Ticks -->
  <path
    d="M6 13H7.3M12 5.5V6.8M18 13H16.7M7.76 8.76L8.7 9.7"
    stroke="#BAE6FD"
    stroke-opacity="0.65"
    stroke-width="1.4"
    stroke-linecap="round"
  />

  <!-- High-RPM Boost Needle & Hub -->
  <path
    d="M12 15L16.35 8.85"
    stroke="url(#{uid}-needle)"
    stroke-width="1.85"
    stroke-linecap="round"
  />
  <circle cx="12" cy="15" r="2.1" fill="#E0F2FE" />
</IconCanvas>
