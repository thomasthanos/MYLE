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
  <div class="workspace">
    <section class="editor" aria-label="Click settings">
      <div class="mode-track" role="group" aria-label="Clicker mode">
        <button type="button" class="mode-card" class:chosen={config.mode === "simple"} aria-pressed={config.mode === "simple"} disabled={s.busy} onclick={() => s.edit({ mode: "simple" })}>
          <div class="mode-icon-pill"><MousePointer2 size={18} /></div>
          <div class="mode-text">
            <strong>Simple clicker</strong>
            <small>Clicks follow your mouse cursor</small>
          </div>
        </button>

        <button type="button" class="mode-card" class:chosen={config.mode === "sequence"} aria-pressed={config.mode === "sequence"} disabled={s.busy} onclick={() => s.edit({ mode: "sequence" })}>
          <div class="mode-icon-pill"><Route size={18} /></div>
          <div class="mode-text">
            <strong>Multi-point sequence</strong>
            <small>Executes recorded coordinates in order</small>
          </div>
        </button>
      </div>

      {#if config.mode === "simple"}
        <div class="simple-dashboard">
          <div class="simple-card timing-card surface">
            <div class="card-head">
              <div class="card-head-icon"><Timer size={16} /></div>
              <div class="card-head-text">
                <h3>Click Timing & Rate</h3>
                <p>Speed between simulated clicks</p>
              </div>
              {#if cps}
                <div class="card-head-badge" title="Estimated clicks per second">
                  <Zap size={12} />
                  <span><strong>{cps}</strong> CPS</span>
                </div>
              {/if}
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
          </div>

          <div class="simple-card behavior-card surface">
            <div class="card-head">
              <div class="card-head-icon"><MousePointer2 size={16} /></div>
              <div class="card-head-text">
                <h3>Click Behavior & Target</h3>
                <p>Simulated mouse button and coordinates</p>
              </div>
              <div class="card-head-badge" title="Dynamic cursor tracking">
                <Crosshair size={12} />
                <span><strong>Active</strong> Cursor</span>
              </div>
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

            <div class="position-block">
              <span class="group-label">Click Position</span>
              <div class="target-panel">
                <div class="target-main">
                  <div class="target-icon-wrap"><Crosshair size={18} /></div>
                  <div class="target-text">
                    <strong>Dynamic Cursor Tracking</strong>
                    <p>Clicks trigger wherever your mouse cursor is pointing on the screen.</p>
                  </div>
                </div>
                <div class="target-tags">
                  <span class="target-tag"><span class="tag-dot"></span> Screen Real-time</span>
                  <span class="target-tag">All Monitors</span>
                  <span class="target-tag hotkey-tag"><Keyboard size={10} /> F6 Toggle</span>
                </div>
              </div>
            </div>
          </div>
        </div>

      {:else}
        <div class="sequence-heading">
          <div>
            <h2>Recorded Points Sequence</h2>
            <p>Each point clicks its target coordinates before advancing to the next.</p>
          </div>
          <span class="counter-badge">{config.steps.length} / 128 points</span>
        </div>

        <div class="steps">
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
          <Plus size={16} /><span>Add sequence point</span>
        </button>
      {/if}

      {#if s.validation}<div class="validation" role="status"><CircleAlert size={16} />{s.validation}</div>{/if}
    </section>

    <aside class="run-panel surface" aria-label="Run controls">
      <div class="sidebar-profile-block">
        <div class="sidebar-profile-head">
          <div class="profile-badge">
            <Layers size={13} />
            <span>Profile</span>
          </div>
          <div class="save-status-indicator" role="status">
            <span class="status-pulse-dot" class:dirty={s.dirty} class:saving={s.saving}></span>
            <span class="status-text">{s.saving ? "Saving…" : s.validation ? "Complete settings" : s.dirty ? "Unsaved" : "Saved"}</span>
          </div>
        </div>

        <div class="sidebar-profile-row">
          <Select value={profile.id} options={s.profiles.items.map(p => ({ value: p.id, label: p.name }))} disabled={s.busy || !!s.validation} fullWidth={true} onchange={id => { s.selectProfile(id); renaming = false; }} ariaLabel="Active clicker profile" />
          <div class="profile-actions">
            <button class="icon-btn tactile-icon-btn" disabled={s.busy || !!s.validation || s.profiles.items.length >= 100} onclick={() => { s.addProfile(); renaming = false; }} title="Create a new profile" aria-label="New profile">
              <Plus size={14} />
            </button>
            <button class="icon-btn tactile-icon-btn" disabled={s.busy} aria-label="Rename profile" title="Rename profile" onclick={() => { name = profile.name; renaming = !renaming; }}>
              <Pencil size={14} />
            </button>
            <button class="icon-btn tactile-icon-btn delete-btn" disabled={s.busy || s.profiles.items.length === 1} aria-label="Delete profile" title="Delete profile" onclick={removeProfile}>
              <Trash2 size={14} />
            </button>
          </div>
        </div>

        {#if renaming}
          <form class="sidebar-rename-form" onsubmit={e => { e.preventDefault(); s.renameProfile(name); renaming = false; }}>
            <input class="sidebar-rename-input" aria-label="Profile name" bind:value={name} maxlength="80" required disabled={s.busy} placeholder="Profile name..." />
            <button class="btn primary tactile-icon-btn save-name-btn" disabled={s.busy || !name.trim()} title="Save name"><Check size={13} /></button>
            <button type="button" class="btn ghost tactile-icon-btn" onclick={() => renaming = false} title="Cancel">✕</button>
          </form>
        {/if}
      </div>

      <div class="sidebar-divider"></div>

      <div class="run-heading">
        <span class="run-light" class:live={s.busy}></span>
        <h2>{s.busy ? "Active Session" : "Playback"}</h2>
        <span class="hotkey-pill"><Keyboard size={12} /> F6</span>
      </div>

      <fieldset class="repeat-section" disabled={s.busy}>
        <legend><Repeat2 size={14} /> <span>{config.mode === "simple" ? "Repeat Clicks" : "Repeat Sequence"}</span></legend>

        <div class="repeat-seg-track" role="radiogroup" aria-label="Repeat mode">
          <button type="button" role="radio" aria-checked={config.loops !== null} class:active={config.loops !== null} disabled={s.busy} onclick={() => s.edit({ loops: config.loops ?? 50 })}>
            <Layers size={13} />
            <span>{config.mode === "simple" ? "Specific count" : "Fixed loops"}</span>
          </button>
          <button type="button" role="radio" aria-checked={config.loops === null} class:active={config.loops === null} disabled={s.busy} onclick={() => s.edit({ loops: null })}>
            <Repeat2 size={13} />
            <span>Until stopped</span>
          </button>
        </div>

        {#if config.loops !== null}
          <div class="repeat-count-card">
            <div class="repeat-count-head">
              <span class="count-label">{config.mode === "simple" ? "Target Count" : "Loop Count"}</span>
              <span class="count-badge">{config.loops ?? 1} {config.mode === "simple" ? "clicks" : "loops"}</span>
            </div>

            <div class="stepper-box compact-clean full-stepper">
              <button type="button" class="stepper-step-btn" disabled={s.busy || (config.loops ?? 1) <= 1} onclick={() => s.edit({ loops: Math.max(1, (config.loops ?? 1) - 1) })} title="Decrease count">
                <Minus size={13} />
              </button>
              <input
                class="stepper-clean-input"
                aria-label={config.mode === "simple" ? "Total clicks" : "Total sequence loops"}
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
          <div class="repeat-infinite-row">
            <Zap size={13} class="tip-icon" />
            <span>Loops continuously until <strong>F6</strong> is pressed</span>
          </div>
        {/if}
      </fieldset>

      {#if pickActive}
        <div class="picking-box" role="status">
          <div class="reticle-anim"><Crosshair size={28} /></div>
          <strong>Location Capture Active</strong>
          <p>Click outside MYLE to record the coordinates. The target will not be clicked.</p>
          <small>Cancel here, or press Esc / F6 · 30 second limit</small>
        </div>
      {:else}
        <div class="run-status-card">
          <div class="status-title-row">
            <span class="status-caption">Progress</span>
            <span class="progress-title" role="status">{title}</span>
          </div>

          {#if hasProgress}
            <ProgressBar value={s.status?.phase === "countdown" ? null : progress} done={s.status?.phase === "completed"} label="Clicker progress" />
            <div class="progress-meta">
              <span><strong>{s.status?.completed.toLocaleString()}</strong> completed</span>
              <span>{s.status?.mode === "sequence" && (s.status.loop || s.status.phase === "running") ? `Loop ${s.status.loop || 1}${s.status.loops === null ? "" : ` / ${s.status.loops}`}` : s.status?.total ? `${Math.round((progress ?? 0) * 100)}%` : "Infinite"}</span>
            </div>
          {:else}
            <div class="countdown-note">
              <Timer size={14} />
              <span>3-second countdown gives you time to focus your target window.</span>
            </div>
          {/if}
        </div>
      {/if}

      {#if s.busy}
        <button class="btn danger run-cta glow-danger-cta" disabled={s.status?.phase === "stopping"} onclick={() => s.stop()}>
          <Square size={17} fill="currentColor" />
          <span>{pickActive ? "Cancel picking" : "Stop Clicker"}</span>
          <kbd>{s.status?.hotkey ? "F6" : ""}</kbd>
        </button>
      {:else}
        <button class="btn primary run-cta glow-primary-cta" disabled={!!s.validation || !s.loaded} onclick={() => s.start()}>
          <Play size={18} fill="currentColor" />
          <span>Start Clicker</span>
          <kbd>{s.status?.hotkey ? "F6" : ""}</kbd>
        </button>
      {/if}

      <div class="hotkey-status-footer">
        <Keyboard size={13} />
        <span>{s.status?.hotkey ? "F6 active globally · Press to start or stop" : s.status?.hotkeyError ? "F6 unavailable (click button)" : "Registering hotkey…"}</span>
      </div>
    </aside>
  </div>
{/if}

<style>
  .workspace input[type="number"]::-webkit-outer-spin-button,
  .workspace input[type="number"]::-webkit-inner-spin-button { -webkit-appearance: none; margin: 0; }
  .workspace input[type="number"] { appearance: textfield; }


  .notice { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-radius: 10px; background: var(--surface-fill); border: 1px solid var(--btn-border); margin-bottom: 14px; font-size: 13px; }
  .notice.error { border-color: rgba(229, 72, 77, 0.4); color: #ffb4b0; }

  .loading { padding: 28px 0; color: var(--text-3); font-size: 14px; }

  .sidebar-profile-block { display: flex; flex-direction: column; gap: 8px; margin-bottom: 4px; }
  .sidebar-profile-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .profile-badge { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; font-weight: 700; color: rgb(var(--accent-soft-rgb)); }
  .save-status-indicator { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-3); }
  .status-pulse-dot { width: 6px; height: 6px; border-radius: 50%; background: #4ade80; box-shadow: 0 0 6px rgba(74, 222, 128, 0.6); }
  .status-pulse-dot.dirty { background: #facc15; box-shadow: 0 0 6px rgba(250, 204, 21, 0.6); }
  .status-pulse-dot.saving { background: rgb(var(--accent-rgb)); box-shadow: 0 0 6px var(--accent-glow); animation: da-pulse 1s infinite alternate; }
  @keyframes da-pulse { from { opacity: 0.4; } to { opacity: 1; } }

  .sidebar-profile-row { display: flex; align-items: center; gap: 6px; }
  .sidebar-profile-row :global(.select) { flex: 1; min-width: 0; }
  .profile-actions { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .tactile-icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; border-radius: 7px; border: 1px solid rgba(255, 255, 255, 0.08); background: linear-gradient(180deg, rgba(255, 255, 255, 0.05) 0%, rgba(255, 255, 255, 0.01) 100%); color: var(--text-2); cursor: pointer; transition: all 0.15s ease; }
  .tactile-icon-btn:hover:not(:disabled) { background: var(--hover); color: var(--text-1); border-color: rgba(255, 255, 255, 0.2); }
  .tactile-icon-btn.delete-btn:hover:not(:disabled) { color: var(--danger); border-color: rgba(229, 72, 77, 0.4); background: rgba(229, 72, 77, 0.12); }
  .tactile-icon-btn.save-name-btn { background: var(--accent-grad); color: #fff; border-color: rgba(255, 255, 255, 0.2); }

  .sidebar-rename-form { display: flex; align-items: center; gap: 5px; margin-top: 4px; padding: 4px 6px; border-radius: 8px; background: rgba(0, 0, 0, 0.3); border: 1px solid rgba(255, 255, 255, 0.08); }
  .sidebar-rename-input { flex: 1; min-width: 0; height: 26px; padding: 0 8px; border-radius: 6px; border: 1px solid rgba(255, 255, 255, 0.1); background: rgba(0, 0, 0, 0.35); color: var(--text-1); font-size: 12px; }
  .sidebar-divider { height: 1px; background: rgba(255, 255, 255, 0.06); margin: 12px 0 14px; }

  .workspace { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 18px; align-items: start; }
  .editor { min-width: 0; }

  .mode-track { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; padding: 4px; border-radius: 12px; background: var(--input-fill, rgba(0, 0, 0, 0.35)); border: 1px solid var(--glass-border, rgba(255, 255, 255, 0.07)); box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.4); margin-bottom: 16px; }
  .mode-card { display: flex; align-items: center; gap: 12px; padding: 10px 14px; border-radius: 9px; border: 1px solid transparent; background: transparent; color: var(--text-3); text-align: left; cursor: pointer; transition: all 0.18s cubic-bezier(0.2, 0.8, 0.2, 1); }
  .mode-card:not(.chosen):hover:not(:disabled) { color: var(--text-1); background: var(--hover, rgba(255, 255, 255, 0.04)); border-color: rgb(var(--accent-rgb) / 0.25); }
  .mode-card.chosen:hover:not(:disabled) { background: linear-gradient(180deg, rgb(var(--accent-rgb) / 0.28) 0%, rgb(var(--accent-rgb) / 0.12) 100%); border-color: rgb(var(--accent-rgb) / 0.9); }
  .mode-card.chosen { background: linear-gradient(180deg, rgb(var(--accent-rgb) / 0.22) 0%, rgb(var(--accent-rgb) / 0.08) 100%); border-color: rgb(var(--accent-rgb) / 0.75); box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.2), 0 0 14px rgb(var(--accent-rgb) / 0.25); color: var(--text-1); }
  .mode-icon-pill { display: flex; align-items: center; justify-content: center; width: 32px; height: 32px; border-radius: 8px; background: rgba(255, 255, 255, 0.05); color: var(--text-3); flex-shrink: 0; transition: all 0.18s ease; }
  .mode-card.chosen .mode-icon-pill { background: rgb(var(--accent-rgb) / 0.28); color: rgb(var(--accent-soft-rgb)); box-shadow: 0 0 12px rgb(var(--accent-rgb) / 0.45); }
  .mode-text strong { display: block; font-size: 13.5px; }
  .mode-card.chosen .mode-text strong { color: #ffffff; }
  .mode-text small { display: block; font-size: 11px; color: var(--text-3); margin-top: 1px; }
  .mode-card.chosen .mode-text small { color: rgb(var(--accent-soft-rgb) / 0.9); }

  .simple-dashboard { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-bottom: 14px; width: 100%; }
  .simple-card { display: flex; flex-direction: column; gap: 16px; padding: 18px 20px; border-radius: 14px; background: linear-gradient(180deg, rgba(255, 255, 255, 0.035) 0%, rgba(255, 255, 255, 0.01) 100%); border: 1px solid rgba(255, 255, 255, 0.08); box-shadow: 0 4px 20px rgba(0, 0, 0, 0.25); }
  .card-head { display: flex; align-items: center; gap: 10px; padding-bottom: 12px; border-bottom: 1px solid rgba(255, 255, 255, 0.06); }
  .card-head-icon { display: grid; place-items: center; width: 32px; height: 32px; border-radius: 8px; background: rgb(var(--accent-rgb) / 0.15); color: rgb(var(--accent-soft-rgb)); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.15); flex-shrink: 0; }
  .card-head-text h3 { margin: 0; font-size: 14px; font-weight: 700; color: var(--text-1); }
  .card-head-text p { margin: 1px 0 0; font-size: 11px; color: var(--text-3); }

  .card-head-badge { margin-left: auto; display: inline-flex; align-items: center; gap: 5px; padding: 3px 9px; border-radius: 6px; background: rgb(var(--accent-rgb) / 0.18); border: 1px solid rgb(var(--accent-rgb) / 0.45); color: rgb(var(--accent-soft-rgb)); font-size: 11px; font-weight: 700; box-shadow: 0 0 8px rgb(var(--accent-rgb) / 0.25); white-space: nowrap; }
  .card-head-badge strong { color: #fff; font-size: 11.5px; }

  /* 2-Column form row for mathematical alignment */
  .form-row-2col { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; align-items: start; }
  .form-group { display: flex; flex-direction: column; gap: 6px; }
  .group-label { font-size: 11.5px; font-weight: 600; color: var(--text-2); }

  .stepper-box { display: inline-flex; align-items: center; height: 36px; border-radius: 9px; background: rgba(0, 0, 0, 0.35); border: 1px solid rgba(255, 255, 255, 0.1); box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4); padding: 2px; }
  .stepper-box.full-stepper { width: 100%; }
  .stepper-step-btn { display: grid; place-items: center; width: 32px; height: 100%; border-radius: 6px; border: none; background: rgba(255, 255, 255, 0.05); color: var(--text-2); font-size: 14px; font-weight: 700; cursor: pointer; transition: all 0.15s ease; flex-shrink: 0; }
  .stepper-step-btn:hover:not(:disabled) { background: rgba(255, 255, 255, 0.12); color: var(--text-1); }
  .interval-number-input { flex: 1; min-width: 0; height: 100%; border: none; background: transparent; text-align: center; font-size: 15px; font-weight: 700; color: var(--text-1); box-shadow: none; font-variant-numeric: tabular-nums; }

  .presets-block, .position-block { display: flex; flex-direction: column; gap: 6px; }
  .presets-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; padding: 4px; border-radius: 10px; background: rgba(0, 0, 0, 0.32); box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.5), inset 0 0 0 1px rgba(255, 255, 255, 0.04); }
  .preset-chip { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 1px; height: 40px; border-radius: 7px; border: 1px solid rgba(255, 255, 255, 0.08); background: linear-gradient(180deg, rgba(255, 255, 255, 0.08) 0%, rgba(255, 255, 255, 0.02) 100%); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16), inset 0 -1px 0 rgba(0, 0, 0, 0.35), 0 2px 4px rgba(0, 0, 0, 0.3); color: var(--text-2); cursor: pointer; transition: all 0.14s cubic-bezier(0.2, 0.8, 0.2, 1); position: relative; user-select: none; }
  .preset-chip:not(.active):hover:not(:disabled) { color: var(--text-1); background: linear-gradient(180deg, rgba(255, 255, 255, 0.14) 0%, rgba(255, 255, 255, 0.04) 100%); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.24), inset 0 -1px 0 rgba(0, 0, 0, 0.3), 0 3px 8px rgba(0, 0, 0, 0.4); transform: translateY(-1px); }
  .preset-chip:not(.active):active:not(:disabled) { transform: translateY(1px); box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.5), inset 0 -1px 0 rgba(255, 255, 255, 0.08); }
  .preset-chip.active { background: var(--accent-grad) !important; border: 1px solid rgba(255, 255, 255, 0.28); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.45), 0 2px 8px rgba(0, 0, 0, 0.4), 0 0 14px rgb(var(--accent-rgb) / 0.55); color: #ffffff !important; }
  .preset-chip.active:hover:not(:disabled) { background: var(--accent-grad) !important; color: #ffffff !important; filter: brightness(1.12); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.55), 0 3px 10px rgba(0, 0, 0, 0.45), 0 0 18px rgb(var(--accent-rgb) / 0.65); transform: translateY(-0.5px); }
  .preset-chip.active:active:not(:disabled) { transform: translateY(1px); }
  .preset-label { font-size: 11.5px; font-weight: 700; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6); }
  .preset-chip.active .preset-label,
  .preset-chip.active:hover .preset-label { color: #ffffff !important; font-weight: 800; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.4); }
  .preset-cps { font-size: 9.5px; font-weight: 600; color: var(--text-3); text-shadow: 0 1px 1px rgba(0, 0, 0, 0.5); }
  .preset-chip.active .preset-cps,
  .preset-chip.active:hover .preset-cps { color: rgba(255, 255, 255, 0.95) !important; font-weight: 700; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.4); }

  /* Target panel (mirrors presets height for 100% vertical balance) */
  .target-panel { display: flex; flex-direction: column; justify-content: space-between; height: 94px; padding: 10px 14px; border-radius: 10px; background: rgba(0, 0, 0, 0.32); box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.5), inset 0 0 0 1px rgba(255, 255, 255, 0.04); border: none; }
  .target-main { display: flex; align-items: center; gap: 10px; }
  .target-icon-wrap { display: grid; place-items: center; width: 32px; height: 32px; border-radius: 7px; background: rgb(var(--accent-rgb) / 0.18); border: 1px solid rgb(var(--accent-rgb) / 0.35); color: rgb(var(--accent-soft-rgb)); flex-shrink: 0; }
  .target-text strong { display: block; font-size: 12.5px; font-weight: 700; color: var(--text-1); }
  .target-text p { margin: 1px 0 0; font-size: 11px; color: var(--text-3); line-height: 1.35; }
  .target-tags { display: flex; align-items: center; gap: 6px; padding-top: 4px; }
  .target-tag { display: inline-flex; align-items: center; gap: 4px; padding: 2px 7px; border-radius: 5px; background: rgba(255, 255, 255, 0.05); border: 1px solid rgba(255, 255, 255, 0.08); font-size: 10.5px; color: var(--text-2); font-weight: 600; }
  .tag-dot { width: 5px; height: 5px; border-radius: 50%; background: #4ade80; box-shadow: 0 0 5px rgba(74, 222, 128, 0.6); }
  .hotkey-tag { color: rgb(var(--accent-soft-rgb)); border-color: rgb(var(--accent-rgb) / 0.3); background: rgb(var(--accent-rgb) / 0.12); }

  :global(.tip-icon) { color: rgb(var(--accent-soft-rgb)); flex-shrink: 0; }

  .sequence-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .sequence-heading h2 { margin: 0; font-size: 15px; }
  .sequence-heading p { margin: 2px 0 0; font-size: 12px; color: var(--text-3); }
  .counter-badge { padding: 3px 9px; border-radius: 6px; background: rgba(255, 255, 255, 0.06); border: 1px solid rgba(255, 255, 255, 0.08); font-size: 11.5px; color: var(--text-2); font-weight: 600; }
  .steps { display: grid; gap: 12px; }
  .add-step-cta { width: 100%; height: 38px; margin-top: 12px; border-radius: 9px; border: 1px dashed rgba(255, 255, 255, 0.2); background: rgba(255, 255, 255, 0.02); color: var(--text-2); font-size: 12.5px; font-weight: 600; gap: 7px; cursor: pointer; transition: all 0.15s ease; }
  .add-step-cta:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.6); background: rgb(var(--accent-rgb) / 0.08); color: var(--text-1); }

  .run-panel { position: sticky; top: 0; padding: 18px; border-radius: 14px; background: linear-gradient(180deg, rgba(255, 255, 255, 0.04) 0%, rgba(255, 255, 255, 0.015) 100%); border: 1px solid rgba(255, 255, 255, 0.09); box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3); }
  .run-heading { display: flex; align-items: center; gap: 8px; margin-bottom: 16px; }
  .run-heading h2 { margin: 0; font-size: 14px; font-weight: 700; flex: 1; }
  .run-light { width: 7px; height: 7px; border-radius: 50%; background: var(--idle, #64748b); }
  .run-light.live { background: #4ade80; box-shadow: 0 0 8px rgba(74, 222, 128, 0.8); }
  .hotkey-pill { display: inline-flex; align-items: center; gap: 4px; padding: 2px 7px; border-radius: 6px; background: rgba(255, 255, 255, 0.08); border: 1px solid rgba(255, 255, 255, 0.1); color: var(--text-2); font-size: 11px; font-weight: 700; }

  .repeat-section { border: none; padding: 0; margin: 0 0 16px; }
  .repeat-section legend { display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--text-2); margin-bottom: 8px; }
  .repeat-seg-track { display: grid; grid-template-columns: 1fr 1fr; gap: 3px; padding: 3px; border-radius: 9px; background: var(--input-fill); border: 1px solid var(--panel-rim); box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.3); }
  .repeat-seg-track button { display: flex; align-items: center; justify-content: center; gap: 6px; height: 32px; padding: 0 8px; border-radius: 7px; border: 1px solid transparent; background: transparent; color: var(--text-3); font-size: 11.5px; font-weight: 600; cursor: pointer; transition: all 0.15s ease; white-space: nowrap; }
  .repeat-seg-track button:not(.active):hover:not(:disabled) { color: var(--text-1); background: var(--hover); }
  .repeat-seg-track button.active:hover:not(:disabled) { background: var(--btn-fill); color: rgb(var(--accent-soft-rgb)); filter: brightness(1.08); }
  .repeat-seg-track button.active { background: var(--btn-fill); border-color: var(--btn-border); color: rgb(var(--accent-soft-rgb)); box-shadow: 0 1px 4px rgba(0, 0, 0, 0.25); }
  .repeat-seg-track button:disabled { opacity: 0.5; cursor: default; }

  .repeat-count-card { display: flex; flex-direction: column; gap: 8px; margin-top: 10px; padding: 10px 12px; border-radius: 10px; background: rgba(0, 0, 0, 0.22); border: 1px solid rgba(255, 255, 255, 0.06); }
  .repeat-count-head { display: flex; align-items: center; justify-content: space-between; gap: 6px; }
  .count-label { font-size: 11.5px; font-weight: 600; color: var(--text-2); }
  .count-badge { padding: 2px 7px; border-radius: 5px; background: rgb(var(--accent-rgb) / 0.15); border: 1px solid rgb(var(--accent-rgb) / 0.35); color: rgb(var(--accent-soft-rgb)); font-size: 10.5px; font-weight: 700; }

  .stepper-box.compact-clean { display: flex; align-items: center; justify-content: space-between; height: 36px; padding: 3px; background: var(--input-fill, #121212); border: 1px solid var(--panel-rim, rgba(255, 255, 255, 0.1)); border-radius: 9px; box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.35); transition: border-color var(--dur-fast), box-shadow var(--dur-fast); }
  .stepper-box.compact-clean:focus-within { border-color: rgb(var(--accent-rgb) / 0.6); box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.35), 0 0 0 2px rgb(var(--accent-rgb) / 0.15); }
  .stepper-box.compact-clean .stepper-step-btn { display: grid; place-items: center; width: 32px; height: 100%; border-radius: 6px; border: 1px solid rgba(255, 255, 255, 0.08); background: linear-gradient(180deg, rgba(255, 255, 255, 0.08) 0%, rgba(255, 255, 255, 0.02) 100%); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25); color: var(--text-2); cursor: pointer; transition: all 0.12s cubic-bezier(0.2, 0.8, 0.2, 1); flex-shrink: 0; }
  .stepper-box.compact-clean .stepper-step-btn:hover:not(:disabled) { background: rgb(var(--accent-rgb) / 0.2); border-color: rgb(var(--accent-rgb) / 0.5); color: rgb(var(--accent-soft-rgb)); transform: translateY(-0.5px); }
  .stepper-box.compact-clean .stepper-step-btn:active:not(:disabled) { transform: scale(0.94); }
  .stepper-box.compact-clean .stepper-step-btn:disabled { opacity: 0.35; cursor: default; }
  .stepper-clean-input { flex: 1; min-width: 0; height: 100%; border: none !important; background: transparent !important; text-align: center; font-size: 14.5px; font-weight: 700; color: var(--text-1); box-shadow: none !important; outline: none !important; padding: 0 !important; margin: 0 !important; font-variant-numeric: tabular-nums; }

  .quick-count-chips { display: grid; grid-template-columns: repeat(5, 1fr); gap: 4px; margin-top: 2px; }
  .count-chip { height: 24px; padding: 0 4px; border-radius: 5px; border: 1px solid rgba(255, 255, 255, 0.08); background: rgba(255, 255, 255, 0.035); color: var(--text-3); font-size: 11px; font-weight: 600; cursor: pointer; transition: all 0.12s ease; }
  .count-chip:not(.active):hover:not(:disabled) { background: rgba(255, 255, 255, 0.09); color: var(--text-1); border-color: rgba(255, 255, 255, 0.16); }
  .count-chip.active { background: var(--accent-grad) !important; border-color: rgba(255, 255, 255, 0.25); color: #ffffff !important; font-weight: 700; box-shadow: 0 0 8px rgb(var(--accent-rgb) / 0.4); }
  .count-chip.active:hover:not(:disabled) { background: var(--accent-grad) !important; color: #ffffff !important; filter: brightness(1.12); box-shadow: 0 0 12px rgb(var(--accent-rgb) / 0.6); }

  .repeat-infinite-row { display: flex; align-items: center; gap: 7px; margin-top: 10px; padding: 8px 12px; border-radius: 8px; background: rgb(var(--accent-rgb) / 0.08); border: 1px solid rgb(var(--accent-rgb) / 0.2); color: var(--text-2); font-size: 11.5px; }
  .repeat-infinite-row strong { color: rgb(var(--accent-soft-rgb)); }

  .picking-box { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 16px; border-radius: 10px; background: rgb(var(--accent-rgb) / 0.1); border: 1px dashed rgb(var(--accent-rgb) / 0.4); text-align: center; color: rgb(var(--accent-soft-rgb)); margin-bottom: 16px; }
  .picking-box p { margin: 2px 0; font-size: 11.5px; color: var(--text-2); }
  .picking-box small { font-size: 10.5px; color: var(--text-3); }

  .run-status-card { padding: 12px; border-radius: 10px; background: rgba(0, 0, 0, 0.25); border: 1px solid rgba(255, 255, 255, 0.06); margin-bottom: 16px; }
  .status-title-row { display: flex; flex-direction: column; gap: 2px; margin-bottom: 8px; }
  .status-caption { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-3); font-weight: 700; }
  .progress-title { font-size: 13px; font-weight: 700; color: var(--text-1); font-variant-numeric: tabular-nums; }
  .progress-meta { display: flex; justify-content: space-between; gap: 6px; margin-top: 8px; font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .countdown-note { display: flex; align-items: center; gap: 7px; font-size: 11px; color: var(--text-3); line-height: 1.4; }

  .run-cta { display: flex; align-items: center; justify-content: center; gap: 8px; width: 100%; height: 44px; border-radius: 9px; font-size: 14px; font-weight: 700; cursor: pointer; transition: all 0.15s cubic-bezier(0.2, 0.8, 0.2, 1); }
  .glow-primary-cta { background: var(--accent-grad); border: 1px solid rgba(255, 255, 255, 0.25); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.38), 0 3px 14px rgb(var(--accent-rgb) / 0.45); color: var(--btn-primary-text, #ffffff); text-shadow: 0 1px 2px rgba(0, 0, 0, 0.35); }
  .glow-primary-cta:hover:not(:disabled) { filter: brightness(1.1); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.45), 0 4px 18px rgb(var(--accent-rgb) / 0.65); transform: translateY(-0.5px); }
  .glow-primary-cta:active:not(:disabled) { transform: translateY(0.5px); }
  .glow-danger-cta { background: linear-gradient(180deg, #e5484d 0%, #b82a2e 100%); border: 1px solid rgba(255, 255, 255, 0.25); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.35), 0 3px 12px rgba(229, 72, 77, 0.45); color: #ffffff; }
  .glow-danger-cta:hover:not(:disabled) { background: linear-gradient(180deg, #f05a5f 0%, #c93539 100%); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.45), 0 4px 18px rgba(229, 72, 77, 0.65); }
  .run-cta kbd { margin-left: auto; padding: 2px 6px; border-radius: 4px; background: rgba(0, 0, 0, 0.25); font-size: 11px; font-weight: 700; }

  .hotkey-status-footer { display: flex; align-items: center; justify-content: center; gap: 6px; margin-top: 10px; font-size: 11px; color: var(--text-3); }

  .validation { display: flex; align-items: center; gap: 8px; padding: 10px 14px; margin-top: 14px; border-radius: 8px; background: rgb(var(--accent-rgb) / 0.1); border: 1px solid rgb(var(--accent-rgb) / 0.3); color: rgb(var(--accent-soft-rgb)); font-size: 12.5px; }


  /* TIER 1: Ultra-Wide / 1440p / 4K Displays (min-width: 1550px and min-height: 850px, or min-width: 2200px) */
  @media (min-width: 1550px) and (min-height: 850px), (min-width: 2200px) {
    .workspace { grid-template-columns: minmax(0, 1fr) 360px; gap: 26px; }
    .steps { grid-template-columns: repeat(auto-fill, minmax(min(100%, 430px), 1fr)); gap: 16px; }
    .add-step-cta { grid-column: 1 / -1; }
    .simple-dashboard { grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr)); gap: 20px; }
    .run-panel { padding: 22px; }
  }

  /* TIER 2: Standard Desktop 1080p (901px to 1549px) */
  @media (min-width: 901px) and (max-width: 1549px) {
    .workspace { grid-template-columns: minmax(0, 1fr) 320px; gap: 18px; }
    .steps { grid-template-columns: repeat(auto-fill, minmax(min(100%, 390px), 1fr)); gap: 12px; }
    .add-step-cta { grid-column: 1 / -1; }
    .simple-dashboard { grid-template-columns: repeat(auto-fit, minmax(min(100%, 280px), 1fr)); gap: 14px; }
  }

  /* TIER 3: Compact 720p Window / Laptops / Low-Height (max-height: 750px), (max-width: 900px) */
  @media (max-height: 750px) and (min-width: 901px) {
    .run-panel { position: static; }
  }
  @media (max-width: 900px) {
    .workspace { grid-template-columns: 1fr; gap: 14px; }
    .run-panel { position: static; grid-row: 1; padding: 14px; }
    .steps { grid-template-columns: 1fr; gap: 10px; }
    .add-step-cta { grid-column: 1; }
    .simple-dashboard { grid-template-columns: 1fr; gap: 12px; }
    .form-row-2col { grid-template-columns: 1fr; }
    .mode-track { margin-bottom: 12px; }
    .mode-card { padding: 8px 10px; }
    .simple-card { padding: 14px 16px; }
  }
</style>
