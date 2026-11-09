<script lang="ts">
  // Starts or stops one feature for now.
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import type { Feature } from "./api";
  import { steamAutomationState as page } from "./state.svelte";

  let { feature, disabled = false, hint = "" }: { feature: Feature; disabled?: boolean; hint?: string } = $props();

  const started = $derived(page.status?.started[feature] ?? false);
</script>

<button class="btn" class:primary={!started} type="button" disabled={disabled && !started} title={disabled && !started ? hint : undefined} onclick={() => void page.setStarted(feature, !started)}>
  {#if started}<Square size={13} />Stop{:else}<Play size={13} />Start{/if}
</button>
