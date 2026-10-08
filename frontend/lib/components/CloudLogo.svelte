<script lang="ts">
  /** The cloud apps whose folder MYLE can back up to (Game Saves, Project Backups). */
  type CloudProvider = "dropbox" | "googleDrive" | "mega" | "oneDrive";

  let { provider, size = 18 }: { provider: CloudProvider; size?: number } = $props();
</script>

<!-- Simplified marks, drawn to read at chip size. -->
<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  {#if provider === "dropbox"}
    <g fill="#0061ff">
      <path d="M7 3.2 2 6.4l5 3.2 5-3.2z" />
      <path d="M17 3.2 12 6.4l5 3.2 5-3.2z" />
      <path d="M2 12.8 7 16l5-3.2-5-3.2z" />
      <path d="M17 9.6 12 12.8l5 3.2 5-3.2z" />
      <path d="M7 17.1 12 20.3l5-3.2-5-3.2z" />
    </g>
  {:else if provider === "googleDrive"}
    <path d="M8.6 3h6.8l6.6 11.4h-6.8z" fill="#ffc107" />
    <path d="M8.6 3 2 14.4l3.4 5.9L12 8.9z" fill="#1fa463" />
    <path d="M5.4 20.3h13.2l3.4-5.9H8.8z" fill="#4285f4" />
  {:else if provider === "mega"}
    <circle cx="12" cy="12" r="10" fill="#d9272e" />
    <path
      d="M7.3 16.2V8.4c0-.3.4-.5.6-.2l4.1 4.1 4.1-4.1c.2-.3.6-.1.6.2v7.8h-2.1v-4.3l-2.3 2.3a.4.4 0 0 1-.6 0l-2.3-2.3v4.3z"
      fill="#fff"
    />
  {:else}
    <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9z" fill="#0a78d4" />
  {/if}
</svg>

<style>
  svg {
    flex: none;
    display: block;
  }
</style>
