<script lang="ts">
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  type Common = {
    label: string;
    /** What it does, or why it is unavailable, in plain words. */
    hint?: string;
    disabled?: boolean;
  };
  /** A switch: the whole row turns it on or off. */
  type Switch = Common & { checked: boolean; onchange: (on: boolean) => void };
  /* An action, often Android's own settings: the whole row opens it. The badge tells its state at a glance (on, off, or why it is unavailable). */
  type Action = Common & { onclick: () => void; badge?: { text: string; on?: boolean } };

  let props: Switch | Action = $props();
</script>

{#if "checked" in props}
  <label class="row" class:disabled={props.disabled}>
    <span class="text"><b>{props.label}</b>{#if props.hint}<small>{props.hint}</small>{/if}</span>
    <input
      type="checkbox"
      class="switch"
      role="switch"
      checked={props.checked}
      disabled={props.disabled}
      onchange={(event) => {
        // The state decides: the switch moves once the change has happened (a fingerprint prompt can still be cancelled).
        const on = event.currentTarget.checked;
        event.currentTarget.checked = props.checked;
        props.onchange(on);
      }}
    />
  </label>
{:else}
  <button class="row" disabled={props.disabled} onclick={props.onclick}>
    <span class="text"><b>{props.label}</b>{#if props.hint}<small>{props.hint}</small>{/if}</span>
    {#if props.badge}<span class="badge" class:on={props.badge.on}>{props.badge.text}</span>{/if}
    {#if !props.disabled}<span class="go" aria-hidden="true"><ChevronRight size={17} /></span>{/if}
  </button>
{/if}

<style>
  /* At least 52 px tall: a finger's target, the whole row. */
  .row { display: flex; align-items: center; gap: 12px; width: 100%; min-height: 52px; padding: 10px 12px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 13px; background: rgb(255 255 255 / 0.03); color: var(--text-1); text-align: left; transition: background 140ms var(--ease-out); }

  button.row:active:not(:disabled),
  label.row:not(.disabled):active { background: rgb(255 255 255 / 0.08); }

  .text { display: grid; flex: 1; gap: 3px; min-width: 0; overflow-wrap: anywhere; }

  b { font-size: 14px; font-weight: 600; }

  small { color: var(--text-2); font-size: 12.5px; line-height: 1.4; }

  .row:disabled b,
  .row.disabled b { color: var(--text-2); }

  .badge { flex: none; padding: 4px 10px; border: 1px solid var(--glass-border); border-radius: 999px; background: rgb(255 255 255 / 0.04); color: var(--text-3); font-size: 12px; font-weight: 600; white-space: nowrap; }

  .badge.on { border-color: rgb(16 185 129 / 0.45); background: rgb(16 185 129 / 0.14); color: #8fe3b6; }

  .go { display: grid; flex: none; place-items: center; color: var(--text-3); }

  .switch { width: 46px; height: 26px; }

  .switch::before { width: 20px; height: 20px; }

  .switch:checked::before { transform: translateX(20px); }
</style>
