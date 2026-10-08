<script lang="ts">
  // The one progress bar of the app: a smooth fill with a light running along
  // it while work is going on, and a settled green fill when it is over.
  let {
    /** 0…1, or null while the total is not known: then it sweeps. */
    value,
    done = false,
    label,
    /** Drawn under the bar, if given. */
    note = "",
  }: { value: number | null; done?: boolean; label?: string; note?: string } = $props();

  const percent = $derived(value === null ? null : Math.round(Math.max(0, Math.min(1, value)) * 100));
</script>

<div
  class="track"
  class:done
  class:sweeping={percent === null}
  role="progressbar"
  aria-label={label}
  aria-valuemin={percent === null ? undefined : 0}
  aria-valuemax={percent === null ? undefined : 100}
  aria-valuenow={percent ?? undefined}
  aria-valuetext={percent === null ? "Working" : undefined}
>
  {#if percent !== null}
    <span class="fill" style:width={`${percent}%`}><span class="sheen" aria-hidden="true"></span></span>
  {:else}
    <span class="fill sweep"><span class="sheen" aria-hidden="true"></span></span>
  {/if}
</div>

{#if note}<p class="note">{note}</p>{/if}

<style>
  .track {
    position: relative;
    height: 8px;
    overflow: hidden;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.28);
    box-shadow: inset 0 1px 2px rgb(0 0 0 / 0.35);
  }

  .fill {
    position: relative;
    display: block;
    height: 100%;
    /* Early steps are a few percent: the sliver still reads as progress. */
    min-width: 10px;
    border-radius: inherit;
    background: var(--accent-grad);
    box-shadow: 0 0 10px -1px var(--accent-glow);
    transition: width 90ms linear;
  }

  /* A light that runs along the filled part, so a step that takes a while
     still looks alive. */
  .sheen {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.35), transparent);
    animation: sheen 1.25s var(--ease-in-out) infinite;
  }

  @keyframes sheen {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(100%);
    }
  }

  .fill.sweep {
    width: 34%;
    animation: sweep 1.3s var(--ease-in-out) infinite;
  }

  @keyframes sweep {
    from {
      transform: translateX(-110%);
    }
    to {
      transform: translateX(330%);
    }
  }

  .track.done .fill {
    background: linear-gradient(135deg, #6fd6a8 0%, #4fbf95 100%);
    box-shadow: 0 0 12px -1px rgb(62 207 142 / 0.5);
  }

  .track.done .sheen {
    animation: none;
    opacity: 0;
  }

  .note {
    margin-top: 6px;
    color: var(--text-3);
    font-size: 10.75px;
  }
</style>
