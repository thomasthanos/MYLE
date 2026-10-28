<script lang="ts">
  // Everything chosen, in one list, before any of it happens. Unticking a line leaves it out of this run (and out of the choices).
  import { onMount } from "svelte";
  import Info from "@lucide/svelte/icons/info";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { SvelteSet } from "svelte/reactivity";
  import { portal } from "../../../../lib/portal";
  import SettingRow from "./SettingRow.svelte";
  import { inProfile } from "./selection";
  import { debloat } from "./state.svelte";

  /** Lines unticked here; they go when the changes are applied. */
  const leftOut = new SvelteSet<string>();
  let dialog = $state<HTMLDivElement>();
  let applyButton = $state<HTMLButtonElement>();

  const on = $derived(debloat.pending.on);
  const off = $derived(debloat.pending.off);
  /** Parts of Windows read better on their own: "Remove" and "Add back". */
  const isFeature = (tweak: { category: string }) => tweak.category === "features";
  const featureName = (title: string) => title.replace(/^Remove /, "");
  const apps = $derived(debloat.pendingApps);
  const kept = $derived(
    on.filter((tweak) => !leftOut.has(tweak.id)).length +
      off.filter((tweak) => !leftOut.has(tweak.id)).length +
      apps.filter((app) => !leftOut.has(`app:${app.id}`)).length,
  );
  const changesPc = $derived(
    on.some((tweak) => !leftOut.has(tweak.id)) || apps.some((app) => !leftOut.has(`app:${app.id}`)),
  );
  const restart = $derived(on.some((tweak) => tweak.restart && !leftOut.has(tweak.id)));
  const asksAgain = $derived(on.filter((tweak) => tweak.confirm && !leftOut.has(tweak.id)));

  function toggle(key: string, keep: boolean) {
    if (keep) leftOut.delete(key);
    else leftOut.add(key);
  }

  function close() {
    debloat.reviewing = false;
  }

  async function apply() {
    debloat.forget(
      [...on, ...off].filter((tweak) => leftOut.has(tweak.id)).map((tweak) => tweak.id),
      apps.filter((app) => leftOut.has(`app:${app.id}`)).map((app) => app.id),
    );
    await debloat.applyPending();
  }

  onMount(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const frame = requestAnimationFrame(() => applyButton?.focus());
    return () => {
      cancelAnimationFrame(frame);
      if (previous?.isConnected) previous.focus();
    };
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab" || !dialog) return;
    const items = [...dialog.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled])")];
    if (!items.length) return;
    const [first, last] = [items[0], items[items.length - 1]];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" {@attach portal}>
  <div class="dialog glass glass--3" role="dialog" aria-modal="true" aria-labelledby="review-title" tabindex="-1" bind:this={dialog}>
    <div class="body">
      <header>
        <h2 id="review-title">Review the changes</h2>
        <p>Untick anything you want to leave out.</p>
      </header>

      <div class="lists">
        {#if on.some((tweak) => !isFeature(tweak))}
          <section aria-label="To turn on">
            <h3>Turn on · {on.filter((tweak) => !isFeature(tweak)).length}</h3>
            {#each on.filter((tweak) => !isFeature(tweak)) as tweak (tweak.id)}
              <SettingRow control="check" title={tweak.title} summary={tweak.summary} note={tweak.note}
                checked={!leftOut.has(tweak.id)} recommended={inProfile(tweak.level, "recommended")}
                caution={tweak.risk === "caution"} restart={tweak.restart} onchange={(keep) => toggle(tweak.id, keep)} />
            {/each}
          </section>
        {/if}
        {#if off.some((tweak) => !isFeature(tweak))}
          <section aria-label="To turn off">
            <h3>Turn off · {off.filter((tweak) => !isFeature(tweak)).length}</h3>
            {#each off.filter((tweak) => !isFeature(tweak)) as tweak (tweak.id)}
              <SettingRow control="check" title={tweak.title}
                summary={tweak.canUndo ? "What MYLE changed is put back exactly as it was." : "Switched back to the Windows default."}
                checked={!leftOut.has(tweak.id)} onchange={(keep) => toggle(tweak.id, keep)} />
            {/each}
          </section>
        {/if}
        {#if on.some(isFeature)}
          <section aria-label="Windows features to remove">
            <h3>Remove Windows features · {on.filter(isFeature).length}</h3>
            {#each on.filter(isFeature) as tweak (tweak.id)}
              <SettingRow control="check" title={featureName(tweak.title)} summary={tweak.summary} note={tweak.note}
                checked={!leftOut.has(tweak.id)} caution={tweak.risk === "caution"} onchange={(keep) => toggle(tweak.id, keep)} />
            {/each}
          </section>
        {/if}
        {#if off.some(isFeature)}
          <section aria-label="Windows features to add back">
            <h3>Add back · {off.filter(isFeature).length}</h3>
            {#each off.filter(isFeature) as tweak (tweak.id)}
              <SettingRow control="check" title={featureName(tweak.title)} summary="Downloaded from Windows Update: it takes a few minutes."
                checked={!leftOut.has(tweak.id)} onchange={(keep) => toggle(tweak.id, keep)} />
            {/each}
          </section>
        {/if}
        {#if apps.length}
          <section aria-label="Apps to remove">
            <h3>Remove apps · {apps.length}</h3>
            <div class="apps">
              {#each apps as app (app.id)}
                <SettingRow control="check" title={app.title} checked={!leftOut.has(`app:${app.id}`)}
                  onchange={(keep) => toggle(`app:${app.id}`, keep)} />
              {/each}
            </div>
          </section>
        {/if}
      </div>

      <ul class="notes">
        {#if changesPc}<li><Info size={13} /> A restore point is made first, and Windows asks once for administrator approval.</li>{/if}
        {#if apps.some((app) => !leftOut.has(`app:${app.id}`))}<li><Info size={13} /> Apps are removed for every user of this PC; the Apps tab can get them again from the Microsoft Store.</li>{/if}
        {#if restart}<li><Info size={13} /> Some changes finish after Windows restarts.</li>{/if}
        {#each asksAgain as tweak (tweak.id)}<li class="ask"><Info size={13} /> You will be asked once more about “{tweak.title}”.</li>{/each}
      </ul>

      <footer>
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <button type="button" class="btn primary" bind:this={applyButton} disabled={!kept || debloat.locked} onclick={apply}>
          <Sparkles size={15} /> {kept === 1 ? "Apply 1 change" : `Apply ${kept} changes`}
        </button>
      </footer>
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 22px; background: rgb(4 6 12 / 0.64); }
  .dialog { position: relative; display: flex; flex-direction: column; width: min(640px, 100%); max-height: calc(100vh - 44px); border-color: rgb(var(--accent-rgb) / 0.24); border-radius: var(--radius-xl); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.07), 0 24px 60px -24px rgb(0 0 0 / 0.88); }
  .dialog:focus-visible { outline: none; }
  .body { display: grid; grid-template-rows: auto minmax(0, 1fr) auto auto; gap: 12px; min-height: 0; padding: 18px; }
  header { display: grid; gap: 3px; }
  h2 { color: var(--text-1); font-size: 16px; font-weight: 650; }
  header p { color: var(--text-2); font-size: 12px; }
  .lists { display: grid; align-content: start; gap: 12px; min-height: 0; max-height: 52vh; margin: 0 -6px; padding: 0 6px; overflow: auto; }
  section { display: grid; gap: 1px; }
  h3 { margin: 0 0 4px 8px; color: var(--text-3); font-size: 10.5px; font-weight: 650; letter-spacing: 0.05em; text-transform: uppercase; }
  .apps { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1px 8px; }
  @media (max-width: 560px) { .apps { grid-template-columns: minmax(0, 1fr); } }
  .notes { display: grid; gap: 5px; padding: 10px 12px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 10px; background: rgb(255 255 255 / 0.025); list-style: none; }
  .notes:empty { display: none; }
  .notes li { display: flex; align-items: flex-start; gap: 7px; color: var(--text-2); font-size: 11.5px; line-height: 1.45; }
  .notes li :global(svg) { flex: none; margin-top: 2px; color: rgb(var(--accent-soft-rgb)); }
  .notes li.ask :global(svg) { color: #efc38a; }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  .btn.primary { border-color: rgb(var(--accent-rgb) / 0.45); background: rgb(var(--accent-rgb) / 0.3); color: var(--text-1); }
  .btn.primary:hover:not(:disabled) { background: rgb(var(--accent-rgb) / 0.42); filter: none; }
</style>
