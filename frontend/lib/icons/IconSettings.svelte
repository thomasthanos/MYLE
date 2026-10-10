<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Settings: Precision Mechanical Cogwheel with Glowing Core -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-fill" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C7D2FE" stop-opacity="0.24" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.06" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="3" y1="2" x2="21" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#E0E7FF" />
      <stop offset="55%" stop-color="#A5B4FC" />
      <stop offset="100%" stop-color="#6366F1" />
    </linearGradient>
    <linearGradient id="{uid}-core" x1="9" y1="9" x2="15" y2="15" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#67E8F9" />
      <stop offset="100%" stop-color="#818CF8" />
    </linearGradient>
  </defs>

  <!-- Smooth Circular-Arc 6-Lobe Gear Outer Path -->
  <path
    d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
  />

  <!-- Inner Core Ring -->
  <circle
    cx="12"
    cy="12"
    r="3"
    fill="#0E1220"
    stroke="url(#{uid}-core)"
    stroke-width="1.75"
  />
</IconCanvas>
