<script lang="ts">
  import IconCanvas from "./IconCanvas.svelte";
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- Password Manager: Sculpted Security Shield with glowing Vault Lock -->
<IconCanvas {size} class={cls}>
  <defs>
    <linearGradient id="{uid}-fill" x1="4" y1="2" x2="20" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C4B5FD" stop-opacity="0.28" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="3" y1="2" x2="21" y2="22" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#EDE9FE" />
      <stop offset="50%" stop-color="#A78BFA" />
      <stop offset="100%" stop-color="#6366F1" />
    </linearGradient>
    <linearGradient id="{uid}-lock" x1="8" y1="5" x2="16" y2="17" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#FFFFFF" />
      <stop offset="100%" stop-color="#C4B5FD" />
    </linearGradient>
    <linearGradient id="{uid}-body" x1="8" y1="10" x2="16" y2="17" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#A78BFA" stop-opacity="0.45" />
      <stop offset="100%" stop-color="#6366F1" stop-opacity="0.2" />
    </linearGradient>
  </defs>

  <!-- Solid Sculpted Shield Silhouette -->
  <path
    d="M12 2.2L19.6 5.4C19.6 11.6 16.4 16.8 12 21.8C7.6 16.8 4.4 11.6 4.4 5.4L12 2.2Z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.65"
    stroke-linejoin="round"
  />

  <!-- Vault Lock Shackle -->
  <path
    d="M9.4 10.2V7.8C9.4 6.36 10.56 5.2 12 5.2C13.44 5.2 14.6 6.36 14.6 7.8V10.2"
    stroke="url(#{uid}-lock)"
    stroke-width="1.5"
    stroke-linecap="round"
  />

  <!-- Vault Lock Solid Core -->
  <rect
    x="8"
    y="10.2"
    width="8"
    height="6.4"
    rx="1.6"
    fill="url(#{uid}-body)"
    stroke="url(#{uid}-lock)"
    stroke-width="1.35"
  />

  <!-- Illuminated Keyhole Dot & Bar -->
  <circle cx="12" cy="12.7" r="0.9" fill="#EDE9FE" />
  <path d="M12 13.6V15" stroke="#EDE9FE" stroke-width="1.2" stroke-linecap="round" />
</IconCanvas>
