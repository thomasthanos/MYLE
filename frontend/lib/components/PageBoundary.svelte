<script lang="ts">
  // Keeps one page's failure to that page: the sidebar, the other pages and the window carry on, and the page can be tried again.
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();

  const describe = (error: unknown) => (error instanceof Error ? error.message : typeof error === "string" ? error : "");

  function report(error: unknown) {
    console.error("A page failed:", error);
  }
</script>

<svelte:boundary onerror={report}>
  {@render children()}
  {#snippet failed(error, reset)}
    <div class="failed" role="alert">
      <strong>This page ran into a problem</strong>
      {#if describe(error)}<p>{describe(error)}</p>{/if}
      <button class="btn small" type="button" onclick={reset}>Try again</button>
    </div>
  {/snippet}
</svelte:boundary>

<style>
  .failed { display: grid; justify-items: center; gap: 8px; max-width: 460px; margin: 64px auto; color: var(--text-2); font-size: 13px; text-align: center; }
  strong { color: var(--text-1); font-size: 14.5px; }
  p { margin: 0; color: var(--text-3); font-size: 12px; overflow-wrap: anywhere; }
</style>
