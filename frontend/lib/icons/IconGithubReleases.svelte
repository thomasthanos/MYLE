<script lang="ts">
  let { size = 20, class: cls = "" }: { size?: number; strokeWidth?: number; class?: string } = $props();
  const uid = $props.id();
</script>

<!-- GitHub Releases: a release tag on a git branch -->
<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  class="custom-nav-icon {cls}"
  aria-hidden="true"
>
  <defs>
    <linearGradient id="{uid}-fill" x1="9" y1="9" x2="21" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#C4B5FD" stop-opacity="0.3" />
      <stop offset="100%" stop-color="#34D399" stop-opacity="0.08" />
    </linearGradient>
    <linearGradient id="{uid}-stroke" x1="9" y1="9" x2="21" y2="21" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#EDE9FE" />
      <stop offset="55%" stop-color="#A78BFA" />
      <stop offset="100%" stop-color="#34D399" />
    </linearGradient>
  </defs>

  <!-- Branch -->
  <path d="M5.5 4.8V15.2M5.5 9.6C5.5 11.8 7.2 12.6 9.2 12.6" stroke="#C7D2FE" stroke-width="1.55" stroke-linecap="round" />
  <circle cx="5.5" cy="4.6" r="1.7" stroke="#C7D2FE" stroke-width="1.5" />
  <circle cx="5.5" cy="17.4" r="1.7" stroke="#C7D2FE" stroke-width="1.5" />

  <!-- Tag -->
  <path
    d="M10.6 10.4C10.6 9.8 11.1 9.3 11.7 9.3H15.6C16 9.3 16.3 9.4 16.6 9.7L20.4 13.5C20.9 14 20.9 14.7 20.4 15.2L16.7 18.9C16.2 19.4 15.5 19.4 15 18.9L11.2 15.1C10.9 14.8 10.8 14.5 10.8 14.1Z"
    fill="url(#{uid}-fill)"
    stroke="url(#{uid}-stroke)"
    stroke-width="1.6"
    stroke-linejoin="round"
  />
  <circle cx="13.6" cy="12.3" r="1.05" fill="#6EE7B7" />
</svg>
