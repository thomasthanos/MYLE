<script lang="ts">
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Check from "@lucide/svelte/icons/check";
  import WindowButton from "../lib/components/WindowButton.svelte";
  import { errorMessage } from "../lib/errors";
  import SplashLogo from "../splash/SplashLogo.svelte";
  import type { Progress, SetupApi, SetupState, Stage } from "./api";

  let { info, api }: { info: SetupState; api: SetupApi } = $props();

  const windowHandle = isTauri() ? getCurrentWindow() : null;
  const steps = ["Close", "Remove", "Finish"];
  const stages: Record<string, { step: number; percent: number; label: string }> = {
    closingApp: { step: 0, percent: 0, label: "Closing MYLE…" },
    preparing: { step: 0, percent: 6, label: "Getting ready…" },
    shortcuts: { step: 1, percent: 10, label: "Removing shortcuts…" },
    removingFiles: { step: 1, percent: 16, label: "Removing files…" },
    registering: { step: 1, percent: 80, label: "Removing MYLE from Installed apps…" },
    removingData: { step: 1, percent: 88, label: "Removing local settings and data…" },
    finishing: { step: 2, percent: 96, label: "Finishing up…" },
  };

  let screen = $state<"options" | "working" | "done" | "error">("options");
  let choice = $state<"keep" | "remove" | null>(null);
  let stage = $state<Stage>("closingApp");
  let files = $state({ done: 0, total: 0, file: "" });
  let error = $state("");
  const busy = $derived(screen === "working");
  const current = $derived(stages[stage] ?? stages.closingApp);
  const percent = $derived(
    stage === "removingFiles" && files.total > 0
      ? current.percent + Math.floor(64 * Math.min(1, files.done / files.total))
      : current.percent,
  );
  const phase = $derived(
    screen === "options" ? "awaiting-choice" : busy ? "removing" : screen === "done" ? "removed" : "error",
  );

  function onEvent(event: Progress) {
    if (event.event === "stage") stage = event.data.stage;
    else files = event.data;
  }

  async function start() {
    if (!choice || busy || screen === "done") return;
    stage = "closingApp";
    files = { done: 0, total: 0, file: "" };
    screen = "working";
    try {
      await api.uninstall(choice === "remove", onEvent);
      screen = "done";
    } catch (cause) {
      error = errorMessage(cause);
      screen = "error";
    }
  }

  function close() {
    if (!busy) void api.exit();
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === "Escape") close(); }} />

<div class="uninstall" data-phase={phase}>
  <header class="chrome" data-tauri-drag-region>
    <span class="wordmark">MYLE</span>
    <span class="version">v{info.version}</span>
    <div class="controls">
      <WindowButton kind="minimize" label="Minimize" onclick={() => windowHandle?.minimize()} />
      <WindowButton kind="close" label="Close" disabled={busy} onclick={close} />
    </div>
  </header>

  <main>
    <div class="brand-art"><SplashLogo {phase} /></div>

    {#key screen}
      <section class="panel" aria-label="Uninstall MYLE">
        {#if screen === "options"}
          <h1>Uninstall MYLE?</h1>
          <p class="lead">Choose what to do with your local data. MYLE will close before removal.</p>

          <fieldset>
            <legend class="sr-only">Local settings and data</legend>
            <label class="choice">
              <input type="radio" name="data-choice" value="keep" bind:group={choice} aria-labelledby="keep-title" aria-describedby="keep-detail" />
              <span>
                <strong id="keep-title">Keep my settings and data</strong>
                <small id="keep-detail">Reuse them if you install MYLE again.</small>
              </span>
            </label>
            <label class="choice erase">
              <input type="radio" name="data-choice" value="remove" bind:group={choice} aria-labelledby="remove-title" aria-describedby="remove-detail" />
              <span>
                <strong id="remove-title">Remove my settings and data</strong>
                <small id="remove-detail">Delete local settings, sign-in data and caches.</small>
              </span>
            </label>
          </fieldset>
          <p class="note">Game Saves backups are kept with either choice.</p>
          <p class="path" title={info.dir}>{info.dir}</p>
        {:else if busy}
          <div class="status-heading">
            <h1>Uninstalling MYLE…</h1>
            <span class="percent" aria-hidden="true">{percent}%</span>
          </div>
          <p class="lead" role="status">{current.label}</p>
          <div class="track" role="progressbar" aria-label="Uninstall progress" aria-valuenow={percent}>
            <span class="fill" style:--progress={percent / 100}></span>
          </div>
          <p class="path" title={files.file}>{stage === "removingFiles" ? files.file : ""}</p>
          <ol class="steps" aria-label="Uninstall stages">
            {#each steps as step, index (step)}
              <li class:completed={index < current.step} aria-current={index === current.step ? "step" : undefined}>
                <span></span>{step}
              </li>
            {/each}
          </ol>
        {:else if screen === "done"}
          <div class="status-heading">
            <h1>MYLE was removed</h1>
            <span class="check" aria-hidden="true"><Check size={17} strokeWidth={2.3} /></span>
          </div>
          <p class="lead" role="status">
            {choice === "remove" ? "Your local settings and data were removed." : "Your settings and data were kept."}
          </p>
        {:else}
          <h1>Uninstall could not finish</h1>
          <p class="lead error selectable" role="alert">{error}</p>
        {/if}
      </section>
    {/key}
  </main>

  {#if !busy}
    <footer>
      {#if screen === "options"}
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <button type="button" class="btn primary" class:danger={choice === "remove"} disabled={!choice} onclick={start}>Uninstall</button>
      {:else if screen === "done"}
        <button type="button" class="btn primary" onclick={close}>Finish</button>
      {:else}
        <button type="button" class="btn" onclick={close}>Close</button>
        <button type="button" class="btn primary" onclick={start}>Try again</button>
      {/if}
    </footer>
  {/if}
</div>

<style>
  .uninstall {
    --signal: #aab9ff;
    --ink: #edf5ff;
    --muted: #aab9d0;
    --line: #28364d;
    --delete: #ffb49d;
    width: 100%;
    height: 100dvh;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    padding: 14px 21px 17px;
    border: 1px solid var(--line);
    background: radial-gradient(ellipse at 50% 23%, #152242, transparent 67%), #0a0c12;
    color: var(--ink);
    font: 13px/1.45 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  }

  [data-phase="removing"] { --signal: #82ecff; }
  [data-phase="removed"] { --signal: #95efca; }
  [data-phase="error"] { --signal: var(--delete); }

  .chrome {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 36px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--line);
  }

  .wordmark, .version { pointer-events: none; }
  .wordmark { margin-right: auto; font: 19px/1 var(--font-titlebar); }
  .version { color: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .controls { display: flex; align-items: center; gap: 1px; }

  main {
    min-height: 0;
    overflow: auto;
    scrollbar-gutter: stable both-edges;
  }

  .brand-art { height: clamp(126px, 32vh, 177px); display: grid; }
  .panel { padding: 7px 0 12px; }

  h1 {
    display: block;
    background: none;
    color: var(--ink);
    -webkit-text-fill-color: currentColor;
    filter: none;
    font: 650 23px/1.2 "Segoe UI Variable Display", "Segoe UI", sans-serif;
    letter-spacing: -0.025em;
  }

  [data-phase="error"] h1 { color: var(--delete); }
  .lead, .note { color: var(--muted); }
  .lead { margin-top: 5px; }
  .note { margin-top: 10px; font-size: 11px; }
  .error { overflow-wrap: anywhere; }

  fieldset {
    min-width: 0;
    display: grid;
    gap: 6px;
    margin: 13px 0 0;
    padding: 0;
    border: 0;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 51px;
    padding: 7px 11px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: rgb(28 38 57 / 0.66);
    cursor: pointer;
    transition: background 160ms, border-color 160ms;
  }

  .choice:hover { background: #23304a; }
  .choice:has(:checked) { border-color: var(--signal); background: #222f50; }
  .erase:has(:checked) { border-color: var(--delete); background: #34262e; }
  .choice:has(:focus-visible) { outline: 2px solid var(--ink); outline-offset: -2px; }
  .choice input { flex: none; margin: 0; accent-color: var(--signal); }
  .erase input { accent-color: var(--delete); }
  .choice strong, .choice small { display: block; }
  .choice strong { font-weight: 600; }
  .choice small { color: var(--muted); font-size: 11px; }

  .status-heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .percent { color: var(--signal); font-size: 16px; font-weight: 650; font-variant-numeric: tabular-nums; }

  .track {
    height: 5px;
    margin-top: 16px;
    overflow: hidden;
    border-radius: 999px;
    background: #293a55;
  }

  .fill {
    display: block;
    height: 100%;
    overflow: hidden;
    background: linear-gradient(90deg, #6589f1, var(--signal));
    transform: scaleX(var(--progress));
    transform-origin: left;
  }

  .path {
    height: 16px;
    margin-top: 7px;
    overflow: hidden;
    color: var(--muted);
    font-size: 10px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .steps {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    padding: 13px 0 0;
    margin: 13px 0 0;
    border-top: 1px solid var(--line);
    list-style: none;
  }

  .steps li {
    display: grid;
    justify-items: center;
    gap: 6px;
    color: var(--muted);
    position: relative;
    font-size: 11px;
  }

  .steps li + li::before {
    position: absolute;
    top: 4px;
    right: calc(50% + 11px);
    left: calc(-50% + 11px);
    border-top: 1px solid #435574;
    content: "";
  }

  .steps li span {
    width: 9px;
    height: 9px;
    border: 1px solid #788ca9;
    border-radius: 50%;
  }

  .steps .completed span { background: #788ca9; }
  .steps [aria-current="step"] { color: var(--signal); }
  .steps [aria-current="step"] span { background: var(--signal); border-color: var(--signal); box-shadow: 0 0 0 3px rgb(130 236 255 / 0.19); }

  .check {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--signal);
    border-radius: 50%;
    color: var(--signal);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }

  footer .btn { min-width: 81px; }
  footer .primary.danger { background: #a63d3b; border-color: #e27069; }
  footer .primary.danger:hover { background: #bf4c49; }

  .sr-only { position: absolute; width: 1px; height: 1px; clip-path: inset(50%); overflow: hidden; }

  @media (max-height: 510px) {
    .brand-art { height: 114px; overflow: hidden; }
    .brand-art :global(.orbital) { scale: 0.62; }
  }

  @media (prefers-reduced-motion: no-preference) {
    .panel { animation: reveal 240ms var(--ease-out) both; }
    .fill { transition: transform 210ms linear; }
    .fill::after {
      display: block;
      width: 40%;
      height: 100%;
      background: linear-gradient(90deg, transparent, #fff9, transparent);
      animation: sweep 1.8s ease-in-out infinite;
      content: "";
    }
  }

  @keyframes reveal { from { opacity: 0.45; transform: translateY(5px); } }
  @keyframes sweep { from { transform: translateX(-100%); } to { transform: translateX(250%); } }
</style>
