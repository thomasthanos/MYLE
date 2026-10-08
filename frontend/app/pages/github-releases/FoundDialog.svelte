<script lang="ts">
  // Repositories found in a folder: pick the ones to add.
  import { untrack } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { toast } from "../../../lib/toast.svelte";
  import { messageOf, type FoundRepo } from "./api";
  import { githubReleases as gr } from "./state.svelte";

  let { found, onclose }: { found: FoundRepo[]; onclose: () => void } = $props();

  let picked = $state<string[]>(untrack(() => found.filter((r) => !r.known).map((r) => r.path)));
  let working = $state(false);
  const fresh = $derived(found.filter((r) => !r.known));

  async function add() {
    if (!picked.length) return;
    working = true;
    try {
      await gr.addRepos(picked);
      onclose();
    } catch (error) {
      toast.error(messageOf(error));
    } finally {
      working = false;
    }
  }

  function toggle(path: string) {
    picked = picked.includes(path) ? picked.filter((p) => p !== path) : [...picked, path];
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !working && onclose()} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div class="dialog glass glass--3" role="dialog" aria-modal="true" aria-labelledby="gr-found-title" transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}>
    <header>
      <h2 id="gr-found-title">{found.length} {found.length === 1 ? "repository" : "repositories"} found</h2>
      <button class="icon-btn" aria-label="Close" onclick={onclose}><X size={16} /></button>
    </header>
    <div class="actions-row">
      <button class="btn small ghost" onclick={() => (picked = fresh.map((r) => r.path))}>All new</button>
      <button class="btn small ghost" onclick={() => (picked = [])}>None</button>
    </div>
    <ul>
      {#each found as repo (repo.path)}
        <li class:known={repo.known}>
          <label>
            <input type="checkbox" class="check" disabled={repo.known} checked={repo.known || picked.includes(repo.path)} onchange={() => toggle(repo.path)} />
            <span class="text">
              <strong>{repo.name}{#if repo.known}<em> · already added</em>{/if}</strong>
              <small>{repo.path}</small>
              {#if repo.apps.length}<small class="apps">Monorepo: {repo.apps.join(", ")}</small>{/if}
            </span>
          </label>
        </li>
      {/each}
    </ul>
    <footer>
      <button class="btn ghost" onclick={onclose} disabled={working}>Cancel</button>
      <button class="btn primary" disabled={working || !picked.length} onclick={() => void add()}>
        {#if working}<LoaderCircle size={14} class="spin" />{/if} Add {picked.length || ""}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 91;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgb(4 6 12 / 0.58);
  }

  .dialog {
    display: grid;
    gap: 10px;
    width: min(560px, 100%);
    max-height: min(640px, 100%);
    padding: 20px;
    border-radius: var(--radius-xl);
    grid-template-rows: auto auto minmax(0, 1fr) auto;
  }

  header,
  footer,
  .actions-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  header {
    justify-content: space-between;
  }

  footer {
    justify-content: flex-end;
  }

  h2 {
    font-size: 17px;
  }

  ul {
    display: grid;
    align-content: start;
    gap: 2px;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  label {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 8px;
  }

  label:hover {
    background: var(--hover);
  }

  .known {
    opacity: 0.55;
  }

  .text {
    display: grid;
    min-width: 0;
    gap: 1px;
  }

  strong {
    font-size: 13px;
  }

  em {
    color: var(--text-3);
    font-style: normal;
    font-weight: 400;
  }

  small {
    overflow: hidden;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 10.8px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .apps {
    color: #b7befa;
    font-family: inherit;
  }
</style>
