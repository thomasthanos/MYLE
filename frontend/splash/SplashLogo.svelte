<script module lang="ts">
  export type SplashPhase = "checking" | "downloading" | "verifying" | "installing" | "restarting" | "starting" | "error";
</script>

<script lang="ts">
  import CoderScene from "./CoderScene.svelte";

  let { phase }: { phase: SplashPhase } = $props();
  const opening = $derived(phase === "restarting" || phase === "starting");
</script>

<div class="visual" data-phase={phase} class:opening aria-hidden="true">
  <div class="grid"></div>
  <div class="orbital">
    <div class="halo"></div>
    <div class="rim"></div>
    <div class="orbit"></div>
    <div class="mark">
      <CoderScene size={130} paused={opening || phase === "error"} />
    </div>
    {#if phase === "verifying"}<div class="scan"></div>{/if}
    {#if phase === "installing" || opening}<div class="pulse"></div>{/if}
  </div>
</div>

<style>
  .visual {
    position: relative;
    display: grid;
    place-items: center;
    min-height: 0;
    isolation: isolate;
  }

  .grid {
    position: absolute;
    inset: 0 -6px;
    background:
      linear-gradient(rgb(148 177 236 / 0.12) 1px, transparent 1px) center / 16px 16px,
      linear-gradient(90deg, rgb(148 177 236 / 0.12) 1px, transparent 1px) center / 16px 16px;
    mask-image: radial-gradient(ellipse at center, black 20%, transparent 74%);
  }

  .orbital {
    position: relative;
    display: grid;
    place-items: center;
    width: 164px;
    height: 164px;
  }

  .halo,
  .rim,
  .orbit,
  .pulse {
    position: absolute;
    border-radius: 50%;
  }

  .halo {
    inset: -24px;
    background: radial-gradient(closest-side, color-mix(in srgb, var(--signal) 24%, transparent), transparent);
    opacity: 0.75;
  }

  .rim {
    inset: -8px;
    border: 1px dashed color-mix(in srgb, var(--signal) 26%, transparent);
  }

  .orbit {
    inset: 0;
    border: 1px solid color-mix(in srgb, var(--signal) 24%, transparent);
    transform: rotate(-32deg);
  }

  .orbit::before {
    position: absolute;
    inset: -1px;
    border: 1px solid transparent;
    border-top-color: var(--signal);
    border-radius: inherit;
    content: "";
  }

  .orbit::after {
    position: absolute;
    top: 11.5%;
    right: 11.5%;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--signal);
    box-shadow: 0 0 10px var(--signal);
    content: "";
  }

  .mark {
    position: relative;
    filter: drop-shadow(0 12px 20px rgb(0 0 0 / 0.48));
  }

  .scan {
    position: absolute;
    top: 50%;
    width: 88%;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--signal), transparent);
    box-shadow: 0 0 15px color-mix(in srgb, var(--signal) 75%, transparent);
  }

  .pulse {
    inset: 0;
    border: 1px solid var(--signal);
    opacity: 0.25;
  }

  .opening .orbit { border-color: color-mix(in srgb, var(--signal) 56%, transparent); }
  .opening .orbit::before,
  .opening .orbit::after,
  [data-phase="error"] .orbit::before,
  [data-phase="error"] .orbit::after { opacity: 0; }

  @media (max-height: 340px) {
    .orbital { scale: 0.76; }
  }

  @media (prefers-reduced-motion: no-preference) {
    .orbital { animation: enter 540ms var(--ease-out) both; }
    .halo { animation: illuminate 560ms var(--ease-out) both; }

    [data-phase="checking"] .orbit { animation: orbit 4s linear infinite; }
    [data-phase="downloading"] .orbit { animation: orbit 2.2s linear infinite; }
    [data-phase="verifying"] .scan { animation: scan 840ms var(--ease-in-out) infinite; }
    [data-phase="installing"] .pulse { animation: ripple 1.65s var(--ease-out) infinite; }
    .opening .pulse { animation: ripple 680ms var(--ease-out) both; }
    .opening .mark { animation: ready 440ms var(--ease-out) both; }
  }

  @keyframes enter {
    from { opacity: 0.58; transform: scale(0.9) rotate(-7deg); }
  }

  @keyframes illuminate {
    from { opacity: 0.25; transform: scale(0.75); }
  }

  @keyframes orbit {
    to { transform: rotate(328deg); }
  }

  @keyframes scan {
    0% { opacity: 0; transform: translateY(-58px); }
    24%, 76% { opacity: 0.9; }
    100% { opacity: 0; transform: translateY(58px); }
  }

  @keyframes ripple {
    from { opacity: 0.75; transform: scale(0.9); }
    to { opacity: 0; transform: scale(1.24); }
  }

  @keyframes ready {
    50% { transform: translateY(-4px) scale(1.05); }
  }
</style>
