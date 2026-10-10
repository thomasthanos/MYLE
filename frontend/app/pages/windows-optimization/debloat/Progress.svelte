<script lang="ts">
  // A run's steps as they happen, then how it went.
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minus from "@lucide/svelte/icons/minus";
  import X from "@lucide/svelte/icons/x";
  import { debloat } from "./state.svelte";

  const done = $derived(debloat.steps.filter((step) => step.state !== "running").length);
  // A restart of Explorer may come on top of what was planned.
  const total = $derived(Math.max(debloat.expected, debloat.steps.length));
  /** After a run, the steps fold away unless something went wrong. */
  let showSteps = $state(false);
  const failed = $derived(debloat.steps.some((step) => step.state === "failed"));
  const listed = $derived(debloat.busy || showSteps || failed);
</script>

{#if debloat.steps.length}
  <section class="progress surface" aria-live="polite">
    <header>
      <div>
        <h3>
          {#if debloat.phase === "restorePoint"}Creating a restore point…
          {:else if debloat.busy}Working… {done} of {total}
          {:else if debloat.outcome?.needsAdmin}Stopped: administrator approval was declined
          {:else if debloat.outcome?.failed.length}Finished, with {debloat.outcome.failed.length} problem{debloat.outcome.failed.length === 1 ? "" : "s"}
          {:else}Finished{/if}
        </h3>
        {#if !debloat.busy && debloat.outcome}
          <p>
            {debloat.outcome.changed} change{debloat.outcome.changed === 1 ? "" : "s"} made.
            {#if debloat.outcome.reboot}Restart Windows to finish some of them.{/if}
          </p>
        {/if}
      </div>
      {#if !debloat.busy}
        <span class="end">
          {#if !failed}
            <button type="button" class="steps-btn" aria-expanded={showSteps} onclick={() => (showSteps = !showSteps)}>{showSteps ? "Hide steps" : "Show steps"}</button>
          {/if}
          <button class="icon-btn" title="Hide" aria-label="Hide the progress" onclick={() => ((debloat.steps = []), (debloat.outcome = null), (showSteps = false))}><X size={15} /></button>
        </span>
      {/if}
    </header>
    {#if debloat.busy}
      <div class="track"><span style:width="{total ? (done / total) * 100 : 0}%"></span></div>
    {/if}
    <ol hidden={!listed}>
      {#each debloat.steps as step (step.id)}
        <li class={step.state}>
          <span class="mark">
            {#if step.state === "running"}<LoaderCircle size={15} class="spin" />
            {:else if step.state === "done"}<CircleCheck size={15} />
            {:else if step.state === "failed"}<CircleX size={15} />
            {:else}<Minus size={15} />{/if}
          </span>
          <span class="text">
            <strong>{step.label}</strong>
            {#if step.detail}<small>{step.detail}</small>{:else if step.state === "unchanged"}<small>Already so</small>{/if}
          </span>
        </li>
      {/each}
    </ol>
  </section>
{/if}

<style>
  .progress { display: grid; gap: 8px; margin-top: 12px; padding: 10px 14px; }

  .end { display: flex; align-items: center; gap: 4px; flex: none; }
  .steps-btn { padding: 3px 8px; border-radius: 6px; color: rgb(var(--accent-soft-rgb)); font-size: 11.5px; font-weight: 550; }
  .steps-btn:hover { background: var(--hover); color: var(--text-1); }
  ol[hidden] { display: none; }

  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }

  h3 { font-size: 13.5px; font-weight: 650; }

  header p { margin-top: 3px; color: var(--text-2); font-size: 11.5px; }

  .track { overflow: hidden; height: 4px; border-radius: 999px; background: rgb(255 255 255 / 0.06); }

  .track span { display: block; height: 100%; border-radius: inherit; background: linear-gradient(90deg, #8c78ff, #55cae6); transition: width var(--dur-med) var(--ease-out); }

  ol { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 4px 14px; margin: 0; padding: 0; list-style: none; }

  li { display: flex; align-items: flex-start; gap: 9px; min-width: 0; padding: 6px 4px; }

  .mark { display: grid; place-items: center; flex: none; margin-top: 1px; color: var(--text-3); }

  li.running .mark { color: rgb(150 170 255); }

  li.done .mark { color: var(--ok); }

  li.failed .mark { color: var(--danger); }

  .text { display: grid; min-width: 0; gap: 1px; }

  .text strong { overflow: hidden; font-size: 12px; font-weight: 560; text-overflow: ellipsis; white-space: nowrap; }

  li.unchanged .text strong { color: var(--text-2); }

  .text small { color: var(--text-3); font-size: 10.5px; line-height: 1.35; }

  li.failed .text small { color: rgb(255 170 170 / 0.85); }
</style>
