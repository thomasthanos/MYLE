<script lang="ts">
  import { onMount } from "svelte";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import Route from "@lucide/svelte/icons/route";
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import Plus from "@lucide/svelte/icons/plus";
  import Minus from "@lucide/svelte/icons/minus";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Repeat2 from "@lucide/svelte/icons/repeat-2";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import Check from "@lucide/svelte/icons/check";
  import Zap from "@lucide/svelte/icons/zap";
  import Layers from "@lucide/svelte/icons/layers";
  import Timer from "@lucide/svelte/icons/timer";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import Select from "../../../lib/components/Select.svelte";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { confirm } from "../../../lib/confirm.svelte";
  import { clickerState as s } from "./state.svelte";
  import { intervalInMs, progressShare, progressText } from "./model";
  import type { Config } from "./api";
  import StepEditor from "./StepEditor.svelte";

  const BUTTON_OPTIONS = [
    { value: "left", label: "Left button" },
    { value: "right", label: "Right button" },
    { value: "middle", label: "Middle button" },
  ] as const;

  const CLICK_TYPE_OPTIONS = [
    { value: "single", label: "Single click" },
    { value: "double", label: "Double click" },
  ] as const;

  const PRESETS = [
    { ms: 10, label: "10 ms" },
    { ms: 25, label: "25 ms" },
    { ms: 50, label: "50 ms" },
    { ms: 100, label: "100 ms" },
    { ms: 500, label: "500 ms" },
    { ms: 1000, label: "1 s" },
  ] as const;

  onMount(() => { void s.load(); });
  let unit = $state<"ms" | "seconds">("ms");
  let renaming = $state(false);
  let name = $state("");
  const profile = $derived(s.active);
  const config = $derived(profile?.config);
  const activeStep = $derived(s.status?.phase === "running" ? s.status.step - 1 : -1);
  const progress = $derived(s.status ? progressShare(s.status) : 0);
  const title = $derived(s.status ? progressText(s.status) : "Ready when you are");
  const hasProgress = $derived(!!s.status && s.status.runId > 0 && s.status.steps > 0 && s.status.phase !== "picking" && s.status.phase !== "ready");
  const pickActive = $derived(s.pickingId !== null || s.status?.phase === "picking");
  const cps = $derived(
    config?.intervalMs && config.intervalMs > 0
      ? (1000 / config.intervalMs).toLocaleString(undefined, {
          maximumFractionDigits: config.intervalMs < 1000 ? 0 : 1,
        })
      : null
  );

  function setPreset(ms: number) {
    if (s.busy) return;
    if (unit === "seconds" && ms < 1000) unit = "ms";
    s.edit({ intervalMs: ms });
  }

  async function removeProfile() {
    if (!profile || s.busy) return;
    if (await confirm({ title: "Delete profile?", message: `Remove “${profile.name}” and its saved steps from this PC?`, confirmLabel: "Delete profile", danger: true })) s.removeProfile();
  }
</script>

<PageHeader title="Auto Clicker & Macros" subtitle="Repeat clicks automatically or build multi-point sequences with precision timing and global F6 control." />

{#if s.error || s.saveError || s.status?.error}
  <div class="notice error" role="alert"><CircleAlert size={18} /><span>{s.error || s.saveError || s.status?.error}</span>{#if s.saveError}<button class="btn" onclick={() => s.save()}>Retry save</button>{:else if !s.loaded}<button class="btn" onclick={() => s.load()}>Try again</button>{/if}</div>
{/if}
{#if s.status?.hotkeyError}<div class="notice" role="status"><Keyboard size={18} /><span>{s.status.hotkeyError}</span></div>{/if}

{#if s.loading}<p class="loading" aria-busy="true">Loading your profiles…</p>{/if}

{#if profile && config && s.profiles}
  <div class="clicker-container">
    <!-- Top Global Bar: Mode Switch on the left, Profile on the right -->
    <header class="top-control-bar surface" aria-label="Clicker modes and profile">
      <div class="mode-track" role="group" aria-label="Clicker mode">
        <button type="button" class="mode-tab" class:chosen={config.mode === "simple"} aria-pressed={config.mode === "simple"} disabled={s.busy} onclick={() => s.edit({ mode: "simple" })}>
          <div class="mode-icon-pill"><MousePointer2 size={16} /></div>
          <div class="mode-info">
            <strong>Simple clicker</strong>
            <small>Follows cursor</small>
          </div>
        </button>

        <button type="button" class="mode-tab" class:chosen={config.mode === "sequence"} aria-pressed={config.mode === "sequence"} disabled={s.busy} onclick={() => s.edit({ mode: "sequence" })}>
          <div class="mode-icon-pill"><Route size={16} /></div>
          <div class="mode-info">
            <strong>Multi-point sequence</strong>
            <small>Ordered coordinates</small>
          </div>
        </button>
      </div>

      <div class="profile-toolbar">
        <div class="profile-meta">
          <div class="profile-label">
            <Layers size={13} />
            <span>Profile</span>
          </div>
          <div class="save-status-indicator" role="status">
            <span class="status-pulse-dot" class:dirty={s.dirty} class:saving={s.saving}></span>
            <span class="status-text">{s.saving ? "Saving…" : s.validation ? "Check settings" : s.dirty ? "Unsaved" : "Saved"}</span>
          </div>
        </div>

        <div class="profile-actions-row">
          <div class="profile-select-wrap">
            <Select value={profile.id} options={s.profiles.items.map(p => ({ value: p.id, label: p.name }))} disabled={s.busy || !!s.validation} minWidth="150px" onchange={id => { s.selectProfile(id); renaming = false; }} ariaLabel="Active clicker profile" />
          </div>
          <div class="profile-action-buttons">
            <button class="tactile-icon-btn" disabled={s.busy || !!s.validation || s.profiles.items.length >= 100} onclick={() => { s.addProfile(); renaming = false; }} title="Create a new profile" aria-label="New profile">
              <Plus size={14} />
            </button>
            <button class="tactile-icon-btn" disabled={s.busy} aria-label="Rename profile" title="Rename profile" onclick={() => { name = profile.name; renaming = !renaming; }}>
              <Pencil size={14} />
            </button>
            <button class="tactile-icon-btn delete-btn" disabled={s.busy || s.profiles.items.length === 1} aria-label="Delete profile" title="Delete profile" onclick={removeProfile}>
              <Trash2 size={14} />
            </button>
          </div>
        </div>

        {#if renaming}
          <form class="inline-rename-form" onsubmit={e => { e.preventDefault(); s.renameProfile(name); renaming = false; }}>
            <input class="inline-rename-input" aria-label="Profile name" bind:value={name} maxlength="80" required disabled={s.busy} placeholder="Profile name..." />
            <button class="btn primary tactile-icon-btn save-name-btn" disabled={s.busy || !name.trim()} title="Save name"><Check size={13} /></button>
            <button type="button" class="btn ghost tactile-icon-btn" onclick={() => renaming = false} title="Cancel">✕</button>
          </form>
        {/if}
      </div>
    </header>

    {#if s.validation}<div class="validation" role="status"><CircleAlert size={16} />{s.validation}</div>{/if}

    <!-- MODE 1: Simple Clicker (Unified All-in-One Studio Card) -->
    {#if config.mode === "simple"}
      <div class="simple-studio surface">
        <!-- Studio Header -->
        <div class="studio-header">
          <div class="studio-header-left">
            <div class="studio-icon-bubble"><MousePointer2 size={18} /></div>
            <div class="studio-header-text">
              <h2>Simple Clicker Configuration</h2>
              <p>Triggers rapid clicks wherever your mouse cursor points on any monitor.</p>
            </div>
          </div>

          <div class="studio-header-badges">
            {#if cps}
              <div class="cps-badge" title="Estimated clicks per second">
                <Zap size={13} />
                <span><strong>{cps}</strong> CPS</span>
              </div>
            {/if}
            <div class="cursor-badge" title="Dynamic cursor tracking">
              <Crosshair size={13} />
              <span>Active Cursor</span>
            </div>
          </div>
        </div>

        <!-- Studio Main 2-Column Body -->
        <div class="studio-body-grid">
          <!-- Column 1: Timing & Rate -->
          <section class="studio-column timing-column" aria-label="Click timing and rate">
            <div class="column-title">
              <Timer size={15} />
              <span>Click Timing & Rate</span>
            </div>

            <div class="form-row-2col">
              <div class="form-group">
                <span class="group-label">Interval Value</span>
                <div class="stepper-box full-stepper">
                  <button type="button" class="stepper-step-btn" disabled={s.busy || config.intervalMs <= 10} onclick={() => s.edit({ intervalMs: Math.max(10, (config.intervalMs || 100) - (unit === "seconds" ? 1000 : 10)) })} title="Decrease interval"><Minus size={13} /></button>
                  <input
                    id="click-interval"
                    aria-label="Click interval"
                    disabled={s.busy}
                    class="interval-number-input"
                    type="number"
                    min={unit === "ms" ? 10 : 0.01}
                    max={unit === "ms" ? 86400000 : 86400}
                    step={unit === "ms" ? 1 : 0.001}
                    value={Number.isFinite(config.intervalMs) ? config.intervalMs / (unit === "seconds" ? 1000 : 1) : undefined}
                    oninput={e => s.edit({ intervalMs: intervalInMs(e.currentTarget.valueAsNumber, unit) ?? NaN })}
                  />
                  <button type="button" class="stepper-step-btn" disabled={s.busy} onclick={() => s.edit({ intervalMs: (config.intervalMs || 100) + (unit === "seconds" ? 1000 : 10) })} title="Increase interval"><Plus size={13} /></button>
                </div>
              </div>

              <div class="form-group">
                <span class="group-label">Time Unit</span>
                <Select value={unit} options={[{ value: "ms", label: "Milliseconds" }, { value: "seconds", label: "Seconds" }]} disabled={s.busy} fullWidth={true} onchange={v => unit = v} ariaLabel="Interval unit" />
              </div>
            </div>

            <div class="presets-block">
              <span class="group-label">Quick Presets</span>
              <div class="presets-grid">
                {#each PRESETS as preset}
                  {@const isCurrent = Math.abs(config.intervalMs - preset.ms) < 1}
                  <button type="button" class="preset-chip" class:active={isCurrent} disabled={s.busy} onclick={() => setPreset(preset.ms)}>
                    <span class="preset-label">{preset.label}</span>
                    <span class="preset-cps">{preset.ms < 1000 ? `${Math.round(1000 / preset.ms)} cps` : "1 cps"}</span>
                  </button>
                {/each}
              </div>
            </div>
          </section>

          <!-- Column 2: Button, Click Type & Repeat Counts -->
          <section class="studio-column behavior-column" aria-label="Mouse button and repeat settings">
            <div class="column-title">
              <MousePointer2 size={15} />
              <span>Mouse Button & Repetition</span>
            </div>

            <div class="form-row-2col">
              <div class="form-group">
                <span class="group-label">Mouse Button</span>
                <Select value={config.button} options={BUTTON_OPTIONS} disabled={s.busy} fullWidth={true} onchange={v => s.edit({ button: v as Config["button"] })} ariaLabel="Mouse button" />
              </div>

              <div class="form-group">
                <span class="group-label">Click Type</span>
                <Select value={config.clickType} options={CLICK_TYPE_OPTIONS} disabled={s.busy} fullWidth={true} onchange={v => s.edit({ clickType: v as Config["clickType"] })} ariaLabel="Click type" />
              </div>
            </div>

            <div class="repeat-block">
              <div class="repeat-heading-row">
                <span class="group-label">Repeat Limit</span>
                <span class="repeat-mode-pill">{config.loops !== null ? `${config.loops} clicks target` : "Infinite loop"}</span>
              </div>

              <div class="repeat-seg-track" role="radiogroup" aria-label="Repeat mode">
                <button type="button" role="radio" aria-checked={config.loops !== null} class:active={config.loops !== null} disabled={s.busy} onclick={() => s.edit({ loops: config.loops ?? 50 })}>
                  <Layers size={13} />
                  <span>Specific count</span>
                </button>
                <button type="button" role="radio" aria-checked={config.loops === null} class:active={config.loops === null} disabled={s.busy} onclick={() => s.edit({ loops: null })}>
                  <Repeat2 size={13} />
                  <span>Until stopped</span>
                </button>
              </div>

              {#if config.loops !== null}
                <div class="repeat-count-details">
                  <div class="stepper-box compact-clean full-stepper">
                    <button type="button" class="stepper-step-btn" disabled={s.busy || (config.loops ?? 1) <= 1} onclick={() => s.edit({ loops: Math.max(1, (config.loops ?? 1) - 1) })} title="Decrease count">
                      <Minus size={13} />
                    </button>
                    <input
                      class="stepper-clean-input"
                      aria-label="Total clicks"
                      type="number"
                      min="1"
                      max="1000000"
                      step="1"
                      disabled={s.busy}
                      value={config.loops ?? 1}
                      oninput={e => s.edit({ loops: Math.max(1, e.currentTarget.valueAsNumber || 1) })}
                    />
                    <button type="button" class="stepper-step-btn" disabled={s.busy} onclick={() => s.edit({ loops: (config.loops ?? 1) + 1 })} title="Increase count">
                      <Plus size={13} />
                    </button>
                  </div>

                  <div class="quick-count-chips">
                    {#each [10, 50, 100, 500, 1000] as count}
                      <button type="button" class="count-chip" class:active={config.loops === count} disabled={s.busy} onclick={() => s.edit({ loops: count })}>
                        {count}
                      </button>
                    {/each}
                  </div>
                </div>
              {:else}
                <div class="repeat-infinite-box">
                  <Zap size={14} class="tip-icon" />
                  <span>Clicks continuously until <strong>F6</strong> or the Stop button is pressed.</span>
                </div>
              {/if}
            </div>
          </section>
        </div>

        <!-- Studio Unified Action & Status Footer -->
        <footer class="studio-footer">
          <div class="studio-footer-status">
            <div class="status-header-line">
              <span class="run-light" class:live={s.busy}></span>
              <strong class="status-title">{s.busy ? "Active Session" : "Status"}</strong>
              <span class="status-summary" role="status">{title}</span>
            </div>

            {#if hasProgress}
              <div class="progress-wrap">
                <ProgressBar value={s.status?.phase === "countdown" ? null : progress} done={s.status?.phase === "completed"} label="Clicker progress" />
                <div class="progress-meta">
                  <span><strong>{s.status?.completed.toLocaleString()}</strong> completed</span>
                  <span>{s.status?.total ? `${Math.round((progress ?? 0) * 100)}%` : "Continuous"}</span>
                </div>
              </div>
            {:else if !s.busy}
              <div class="countdown-note">
                <Timer size={13} />
                <span>3-second countdown gives you time to focus your target window.</span>
              </div>
            {/if}
          </div>

          <div class="studio-footer-cta">
            {#if s.busy}
              <button class="btn danger run-cta glow-danger-cta" disabled={s.status?.phase === "stopping"} onclick={() => s.stop()}>
                <Square size={17} fill="currentColor" />
                <span>Stop Clicker</span>
                <kbd>{s.status?.hotkey ? "F6" : ""}</kbd>
              </button>
            {:else}
              <button class="btn primary run-cta glow-primary-cta" disabled={!!s.validation || !s.loaded} onclick={() => s.start()}>
                <Play size={18} fill="currentColor" />
                <span>Start Clicker</span>
                <kbd>{s.status?.hotkey ? "F6" : ""}</kbd>
              </button>
            {/if}

            <div class="hotkey-status-row">
              <Keyboard size={12} />
              <span>{s.status?.hotkey ? "F6 active globally · Press to start or stop" : s.status?.hotkeyError ? "F6 unavailable (use button)" : "Registering hotkey…"}</span>
            </div>
          </div>
        </footer>
      </div>

    <!-- MODE 2: Multi-point Sequence -->
    {:else}
      <div class="sequence-workspace">
        <!-- Sequence Command & Control Header Bar -->
        <div class="sequence-control-panel surface">
          <div class="seq-panel-left">
            <div class="seq-icon-pill"><Route size={18} /></div>
            <div class="seq-panel-text">
              <h2>Recorded Points Sequence</h2>
              <p>Coordinates are clicked in order before repeating or concluding.</p>
            </div>
            <span class="counter-badge">{config.steps.length} / 128 points</span>
          </div>

          <!-- Sequence Loop & Playback Controls -->
          <div class="seq-panel-right">
            <div class="seq-loop-group">
              <div class="seq-loop-toggle" role="radiogroup" aria-label="Sequence repeat mode">
                <button type="button" role="radio" aria-checked={config.loops !== null} class:active={config.loops !== null} disabled={s.busy} onclick={() => s.edit({ loops: config.loops ?? 50 })}>
                  <Layers size={12} />
                  <span>Fixed loops</span>
                </button>
                <button type="button" role="radio" aria-checked={config.loops === null} class:active={config.loops === null} disabled={s.busy} onclick={() => s.edit({ loops: null })}>
                  <Repeat2 size={12} />
                  <span>Until stopped</span>
                </button>
              </div>

              {#if config.loops !== null}
                <div class="stepper-box compact-clean seq-loop-stepper">
                  <button type="button" class="stepper-step-btn" disabled={s.busy || (config.loops ?? 1) <= 1} onclick={() => s.edit({ loops: Math.max(1, (config.loops ?? 1) - 1) })} title="Decrease loops"><Minus size={12} /></button>
                  <input
                    class="stepper-clean-input"
                    aria-label="Total sequence loops"
                    type="number"
                    min="1"
                    max="1000000"
                    step="1"
                    disabled={s.busy}
                    value={config.loops ?? 1}
                    oninput={e => s.edit({ loops: Math.max(1, e.currentTarget.valueAsNumber || 1) })}
                  />
                  <button type="button" class="stepper-step-btn" disabled={s.busy} onclick={() => s.edit({ loops: (config.loops ?? 1) + 1 })} title="Increase loops"><Plus size={12} /></button>
                </div>
              {/if}
            </div>

            <!-- Sequence CTA Button -->
            <div class="seq-cta-wrap">
              {#if pickActive}
                <div class="seq-picking-pill" role="status">
                  <Crosshair size={14} />
                  <span>Click outside to record location…</span>
                  <button class="tactile-icon-btn cancel-pick-btn" onclick={() => s.stop()} title="Cancel picking">✕</button>
                </div>
              {:else if s.busy}
                <button class="btn danger run-cta seq-cta glow-danger-cta" disabled={s.status?.phase === "stopping"} onclick={() => s.stop()}>
                  <Square size={16} fill="currentColor" />
                  <span>Stop Sequence</span>
                  <kbd>{s.status?.hotkey ? "F6" : ""}</kbd>
                </button>
              {:else}
                <button class="btn primary run-cta seq-cta glow-primary-cta" disabled={!!s.validation || !s.loaded} onclick={() => s.start()}>
                  <Play size={17} fill="currentColor" />
                  <span>Start Sequence</span>
                  <kbd>{s.status?.hotkey ? "F6" : ""}</kbd>
                </button>
              {/if}
            </div>
          </div>
        </div>

        <!-- Live Sequence Progress Banner -->
        {#if hasProgress}
          <div class="sequence-progress-strip surface">
            <div class="seq-prog-info">
              <span class="run-light live"></span>
              <strong>{title}</strong>
              <span class="seq-prog-meta">{s.status?.completed.toLocaleString()} clicks · Loop {s.status?.loop || 1}{s.status?.loops === null ? "" : ` / ${s.status?.loops}`}</span>
            </div>
            <ProgressBar value={s.status?.phase === "countdown" ? null : progress} done={s.status?.phase === "completed"} label="Sequence progress" />
          </div>
        {/if}

        <!-- Steps Grid: Responsive tiling cards without dead space -->
        <div class="sequence-steps-section">
          <div class="steps-grid">
            {#each config.steps as step, index (step.id)}
              <StepEditor
                {step}
                {index}
                count={config.steps.length}
                disabled={s.busy}
                active={activeStep === index}
                picking={s.pickingId === step.id}
                onchange={change => s.updateStep(step.id, change)}
                onpick={() => s.pick(step.id)}
                onremove={() => s.removeStep(step.id)}
                onmove={direction => s.moveStep(index, direction)}
              />
            {/each}
          </div>

          <button class="btn add-step-cta" disabled={s.busy || config.steps.length >= 128} onclick={() => s.addStep()}>
            <Plus size={16} /><span>Add sequence point ({config.steps.length + 1} of 128)</span>
          </button>
        </div>

        <div class="hotkey-status-row seq-hotkey-footer">
          <Keyboard size={12} />
          <span>{s.status?.hotkey ? "F6 active globally · Press to start or stop" : s.status?.hotkeyError ? "F6 unavailable (use button)" : "Registering hotkey…"}</span>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  /* Base reset for number inputs */
  .clicker-container input[type="number"]::-webkit-outer-spin-button,
  .clicker-container input[type="number"]::-webkit-inner-spin-button { -webkit-appearance: none; margin: 0; }
  .clicker-container input[type="number"] { appearance: textfield; }

  .notice { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-radius: 10px; background: var(--surface-fill); border: 1px solid var(--btn-border); margin-bottom: 14px; font-size: 13px; }
  .notice.error { border-color: rgba(229, 72, 77, 0.4); color: #ffb4b0; }
  .loading { padding: 28px 0; color: var(--text-3); font-size: 14px; }
  .validation { display: flex; align-items: center; gap: 8px; padding: 10px 14px; margin-bottom: 14px; border-radius: 8px; background: rgb(var(--accent-rgb) / 0.1); border: 1px solid rgb(var(--accent-rgb) / 0.3); color: rgb(var(--accent-soft-rgb)); font-size: 12.5px; }

  .clicker-container { display: flex; flex-direction: column; gap: 16px; width: 100%; }

  /* -------------------------------------------------------------
     TOP GLOBAL CONTROL BAR (Mode selector + Profile manager)
  ------------------------------------------------------------- */
  .top-control-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 8px 14px;
    border-radius: 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.04) 0%, rgba(255, 255, 255, 0.015) 100%);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    flex-wrap: wrap;
  }

  /* Mode track */
  .mode-track {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px;
    border-radius: 10px;
    background: var(--input-fill, rgba(0, 0, 0, 0.35));
    border: 1px solid var(--glass-border, rgba(255, 255, 255, 0.07));
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4);
  }
  .mode-tab {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 14px;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-3);
    text-align: left;
    cursor: pointer;
    transition: all 0.18s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .mode-tab:not(.chosen):hover:not(:disabled) {
    color: var(--text-1);
    background: var(--hover, rgba(255, 255, 255, 0.04));
    border-color: rgb(var(--accent-rgb) / 0.25);
  }
  .mode-tab.chosen {
    background: linear-gradient(180deg, rgb(var(--accent-rgb) / 0.22) 0%, rgb(var(--accent-rgb) / 0.08) 100%);
    border-color: rgb(var(--accent-rgb) / 0.75);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.2), 0 0 12px rgb(var(--accent-rgb) / 0.25);
    color: var(--text-1);
  }
  .mode-icon-pill {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-3);
    flex-shrink: 0;
    transition: all 0.18s ease;
  }
  .mode-tab.chosen .mode-icon-pill {
    background: rgb(var(--accent-rgb) / 0.28);
    color: rgb(var(--accent-soft-rgb));
    box-shadow: 0 0 10px rgb(var(--accent-rgb) / 0.4);
  }
  .mode-info strong { display: block; font-size: 13px; line-height: 1.2; }
  .mode-tab.chosen .mode-info strong { color: #ffffff; }
  .mode-info small { display: block; font-size: 10.5px; color: var(--text-3); margin-top: 1px; }
  .mode-tab.chosen .mode-info small { color: rgb(var(--accent-soft-rgb) / 0.9); }

  /* Profile Toolbar */
  .profile-toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    position: relative;
  }
  .profile-meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-items: flex-end;
  }
  .profile-label {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    font-weight: 700;
    color: rgb(var(--accent-soft-rgb));
  }
  .save-status-indicator {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    color: var(--text-3);
  }
  .status-pulse-dot { width: 6px; height: 6px; border-radius: 50%; background: #4ade80; box-shadow: 0 0 6px rgba(74, 222, 128, 0.6); }
  .status-pulse-dot.dirty { background: #facc15; box-shadow: 0 0 6px rgba(250, 204, 21, 0.6); }
  .status-pulse-dot.saving { background: rgb(var(--accent-rgb)); box-shadow: 0 0 6px var(--accent-glow); animation: da-pulse 1s infinite alternate; }
  @keyframes da-pulse { from { opacity: 0.4; } to { opacity: 1; } }

  .profile-actions-row { display: flex; align-items: center; gap: 6px; }
  .profile-action-buttons { display: flex; align-items: center; gap: 4px; }
  .tactile-icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 7px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.05) 0%, rgba(255, 255, 255, 0.01) 100%);
    color: var(--text-2);
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .tactile-icon-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); border-color: rgba(255, 255, 255, 0.2); }
  .tactile-icon-btn.delete-btn:hover:not(:disabled) { color: var(--danger); border-color: rgba(229, 72, 77, 0.4); background: rgba(229, 72, 77, 0.12); }
  .tactile-icon-btn.save-name-btn { background: var(--accent-grad); color: #fff; border-color: rgba(255, 255, 255, 0.2); }

  .inline-rename-form {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 6px;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 6px 8px;
    border-radius: 8px;
    background: rgba(18, 18, 18, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.15);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  .inline-rename-input {
    width: 140px;
    height: 28px;
    padding: 0 8px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: rgba(0, 0, 0, 0.4);
    color: var(--text-1);
    font-size: 12px;
  }

  /* -------------------------------------------------------------
     MODE 1: SIMPLE CLICKER STUDIO (ALL-IN-ONE CARD)
  ------------------------------------------------------------- */
  .simple-studio {
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.035) 0%, rgba(255, 255, 255, 0.01) 100%);
    box-shadow: 0 6px 28px rgba(0, 0, 0, 0.3);
    overflow: hidden;
  }

  .studio-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 18px 22px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(255, 255, 255, 0.015);
  }
  .studio-header-left { display: flex; align-items: center; gap: 12px; }
  .studio-icon-bubble {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 9px;
    background: rgb(var(--accent-rgb) / 0.18);
    border: 1px solid rgb(var(--accent-rgb) / 0.35);
    color: rgb(var(--accent-soft-rgb));
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.2);
    flex-shrink: 0;
  }
  .studio-header-text h2 { margin: 0; font-size: 15px; font-weight: 700; color: var(--text-1); }
  .studio-header-text p { margin: 2px 0 0; font-size: 11.5px; color: var(--text-3); }

  .studio-header-badges { display: flex; align-items: center; gap: 8px; }
  .cps-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    border-radius: 7px;
    background: rgb(var(--accent-rgb) / 0.18);
    border: 1px solid rgb(var(--accent-rgb) / 0.45);
    color: rgb(var(--accent-soft-rgb));
    font-size: 11.5px;
    font-weight: 700;
    box-shadow: 0 0 10px rgb(var(--accent-rgb) / 0.25);
  }
  .cursor-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.09);
    color: var(--text-2);
    font-size: 11.5px;
    font-weight: 600;
  }

  /* 2-Column Body Grid */
  .studio-body-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 22px;
    padding: 22px;
  }
  .studio-column {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px 18px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.22);
    border: 1px solid rgba(255, 255, 255, 0.05);
  }
  .column-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--text-1);
    padding-bottom: 8px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  }
  .column-title :global(svg) { color: rgb(var(--accent-soft-rgb)); }

  /* 2-Column Form Rows */
  .form-row-2col { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; align-items: start; }
  .form-group { display: flex; flex-direction: column; gap: 6px; }
  .group-label { font-size: 11.5px; font-weight: 600; color: var(--text-2); }

  /* Steppers */
  .stepper-box {
    display: inline-flex;
    align-items: center;
    height: 36px;
    border-radius: 9px;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
    padding: 2px;
  }
  .stepper-box.full-stepper { width: 100%; }
  .stepper-step-btn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 100%;
    border-radius: 6px;
    border: none;
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-2);
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }
  .stepper-step-btn:hover:not(:disabled) { background: rgba(255, 255, 255, 0.12); color: var(--text-1); }
  .interval-number-input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: none;
    background: transparent;
    text-align: center;
    font-size: 15px;
    font-weight: 700;
    color: var(--text-1);
    box-shadow: none;
    font-variant-numeric: tabular-nums;
  }

  .stepper-box.compact-clean {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 36px;
    padding: 3px;
    background: var(--input-fill, #121212);
    border: 1px solid var(--panel-rim, rgba(255, 255, 255, 0.1));
    border-radius: 9px;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.35);
  }
  .stepper-box.compact-clean:focus-within {
    border-color: rgb(var(--accent-rgb) / 0.6);
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.35), 0 0 0 2px rgb(var(--accent-rgb) / 0.15);
  }
  .stepper-clean-input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: none !important;
    background: transparent !important;
    text-align: center;
    font-size: 14.5px;
    font-weight: 700;
    color: var(--text-1);
    box-shadow: none !important;
    outline: none !important;
    padding: 0 !important;
    margin: 0 !important;
    font-variant-numeric: tabular-nums;
  }

  /* Presets */
  .presets-block { display: flex; flex-direction: column; gap: 7px; }
  .presets-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
    padding: 4px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.32);
    box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.5), inset 0 0 0 1px rgba(255, 255, 255, 0.04);
  }
  .preset-chip {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1px;
    height: 42px;
    border-radius: 7px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.08) 0%, rgba(255, 255, 255, 0.02) 100%);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16), 0 2px 4px rgba(0, 0, 0, 0.3);
    color: var(--text-2);
    cursor: pointer;
    transition: all 0.14s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .preset-chip:not(.active):hover:not(:disabled) {
    color: var(--text-1);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.14) 0%, rgba(255, 255, 255, 0.04) 100%);
    transform: translateY(-1px);
  }
  .preset-chip.active {
    background: var(--accent-grad) !important;
    border: 1px solid rgba(255, 255, 255, 0.28);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.45), 0 2px 8px rgba(0, 0, 0, 0.4), 0 0 14px rgb(var(--accent-rgb) / 0.55);
    color: #ffffff !important;
  }
  .preset-label { font-size: 11.5px; font-weight: 700; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6); }
  .preset-cps { font-size: 9.5px; font-weight: 600; color: var(--text-3); text-shadow: 0 1px 1px rgba(0, 0, 0, 0.5); }
  .preset-chip.active .preset-cps { color: rgba(255, 255, 255, 0.95) !important; font-weight: 700; }

  /* Repeat section */
  .repeat-block { display: flex; flex-direction: column; gap: 7px; }
  .repeat-heading-row { display: flex; align-items: center; justify-content: space-between; }
  .repeat-mode-pill { font-size: 10.5px; font-weight: 700; color: rgb(var(--accent-soft-rgb)); }
  .repeat-seg-track {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 3px;
    padding: 3px;
    border-radius: 9px;
    background: var(--input-fill);
    border: 1px solid var(--panel-rim);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.3);
  }
  .repeat-seg-track button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 32px;
    padding: 0 8px;
    border-radius: 7px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-3);
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .repeat-seg-track button:not(.active):hover:not(:disabled) { color: var(--text-1); background: var(--hover); }
  .repeat-seg-track button.active {
    background: var(--btn-fill);
    border-color: var(--btn-border);
    color: rgb(var(--accent-soft-rgb));
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.25);
  }

  .repeat-count-details { display: flex; flex-direction: column; gap: 6px; margin-top: 2px; }
  .quick-count-chips { display: grid; grid-template-columns: repeat(5, 1fr); gap: 4px; }
  .count-chip {
    height: 24px;
    padding: 0 4px;
    border-radius: 5px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.035);
    color: var(--text-3);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .count-chip:not(.active):hover:not(:disabled) { background: rgba(255, 255, 255, 0.09); color: var(--text-1); }
  .count-chip.active {
    background: var(--accent-grad) !important;
    border-color: rgba(255, 255, 255, 0.25);
    color: #ffffff !important;
    font-weight: 700;
    box-shadow: 0 0 8px rgb(var(--accent-rgb) / 0.4);
  }

  .repeat-infinite-box {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    border-radius: 8px;
    background: rgb(var(--accent-rgb) / 0.08);
    border: 1px solid rgb(var(--accent-rgb) / 0.2);
    color: var(--text-2);
    font-size: 12px;
  }
  .repeat-infinite-box strong { color: rgb(var(--accent-soft-rgb)); }
  :global(.tip-icon) { color: rgb(var(--accent-soft-rgb)); flex-shrink: 0; }

  /* Studio Footer */
  .studio-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 18px 24px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(0, 0, 0, 0.25);
  }
  .studio-footer-status { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .status-header-line { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .status-title { color: var(--text-1); font-weight: 700; }
  .status-summary { color: var(--text-3); font-variant-numeric: tabular-nums; }
  .run-light { width: 7px; height: 7px; border-radius: 50%; background: var(--idle, #64748b); flex-shrink: 0; }
  .run-light.live { background: #4ade80; box-shadow: 0 0 8px rgba(74, 222, 128, 0.8); }

  .progress-wrap { display: flex; flex-direction: column; gap: 4px; max-width: 440px; }
  .progress-meta { display: flex; justify-content: space-between; font-size: 11px; color: var(--text-3); }
  .countdown-note { display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-3); }

  .studio-footer-cta { display: flex; flex-direction: column; align-items: flex-end; gap: 6px; min-width: 240px; }
  .run-cta {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 9px;
    width: 100%;
    height: 44px;
    border-radius: 10px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.15s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .glow-primary-cta {
    background: var(--accent-grad);
    border: 1px solid rgba(255, 255, 255, 0.25);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.38), 0 3px 14px rgb(var(--accent-rgb) / 0.45);
    color: var(--btn-primary-text, #ffffff);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);
  }
  .glow-primary-cta:hover:not(:disabled) { filter: brightness(1.1); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.45), 0 4px 18px rgb(var(--accent-rgb) / 0.65); transform: translateY(-0.5px); }
  .glow-danger-cta {
    background: linear-gradient(180deg, #e5484d 0%, #b82a2e 100%);
    border: 1px solid rgba(255, 255, 255, 0.25);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35), 0 3px 12px rgba(229, 72, 77, 0.45);
    color: #ffffff;
  }
  .run-cta kbd { margin-left: auto; padding: 2px 7px; border-radius: 5px; background: rgba(0, 0, 0, 0.25); font-size: 11px; font-weight: 700; border: 1px solid rgba(255, 255, 255, 0.15); }
  .hotkey-status-row { display: flex; align-items: center; gap: 5px; font-size: 11px; color: var(--text-3); }

  /* -------------------------------------------------------------
     MODE 2: MULTI-POINT SEQUENCE WORKSPACE
  ------------------------------------------------------------- */
  .sequence-workspace { display: flex; flex-direction: column; gap: 16px; }

  .sequence-control-panel {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 16px 20px;
    border-radius: 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.035) 0%, rgba(255, 255, 255, 0.015) 100%);
    box-shadow: 0 4px 18px rgba(0, 0, 0, 0.25);
    flex-wrap: wrap;
  }
  .seq-panel-left { display: flex; align-items: center; gap: 12px; }
  .seq-icon-pill {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 8px;
    background: rgb(var(--accent-rgb) / 0.18);
    border: 1px solid rgb(var(--accent-rgb) / 0.35);
    color: rgb(var(--accent-soft-rgb));
    flex-shrink: 0;
  }
  .seq-panel-text h2 { margin: 0; font-size: 14.5px; font-weight: 700; color: var(--text-1); }
  .seq-panel-text p { margin: 2px 0 0; font-size: 11px; color: var(--text-3); }
  .counter-badge {
    padding: 3px 9px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.08);
    font-size: 11.5px;
    color: var(--text-2);
    font-weight: 600;
    white-space: nowrap;
  }

  .seq-panel-right { display: flex; align-items: center; gap: 16px; flex-wrap: wrap; }
  .seq-loop-group { display: flex; align-items: center; gap: 8px; }
  .seq-loop-toggle {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 3px;
    border-radius: 8px;
    background: var(--input-fill);
    border: 1px solid var(--panel-rim);
  }
  .seq-loop-toggle button {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 8px;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-3);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .seq-loop-toggle button.active {
    background: var(--btn-fill);
    border-color: var(--btn-border);
    color: rgb(var(--accent-soft-rgb));
  }
  .seq-loop-stepper { width: 100px; height: 32px; }

  .seq-cta-wrap { min-width: 170px; }
  .seq-cta { height: 38px; font-size: 13px; }
  .seq-picking-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-radius: 8px;
    background: rgb(var(--accent-rgb) / 0.15);
    border: 1px dashed rgb(var(--accent-rgb) / 0.5);
    color: rgb(var(--accent-soft-rgb));
    font-size: 12px;
    font-weight: 600;
  }
  .cancel-pick-btn { width: 22px; height: 22px; font-size: 11px; }

  .sequence-progress-strip {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 18px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.28);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }
  .seq-prog-info { display: flex; align-items: center; gap: 8px; font-size: 12.5px; }
  .seq-prog-meta { margin-left: auto; font-size: 11px; color: var(--text-3); }

  /* Responsive Steps Grid */
  .sequence-steps-section { display: flex; flex-direction: column; gap: 14px; }
  .steps-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 460px), 1fr));
    gap: 14px;
    width: 100%;
  }
  .add-step-cta {
    width: 100%;
    height: 42px;
    border-radius: 10px;
    border: 1px dashed rgba(255, 255, 255, 0.2);
    background: rgba(255, 255, 255, 0.02);
    color: var(--text-2);
    font-size: 13px;
    font-weight: 600;
    gap: 8px;
    cursor: pointer;
    transition: all 0.15s ease;
  }
  .add-step-cta:hover:not(:disabled) {
    border-color: rgb(var(--accent-rgb) / 0.6);
    background: rgb(var(--accent-rgb) / 0.08);
    color: var(--text-1);
    transform: translateY(-0.5px);
  }
  .seq-hotkey-footer { justify-content: center; padding-top: 4px; }

  /* -------------------------------------------------------------
     MEDIA QUERIES FOR COMPACT & LARGE SCREENS
  ------------------------------------------------------------- */
  @media (max-width: 900px) {
    .top-control-bar { flex-direction: column; align-items: stretch; gap: 10px; }
    .mode-track { width: 100%; }
    .mode-tab { flex: 1; justify-content: center; }
    .profile-toolbar { justify-content: space-between; }
    .studio-body-grid { grid-template-columns: 1fr; gap: 16px; }
    .studio-footer { flex-direction: column; align-items: stretch; gap: 16px; }
    .studio-footer-cta { align-items: stretch; min-width: 0; }
    .sequence-control-panel { flex-direction: column; align-items: stretch; gap: 14px; }
    .seq-panel-right { justify-content: space-between; }
    .steps-grid { grid-template-columns: 1fr; }
  }
</style>
