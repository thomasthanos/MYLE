<script lang="ts">
  import { untrack } from "svelte";
  import X from "@lucide/svelte/icons/x";
  import { pickable, type PickerKind } from "./model";
  import { steamAutomationState as page } from "./state.svelte";

  let { kind, selected, onSave, onClose }: { kind: PickerKind; selected: number[]; onSave: (games: number[]) => void; onClose: () => void } = $props();

  const LIMIT = 32;
  let query = $state("");
  let picked = $state(untrack(() => new Set(selected)));

  const games = $derived(pickable(page.library ?? [], kind, query));
  const atLimit = $derived(kind === "idle" && picked.size >= LIMIT);
  const title = $derived(kind === "idle" ? "Games to idle" : "Games to unlock achievements in");

  function toggle(appid: number, on: boolean) {
    const next = new Set(picked);
    if (on) next.add(appid);
    else next.delete(appid);
    picked = next;
  }

  $effect(() => {
    void page.loadLibrary();
  });
</script>

<svelte:window onkeydown={(event) => event.key === "Escape" && onClose()} />

<div class="scrim" role="presentation" onclick={(event) => event.target === event.currentTarget && onClose()}>
  <div class="dialog glass" role="dialog" aria-modal="true" aria-label={title}>
    <header>
      <h2>{title}</h2>
      <button class="icon-btn" type="button" aria-label="Close" onclick={onClose}><X size={16} /></button>
    </header>
    <!-- svelte-ignore a11y_autofocus -->
    <input class="input" type="search" placeholder="Search your games" bind:value={query} autofocus />
    <div class="list">
      {#if page.libraryLoading}
        <p class="note">Reading your library from Steam…</p>
      {:else if page.libraryError}
        <p class="note">{page.libraryError}</p>
      {:else if games.length === 0}
        <p class="note">No games match.</p>
      {:else}
        {#each games as game (game.appid)}
          <label class="row">
            <input class="check" type="checkbox" checked={picked.has(game.appid)} disabled={atLimit && !picked.has(game.appid)} onchange={(event) => toggle(game.appid, event.currentTarget.checked)} />
            {#if game.coverUrl}<img src={game.coverUrl} alt="" loading="lazy" />{:else}<span class="no-art"></span>{/if}
            <span class="name">{game.name}</span>
          </label>
        {/each}
      {/if}
    </div>
    <footer>
      <span class="count">{picked.size}{kind === "idle" ? ` of ${LIMIT}` : ""} chosen</span>
      <button class="btn" type="button" onclick={onClose}>Cancel</button>
      <button class="btn primary" type="button" onclick={() => onSave([...picked])}>Save</button>
    </footer>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 24px; background: var(--scrim); }
  .dialog { display: flex; flex-direction: column; gap: 10px; width: min(520px, 100%); max-height: 80vh; padding: 16px; border-radius: var(--radius-lg); }
  header, footer { display: flex; align-items: center; gap: 8px; }
  header { justify-content: space-between; }
  h2 { margin: 0; font-size: 15px; }
  .list { flex: 1; min-height: 160px; overflow-y: auto; }
  .row { display: flex; align-items: center; gap: 10px; padding: 5px 6px; border-radius: var(--radius-sm); cursor: pointer; }
  .row:hover { background: var(--hover); }
  img, .no-art { width: 62px; aspect-ratio: 460 / 215; border-radius: 4px; object-fit: cover; }
  .no-art { background: var(--input-fill); }
  .name { min-width: 0; overflow: hidden; font-size: 13px; white-space: nowrap; text-overflow: ellipsis; }
  .note { margin: 18px 0; color: var(--text-3); font-size: 12.5px; text-align: center; }
  .count { flex: 1; color: var(--text-3); font-size: 12px; }
</style>
