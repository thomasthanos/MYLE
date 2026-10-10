<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import History from "@lucide/svelte/icons/history";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import X from "@lucide/svelte/icons/x";
  import Select from "../../../lib/components/Select.svelte";
  import { portal } from "../../../lib/portal";
  import { dialogFocus } from "./dialog-focus";
  import { formatBytes, formatDate, gameSavesState as gs } from "./state.svelte";

  let cancelButton = $state<HTMLButtonElement>();
  const choices = $derived(
    gs.selectedGames
      .map((game) => ({ gameId: game.id, snapshotId: gs.snapshotFor(game) }))
      .filter((item) => item.snapshotId),
  );
  const missingLocation = $derived(
    gs.selectedGames.some((game) => game.status === "needsLocation" && !gs.mappingFor(game)),
  );
  const canRestore = $derived(choices.length > 0 && choices.length === gs.selectedGames.length && !missingLocation);
  const selectedBytes = $derived(gs.selectedGames.reduce((sum, game) => sum + (game.snapshots.find((snapshot) => snapshot.id === gs.snapshotFor(game))?.bytes ?? 0), 0));

  onMount(() => cancelButton?.focus());

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !gs.locked) gs.restoreDialogOpen = false;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div
    class="dialog glass glass--3"
    role="dialog"
    aria-modal="true"
    aria-labelledby="snapshot-title"
    tabindex="-1"
    {@attach dialogFocus}
    transition:scale={{ start: 0.97, duration: 170, easing: cubicOut }}
  >
    <header>
      <span class="heading-icon"><History size={19} /></span>
      <span class="heading">
        <h2 id="snapshot-title">Get your progress back</h2>
        <p>Review the saved copies below. Your current saves are kept in a safety backup first.</p>
      </span>
      <button class="icon-btn" aria-label="Close" disabled={gs.locked} onclick={() => (gs.restoreDialogOpen = false)}><X size={16} /></button>
    </header>

    <div class="games">
      {#each gs.selectedGames as game (game.id)}
        {@const mapping = gs.mappingFor(game)}
        <article class="game surface">
          <div class="game-title">
            <strong>{game.title}</strong>
            <span>{game.snapshots.length} {game.snapshots.length === 1 ? "snapshot" : "snapshots"}</span>
          </div>

          {#if game.snapshots.length}
            <div class="snapshot-field">
              <span>Restore from</span>
              <Select
                value={gs.snapshotFor(game)}
                options={game.snapshots.map((snapshot) => ({
                  value: snapshot.id,
                  label: `${formatDate(snapshot.timestamp)} · ${formatBytes(snapshot.bytes)}${snapshot.label ? ` · ${snapshot.label}` : ""}${snapshot.isSafety ? " · Safety" : ""}`,
                }))}
                ariaLabel={`Restore snapshot for ${game.title}`}
                fullWidth
                disabled={gs.locked}
                onchange={(id) => gs.setSnapshot(game.id, id)}
              />
            </div>
          {:else}
            <p class="problem">No restorable snapshot is available.</p>
          {/if}

          {#if game.status === "needsLocation" || mapping}
            <div class="destination" class:missing={!mapping}>
              <span>
                <small>Restore location</small>
                <strong class="selectable" title={mapping?.target ?? ""}>{mapping?.target ?? "Choose a folder on this PC"}</strong>
              </span>
              <button class="btn small" disabled={gs.locked} onclick={() => gs.chooseRestoreLocation(game)}>
                <FolderSearch size={14} /> {mapping ? "Change" : "Choose"}
              </button>
              {#if mapping}
                <button class="icon-btn" disabled={gs.locked} title="Clear restore location" aria-label={`Clear restore location for ${game.title}`} onclick={() => gs.removeRestoreLocation(game)}>
                  <X size={14} />
                </button>
              {/if}
            </div>
          {/if}
        </article>
      {/each}
    </div>

    <div class="safety">
      <ShieldCheck size={15} />
      Close these games before restoring. You can undo a completed restore for 7 days.
    </div>

    <footer>
      <span class="summary" aria-live="polite">{choices.length} {choices.length === 1 ? "game" : "games"} · {formatBytes(selectedBytes)}{missingLocation ? " · Choose a restore folder" : ""}</span>
      <button bind:this={cancelButton} class="btn" disabled={gs.locked} onclick={() => (gs.restoreDialogOpen = false)}>Cancel</button>
      <button class="btn primary" disabled={!canRestore || gs.locked} onclick={() => gs.restore(choices)}>
        <RotateCcw size={15} /> Continue to restore
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 91; display: grid; place-items: center; padding: 24px; background: var(--scrim); }

  /* The game list scrolls, not the glass element: an overflow on it clips
     the glass rim (see BiosRestartDialog). */
  .dialog { display: grid; grid-template-rows: auto minmax(0, 1fr) auto auto; gap: 15px; width: min(680px, 100%); max-height: calc(100vh - 48px); padding: 20px; border-radius: var(--radius-xl); }

  header { display: flex; align-items: flex-start; gap: 10px; }

  .heading-icon { display: grid; place-items: center; width: 34px; height: 34px; flex: none; border: 1px solid rgb(var(--accent-rgb) / 0.17); border-radius: 10px; background: rgb(var(--accent-rgb) / 0.08); color: var(--accent); }

  .heading { flex: 1; }

  h2 { font-size: 17px; }

  header p { margin-top: 3px; color: var(--text-3); font-size: 12px; }

  .games { display: grid; gap: 7px; overflow: auto; }

  .game { display: grid; grid-template-columns: minmax(120px, 0.7fr) minmax(220px, 1.3fr); align-items: center; gap: 10px 16px; padding: 11px 12px; }

  .game-title { display: grid; min-width: 0; }

  .game-title strong { overflow: hidden; font-size: 12.5px; white-space: nowrap; text-overflow: ellipsis; }

  .game-title span { color: var(--text-3); font-size: 10.5px; }

  .snapshot-field { display: grid; gap: 4px; min-width: 0; }

  .snapshot-field > span,
  .destination small { color: var(--text-3); font-size: 10px; font-weight: 600; letter-spacing: 0.035em; text-transform: uppercase; }

  .destination { grid-column: 1 / -1; display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 7px; padding-top: 8px; border-top: 1px solid rgb(255 255 255 / 0.045); }

  .destination > span { display: grid; min-width: 0; }

  .destination strong { overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 10px; font-weight: 400; white-space: nowrap; text-overflow: ellipsis; }

  .destination.missing strong,
  .problem { color: rgb(245 188 95 / 0.82); }

  .problem { font-size: 11.5px; }

  .safety { display: flex; align-items: center; gap: 7px; color: rgb(92 218 166 / 0.72); font-size: 11.5px; }

  footer { display: flex; align-items: center; justify-content: flex-end; gap: 8px; }

  .summary { margin-right: auto; color: var(--text-3); font-size: 11.5px; }

  button:disabled { opacity: 0.45; pointer-events: none; }

  @media (max-width: 620px) {
    .game {
      grid-template-columns: 1fr;
    }

    .destination {
      grid-column: auto;
    }
  }
</style>
