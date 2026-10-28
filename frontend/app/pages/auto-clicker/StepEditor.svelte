<script lang="ts">
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Select from "../../../lib/components/Select.svelte";
  import type { Step } from "./api";

  const BUTTON_OPTIONS = [
    { value: "left", label: "Left" },
    { value: "right", label: "Right" },
    { value: "middle", label: "Middle" },
  ] as const;

  const CLICK_TYPE_OPTIONS = [
    { value: "single", label: "Single click" },
    { value: "double", label: "Double click" },
  ] as const;
  let { step, index, count, disabled, active = false, picking = false, onchange, onpick, onremove, onmove }: {
    step: Step; index: number; count: number; disabled: boolean; active?: boolean; picking?: boolean;
    onchange: (change: Partial<Step>) => void; onpick: () => void; onremove: () => void; onmove: (direction: number) => void;
  } = $props();
</script>

<article class="step" class:active class:picking>
  <header>
    <span class="order">{index + 1}</span>
    <div class="heading"><h3>Point {index + 1}</h3><span>{step.x.toLocaleString()}, {step.y.toLocaleString()}</span></div>
    <button class="btn pick" {disabled} onclick={onpick}><Crosshair size={15} /> {picking ? "Picking…" : "Pick location"}</button>
    <div class="actions">
      <button class="icon-btn" disabled={disabled || index === 0} aria-label={`Move step ${index + 1} up`} onclick={() => onmove(-1)}><ArrowUp size={15} /></button>
      <button class="icon-btn" disabled={disabled || index === count - 1} aria-label={`Move step ${index + 1} down`} onclick={() => onmove(1)}><ArrowDown size={15} /></button>
      <button class="icon-btn" {disabled} aria-label={`Remove step ${index + 1}`} onclick={onremove}><Trash2 size={15} /></button>
    </div>
  </header>
  <fieldset {disabled} aria-label={`Step ${index + 1} settings`}>
    <div class="coordinates">
      <label><span>X position</span><input class="input" type="number" step="1" value={step.x} oninput={e => onchange({ x: e.currentTarget.valueAsNumber })} /></label>
      <label><span>Y position</span><input class="input" type="number" step="1" value={step.y} oninput={e => onchange({ y: e.currentTarget.valueAsNumber })} /></label>
    </div>
    <label><span>Clicks</span><input class="input" type="number" min="1" max="1000000" step="1" value={step.clicks} oninput={e => onchange({ clicks: e.currentTarget.valueAsNumber })} /></label>
    <label><span>Mouse button</span><Select value={step.button} options={BUTTON_OPTIONS} {disabled} fullWidth={true} onchange={v => onchange({ button: v as Step["button"] })} ariaLabel="Mouse button" /></label>
    <label><span>Click type</span><Select value={step.clickType} options={CLICK_TYPE_OPTIONS} {disabled} fullWidth={true} onchange={v => onchange({ clickType: v as Step["clickType"] })} ariaLabel="Click type" /></label>
    <label><span>Click interval <small>ms</small></span><input class="input" type="number" min="10" max="86400000" step="1" value={step.intervalMs} oninput={e => onchange({ intervalMs: e.currentTarget.valueAsNumber })} /></label>
    <label class="delay-field"><span>Delay after <small>ms</small></span><input class="input" type="number" min="0" max="86400000" step="1" value={step.delayMs} oninput={e => onchange({ delayMs: e.currentTarget.valueAsNumber })} /></label>
  </fieldset>
</article>

<style>
  .step input[type="number"]::-webkit-outer-spin-button,
  .step input[type="number"]::-webkit-inner-spin-button { -webkit-appearance: none; margin: 0; }
  .step input[type="number"] { appearance: textfield; }

  .step { position: relative; border: 1px solid var(--glass-border); border-radius: 14px; background: var(--surface-fill); box-shadow: var(--surface-depth); padding: 16px; }
  .step.active, .step.picking { border-color: var(--accent); box-shadow: inset 3px 0 var(--accent), var(--surface-depth); }
  header { display: flex; align-items: center; gap: 10px; margin-bottom: 16px; }
  .order { display: grid; place-items: center; flex: 0 0 32px; height: 32px; border-radius: 10px; background: linear-gradient(180deg, rgb(var(--accent-rgb) / 0.25) 0%, rgb(var(--accent-rgb) / 0.1) 100%); border: 1px solid rgb(var(--accent-rgb) / 0.3); color: rgb(var(--accent-soft-rgb)); font-size: 15px; font-weight: 700; box-shadow: inset 0 1px rgb(255 255 255 / .12); }
  .heading { flex: 1; min-width: 0; }
  h3 { font-size: 14px; margin: 0; font-weight: 700; color: var(--text-1); }
  .heading span { font-size: 11.5px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .actions { display: flex; gap: 4px; }
  .actions .icon-btn { width: 28px; height: 28px; border-radius: 6px; border: 1px solid rgba(255, 255, 255, 0.08); background: rgba(255, 255, 255, 0.03); }
  .actions .icon-btn:hover:not(:disabled) { background: var(--hover); border-color: rgba(255, 255, 255, 0.2); }
  .pick { height: 30px; padding: 0 11px; font-size: 11.5px; font-weight: 600; gap: 6px; border-radius: 7px; background: linear-gradient(180deg, rgba(255, 255, 255, 0.08) 0%, rgba(255, 255, 255, 0.02) 100%); border: 1px solid rgba(255, 255, 255, 0.12); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.1), 0 2px 4px rgba(0, 0, 0, 0.2); cursor: pointer; transition: all 0.15s ease; }
  .pick:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.5); background: linear-gradient(180deg, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0.04) 100%); transform: translateY(-0.5px); }
  .step.picking .pick { background: rgb(var(--accent-rgb) / 0.25); border-color: rgb(var(--accent-soft-rgb)); color: rgb(var(--accent-soft-rgb)); box-shadow: 0 0 12px rgb(var(--accent-rgb) / 0.4); }
  fieldset { display: grid; grid-template-columns: 1fr 1fr; gap: 10px 14px; border: 0; padding: 0; margin: 0; min-width: 0; }
  label { display: flex; flex-direction: column; gap: 5px; min-width: 0; font-size: 12px; color: var(--text-2); }
  label > span { display: flex; justify-content: space-between; gap: 6px; }
  small { color: var(--text-3); font-size: 11px; }
  .input { width: 100%; min-width: 0; height: 36px; font-variant-numeric: tabular-nums; }
  .step :global(.select-btn) { height: 36px; border-radius: 9px; }
  .coordinates { grid-column: 1 / -1; display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .delay-field { grid-column: 1 / -1; }
  @media (max-width: 1150px) {
    header { flex-wrap: wrap; }
    .pick { order: 1; width: 100%; }
  }
  @media (max-width: 480px) { fieldset { grid-template-columns: 1fr; } .coordinates { grid-column: 1; grid-template-columns: 1fr; } }
  @media (max-width: 560px) { header { flex-wrap: wrap; } .pick { order: 1; width: 100%; } .step { padding: 13px; } }
</style>
