<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Password Manager: a vault key with a glowing shield bow -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-fill" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C4B5FD" stop-opacity="0.32" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#EDE9FE" />
      <stop offset="50%" stop-color="#A5B4FC" />
      <stop offset="100%" stop-color="#818CF8" />
    </linearGradient>
  </defs>

  <!-- Shield-shaped bow -->
  <path
    d="M9 2.75 13.75 4.4v3.55c0 3.05-1.95 5.2-4.75 6.3-2.8-1.1-4.75-3.25-4.75-6.3V4.4Z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.6"
    stroke-linejoin="round"
  />
  <!-- Keyhole -->
  <circle cx="9" cy="7.4" r="1.25" fill="#EDE9FE" />
  <path d="M9 8.5v2.1" stroke="#EDE9FE" stroke-width="1.4" stroke-linecap="round" />

  <!-- Key blade and teeth -->
  <path
    d="M12.6 12.6 20.25 20.25M16.9 16.9l1.9-1.9M18.9 18.9l1.35-1.35"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.65"
    stroke-linecap="round"
  />
</IconCanvas>
