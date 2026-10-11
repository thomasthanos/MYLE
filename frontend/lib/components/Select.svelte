<script lang="ts" module>
  export interface SelectOption<V extends string = string> {
    value: V;
    label: string;
  }
</script>

<script lang="ts" generics="T extends string = string">
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Popover from "./Popover.svelte";

  interface Props {
    value: T;
    options: readonly SelectOption<T>[];
    disabled?: boolean;
    ariaLabel?: string;
    size?: "sm" | "md";
    fullWidth?: boolean;
    minWidth?: string;
    align?: "start" | "end";
    placement?: "bottom" | "top";
    onchange?: (value: T) => void;
  }

  let {
    value = $bindable(),
    options,
    disabled = false,
    ariaLabel,
    size = "md",
    fullWidth = false,
    minWidth,
    align = "start",
    placement = "bottom",
    onchange,
  }: Props = $props();

  const selectedLabel = $derived(options.find((item) => item.value === value)?.label ?? value);

  function pick(next: T, close: () => void) {
    close();
    if (next === value) return;
    if (onchange) onchange(next);
    else value = next;
  }
</script>

<div class="select" class:full={fullWidth} style:min-width={minWidth}>
  <Popover {align} {placement}>
    {#snippet trigger({ toggle, open })}
      <button
        type="button"
        class="btn select-btn"
        class:small={size === "sm"}
        class:open
        {disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onclick={toggle}
      >
        <span class="label">{selectedLabel}</span>
        <ChevronDown size={14} class="chevron" />
      </button>
    {/snippet}
    {#snippet children({ close })}
      <div class="menu select-menu" role="listbox" aria-label={ariaLabel}>
        {#each options as option (option.value)}
          {@const active = option.value === value}
          <button
            type="button"
            class="menu-item"
            class:active
            role="option"
            aria-selected={active}
            onclick={() => pick(option.value, close)}
          >
            <span class="label">{option.label}</span>
            {#if active}<span class="hint"><Check size={14} /></span>{/if}
          </button>
        {/each}
      </div>
    {/snippet}
  </Popover>
</div>

<style>
  .select { display: inline-flex; min-width: 130px; }

  .select.full,
  .select :global(.popover) { width: 100%; }

  .select-btn { justify-content: space-between; width: 100%; height: 34px; padding: 0 11px; border-radius: 9px; font-size: 12.5px; background: var(--input-fill); box-shadow: inset 0 1px 2px rgb(0 0 0 / 0.2); transition: border-color var(--dur-fast), background var(--dur-fast), box-shadow var(--dur-fast); }

  .select-btn.small { height: 28px; padding: 0 9px; font-size: 12px; border-radius: 8px; }

  .select-btn:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.45); background: var(--hover); }

  .select-btn.open { border-color: rgb(var(--accent-rgb) / 0.65); background: var(--selected); box-shadow: 0 0 0 2px rgb(var(--accent-rgb) / 0.15); }

  .label { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }

  .select-btn :global(.chevron) { flex: none; color: var(--text-3); transition: transform var(--dur-fast) var(--ease-out); }

  .select-btn.open :global(.chevron) { transform: rotate(180deg); color: rgb(var(--accent-soft-rgb)); }

  .select-menu { min-width: 100%; max-height: 240px; overflow-y: auto; padding: 4px; border: 1px solid rgb(255 255 255 / 0.1); border-radius: 10px; background: var(--float-bg); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08), 0 10px 28px -6px rgb(0 0 0 / 0.75); }
</style>
