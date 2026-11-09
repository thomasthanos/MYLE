<script lang="ts">
  // The top of a feature: what is happening, what it does in plain words, and its controls.
  import type { Snippet } from "svelte";

  let { title, detail, active = false, children }: { title: string; detail: string; active?: boolean; children?: Snippet } = $props();
</script>

<section class="panel surface" class:active>
  <span class="light" aria-hidden="true"></span>
  <div class="words">
    <h2>{title}</h2>
    <p>{detail}</p>
  </div>
  {#if children}<div class="controls">{@render children()}</div>{/if}
</section>

<style>
  .panel { display: flex; flex-wrap: wrap; align-items: center; gap: 12px 14px; margin-bottom: 16px; padding: 13px 16px; }
  .light { flex: none; width: 10px; height: 10px; border-radius: 50%; background: var(--idle); }
  .active .light { background: var(--ok); box-shadow: 0 0 10px var(--ok); }
  .words { flex: 1 1 300px; min-width: 0; }
  h2 { margin: 0; font-size: 15px; font-weight: 700; }
  p { margin: 2px 0 0; color: var(--text-2); font-size: 12.5px; line-height: 1.4; }
  .controls { display: flex; flex-wrap: wrap; align-items: center; gap: 14px; }
</style>
