<script lang="ts">
  // The "What's new" dialog: the release notes of each version an update
  // brought, newest first (lib/whats-new.svelte.ts).
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { confirmState } from "../confirm.svelte";
  import { toast } from "../toast.svelte";
  import { whatsNew } from "../whats-new.svelte";
  import Markdown from "./Markdown.svelte";

  let closeButton = $state<HTMLButtonElement>();

  $effect(() => {
    if (whatsNew.shown) closeButton?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    // A confirmation on top answers Escape first.
    if (whatsNew.shown && !confirmState.current && e.key === "Escape") whatsNew.close();
  }

  function open(url: string) {
    void openUrl(url).catch((error) => toast.error(`Could not open the link: ${error instanceof Error ? error.message : String(error)}`));
  }

  function day(date: string | null): string {
    if (!date) return "";
    const parsed = new Date(date);
    return Number.isNaN(parsed.getTime()) ? "" : parsed.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if whatsNew.shown}
  {@const w = whatsNew.shown}
  <div class="backdrop" transition:fade={{ duration: 150 }}>
    <div class="dialog glass glass--3" role="dialog" aria-modal="true" aria-labelledby="whats-new-title" transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}>
      <header>
        <span class="icon"><Sparkles size={18} /></span>
        <div>
          <h2 id="whats-new-title">What's new in MYLE v{w.current}</h2>
          <p class="sub">
            {#if w.previous && w.versions.length > 1}
              {w.versions.length} versions since v{w.previous}
            {:else if w.previous}
              Updated from v{w.previous}
            {:else}
              The notes of this version
            {/if}
            {#if w.offline}· GitHub couldn't be reached, so these are the notes saved on this PC{/if}
          </p>
        </div>
      </header>

      <div class="body">
        {#each w.versions as v (v.version)}
          <section>
            {#if w.versions.length > 1}
              <h3 class="version">v{v.version}{#if day(v.date)}<span> · {day(v.date)}</span>{/if}</h3>
            {/if}
            {#if v.notes}
              <Markdown source={v.notes} />
            {:else}
              <p class="none">Updated to v{v.version}. Its notes couldn't be loaded right now.</p>
            {/if}
            <button class="link" onclick={() => open(v.url)}><ExternalLink size={12} /> v{v.version} on GitHub</button>
          </section>
        {/each}
      </div>

      <footer>
        <button bind:this={closeButton} class="btn primary" onclick={() => whatsNew.close()}>Got it</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--scrim);
  }

  .dialog {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    width: min(640px, 100%);
    max-height: min(78vh, 760px);
    border-radius: var(--radius-xl);
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 20px 22px 14px;
    border-bottom: 1px solid rgb(255 255 255 / 0.06);
  }

  .icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.25);
    border-radius: 10px;
    background: rgb(var(--accent-rgb) / 0.12);
    color: rgb(var(--accent-soft-rgb));
  }

  h2 {
    font-size: 17px;
  }

  .sub {
    margin-top: 2px;
    color: var(--text-3);
    font-size: 12px;
  }

  .body {
    padding: 14px 22px;
    overflow-y: auto;
  }

  section + section {
    margin-top: 18px;
    padding-top: 16px;
    border-top: 1px solid rgb(255 255 255 / 0.07);
  }

  .version {
    margin-bottom: 8px;
    color: var(--text-1);
    font-size: 15px;
    font-family: var(--font-mono);
  }

  .version span {
    color: var(--text-3);
    font-family: inherit;
    font-size: 12px;
    font-weight: 400;
  }

  .none {
    color: var(--text-2);
    font-size: 13px;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 10px;
    padding: 0;
    border: 0;
    background: none;
    color: rgb(var(--accent-rgb));
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    padding: 12px 22px 16px;
    border-top: 1px solid rgb(255 255 255 / 0.06);
  }
</style>
