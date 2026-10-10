<script lang="ts">
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import Download from "@lucide/svelte/icons/download";
  import FileDown from "@lucide/svelte/icons/file-down";
  import FileUp from "@lucide/svelte/icons/file-up";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Popover from "../../../lib/components/Popover.svelte";
  import { appsState } from "./state.svelte";

  const n = $derived(appsState.selected.size);
</script>

<div class="selection-bar">
  <div class="summary">
    <span class="selection-dot" class:active={n > 0}></span>
    <span class="count" aria-live="polite">Selected <strong>{n}</strong></span>
    <span class="summary-divider"></span>
    <button class="btn ghost small clear" disabled={!n} onclick={() => appsState.uncheckAll()}>Uncheck all</button>
  </div>

  <span class="spacer"></span>

  <div class="actions">
    {#if appsState.busy}
      <button class="btn" onclick={() => appsState.cancelAll()}>Cancel all</button>
    {/if}

    <Popover placement="top" align="end">
      {#snippet trigger({ toggle, open })}
        <button
          class="btn more"
          class:open
          disabled={appsState.externallyLocked}
          aria-haspopup="menu"
          aria-expanded={open}
          onclick={toggle}
        >
          More
          <ChevronUp size={14} />
        </button>
      {/snippet}
      {#snippet children({ close })}
        <div class="menu" role="menu">
          <button class="menu-item" role="menuitem" disabled={appsState.externallyLocked} onclick={() => (close(), appsState.importList())}>
            <FileUp size={15} /> Import list…
          </button>
          <button class="menu-item" role="menuitem" disabled={appsState.externallyLocked} onclick={() => (close(), appsState.exportList())}>
            <FileDown size={15} /> Export list…
          </button>
          <button
            class="menu-item"
            role="menuitem"
            disabled={appsState.checking || appsState.externallyLocked}
            onclick={() => (close(), appsState.checkInstalled(true))}
          >
            <RefreshCw size={15} /> Check installed
          </button>
        </div>
      {/snippet}
    </Popover>

    <button class="btn primary install" disabled={!n || appsState.externallyLocked} onclick={() => appsState.installSelected()}>
      <Download size={15} />
      Install ({n})
    </button>
  </div>
</div>

<style>
  .selection-bar {
    position: sticky;
    bottom: -14px;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 28px;
    padding: 7px;
    border: 1px solid var(--btn-border);
    border-radius: 16px;
    background: var(--bar-fill);
    box-shadow: 0 10px 26px -16px rgb(0 0 0 / 0.6);
  }

  .summary {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 34px;
    padding: 0 5px 0 9px;
    border: 1px solid rgb(255 255 255 / 0.055);
    border-radius: 11px;
    background: rgb(255 255 255 / 0.025);
  }

  .selection-dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--idle);
    transition:
      background var(--dur-med),
      box-shadow var(--dur-med);
  }

  .selection-dot.active {
    background: var(--accent);
    box-shadow: 0 0 9px var(--accent-glow);
  }

  .count {
    color: var(--text-2);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .count strong {
    margin-left: 3px;
    color: var(--text-1);
  }

  .summary-divider {
    width: 1px;
    height: 16px;
    background: rgb(255 255 255 / 0.075);
  }

  .clear {
    color: var(--text-2);
  }

  .spacer {
    flex: 1;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .more {
    min-width: 82px;
  }

  .more.open {
    border-color: rgb(var(--accent-rgb) / 0.28);
    background: var(--selected);
  }

  .install {
    min-width: 126px;
  }

  @media (max-width: 720px) {
    .summary-divider,
    .clear {
      display: none;
    }
  }
</style>
