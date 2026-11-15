<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    checkForUpdate,
    finishStartup,
    formatBytes,
    installUpdate,
    splashShown,
    type DownloadEvent,
    type UpdateAsset,
  } from "../lib/updater";
  import SplashLogo, { type SplashPhase } from "./SplashLogo.svelte";

  /* After an update the previous version was on screen until now: long enough to read "Updated to …". Otherwise the app opens as soon as it can. */
  const JUST_UPDATED_MS = 700;
  /** How long an error stays on screen before the app starts anyway. */
  const ERROR_HOLD_MS = 2200;
  /** How often the byte count and speed are redrawn; faster is unreadable. */
  const META_EVERY_MS = 250;
  /** Window over which the download speed is measured. */
  const SPEED_WINDOW_MS = 500;

  const steps = ["Check", "Update", "Open"] as const;
  type Step = (typeof steps)[number];

  let phase = $state<SplashPhase>("checking");
  let title = $state("Checking for updates…");
  let detail = $state("");
  let progress = $state<number | null>(null); // null = indeterminate
  let percent = $state<number | null>(null);
  let transfer = $state(""); // "5.7 MB of 13.6 MB"
  let rate = $state(""); // "2.1 MB/s · 4s left"
  let version = $state("");

  let updateAttempted = $state(false);
  let failedStep = $state<Step | null>(null);

  const opening = $derived(phase === "restarting" || phase === "starting");
  const currentStep: Step = $derived(
    phase === "error" ? failedStep ?? "Check" : phase === "checking" ? "Check" : opening ? "Open" : "Update",
  );
  const stepIndex = $derived(steps.indexOf(currentStep));

  let updater = { checkForUpdate, installUpdate, finishStartup };

  const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

  onMount(() => {
    if (!isTauri()) {
      const demo = new URLSearchParams(location.search).get("demo");
      if (import.meta.env.DEV && demo !== null) {
        void import("./preview").then(({ previewUpdater }) => {
          const preview = previewUpdater(demo);
          updater = preview;
          version = preview.version;
          void run();
        });
      } else {
        detail = "Preview: the updater only runs inside the app";
      }
      return;
    }
    // Show the window only after the first frame is painted (no blank flash), and only then let the main window load, so the two don't compete.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => void getCurrentWindow().show().finally(() => void splashShown())),
    );
    void getVersion().then((v) => (version = v));
    void run();
  });

  async function run() {
    const startedAt = performance.now();
    let upToDate = false;
    let justUpdated: string | null = null;
    try {
      const result = await updater.checkForUpdate();
      upToDate = result.status === "upToDate";
      if (result.status === "justUpdated") justUpdated = result.current;
      if (result.status === "available") await applyUpdate(result.current, result.latest, result.asset);
    } catch (err) {
      showError(phase === "checking" ? "Couldn't check for updates" : "Update failed", err);
      await sleep(ERROR_HOLD_MS);
    }

    phase = "starting";
    title = "Starting…";
    detail = justUpdated ? `Updated to v${justUpdated}` : upToDate ? "You're on the latest version" : "";
    progress = 1;
    percent = null;
    transfer = "";
    rate = "";
    // Just updated: the previous version was on screen until now, so only long enough to read the line.
    if (justUpdated) await sleep(Math.max(JUST_UPDATED_MS - (performance.now() - startedAt), 0));
    // A failure here would leave "Starting…" on screen for good: it is shown, and tried twice more.
    for (let attempt = 1; ; attempt++) {
      try {
        await updater.finishStartup();
        return;
      } catch (err) {
        showError("Couldn't open MYLE", err);
        if (attempt === 3) {
          transfer = "Close this window and start MYLE again.";
          return;
        }
        transfer = "Trying again…";
        await sleep(ERROR_HOLD_MS);
      }
    }
  }

  // In a real update the app exits once the installer launches, so this only returns in demo mode.
  async function applyUpdate(current: string, latest: string, asset: UpdateAsset) {
    updateAttempted = true;
    phase = "downloading";
    title = "Downloading update";
    detail = `v${current} → v${latest}`;
    progress = asset.size ? 0 : null;
    percent = asset.size ? 0 : null;
    transfer = asset.size ? `0 B of ${formatBytes(asset.size)}` : "";
    await updater.installUpdate(asset, onDownloadEvent);
  }

  let sample = { at: 0, bytes: 0 };
  let speed = 0; // bytes per second, smoothed
  let lastMeta = 0;

  function onDownloadEvent(e: DownloadEvent) {
    switch (e.event) {
      case "started":
        sample = { at: performance.now(), bytes: 0 };
        speed = 0;
        lastMeta = 0;
        progress = e.data.total ? 0 : null;
        percent = e.data.total ? 0 : null;
        transfer = e.data.total ? "0 B of " + formatBytes(e.data.total) : "";
        break;
      case "progress":
        onProgress(e.data.downloaded, e.data.total);
        break;
      case "verifying":
        phase = "verifying";
        title = "Verifying update…";
        detail = "Checking its SHA-256 checksum";
        clearTransfer();
        break;
      case "installing":
        phase = "installing";
        title = "Installing update…";
        detail = "It opens again by itself when it's done";
        clearTransfer();
        break;
      case "restarting":
        phase = "restarting";
        clearTransfer();
        title = "Restarting…";
        detail = e.data.version ? `Opening v${e.data.version}` : "Opening the new version";
        progress = 1;
        break;
    }
  }

  function onProgress(downloaded: number, total: number | null) {
    const now = performance.now();
    const elapsed = now - sample.at;
    if (elapsed >= SPEED_WINDOW_MS) {
      const current = ((downloaded - sample.bytes) * 1000) / elapsed;
      speed = speed ? speed * 0.65 + current * 0.35 : current;
      sample = { at: now, bytes: downloaded };
    }
    // The backend includes the manifest hint and withdraws it when stale.
    const size = total || null;
    progress = size ? Math.min(downloaded / size, 1) : null;
    total = size;

    // The bar and percentage follow every event; speed and bytes settle at a readable pace.
    percent = progress === null ? null : Math.floor(progress * 100);
    const finished = total !== null && downloaded >= total;
    if (now - lastMeta < META_EVERY_MS && !finished) return;
    lastMeta = now;
    transfer = total ? `${formatBytes(downloaded)} of ${formatBytes(total)}` : formatBytes(downloaded);
    rate =
      speed > 0
        ? [`${formatBytes(speed)}/s`, total && !finished ? timeLeft((total - downloaded) / speed) : ""]
            .filter(Boolean)
            .join(" · ")
        : "";
  }

  function timeLeft(seconds: number): string {
    if (seconds < 60) return `${Math.max(1, Math.ceil(seconds))}s left`;
    return `${Math.ceil(seconds / 60)} min left`;
  }

  function clearTransfer() {
    progress = null;
    percent = null;
    transfer = "";
    rate = "";
  }

  function showError(heading: string, err: unknown) {
    failedStep = currentStep;
    phase = "error";
    title = heading;
    detail = err instanceof Error ? err.message : String(err);
    clearTransfer();
    transfer = "Opening the app anyway…";
  }
</script>

<main class="splash" data-phase={phase} data-tauri-drag-region>
  <header class="brand">
    <span class="wordmark">MYLE</span>
    {#if version}<span class="version">v{version}</span>{/if}
  </header>

  <SplashLogo {phase} />

  <section class="status" aria-label="Startup status">
    <div class="summary">
      <div class="message" role="status" aria-atomic="true">
        {#key title}
          <h1>{title}</h1>
          {#if detail}<p class="detail">{detail}</p>{/if}
        {/key}
        {#if phase === "error" && transfer}
          <p class="recovery">{transfer}</p>
        {/if}
      </div>
      {#if percent !== null}<span class="percent" aria-hidden="true">{percent}%</span>{/if}
    </div>

    {#if phase !== "error"}
      <div
        class="track"
        class:indeterminate={progress === null}
        role="progressbar"
        aria-label={title}
        aria-valuenow={opening ? 100 : (percent ?? undefined)}
        style:--progress={progress ?? 0}
      >
        <div class="fill"></div>
      </div>
    {/if}

    {#if phase === "downloading"}
      <div class="transfer">
        <span>{transfer}</span>
        <span>{rate}</span>
      </div>
    {/if}
  </section>

  <ol class="steps" aria-label="Startup stages">
    {#each steps as step, index (step)}
      {@const state = step === failedStep ? "failed"
        : index === stepIndex ? "current"
        : index > stepIndex ? "pending"
        : step === "Update" && !updateAttempted ? "skipped" : "complete"}
      <li data-step={state} aria-current={index === stepIndex ? "step" : undefined}>
        <span class="node" aria-hidden="true"></span>
        {step}
        {#if state === "skipped"}<span class="sr-only"> skipped</span>{/if}
        {#if state === "failed" && index !== stepIndex}<span class="sr-only"> failed</span>{/if}
      </li>
    {/each}
  </ol>
</main>

<style>
  .splash {
    --base: #0a0c12;
    --ink: #edf5ff;
    --muted: #a4b3c8;
    --signal: #aab9ff;
    --line: #243047;
    --error: #ffb493;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto auto;
    width: 100%;
    height: 100dvh;
    min-height: 300px;
    padding: 18px clamp(15px, 8vw, 28px) 18px;
    border: 1px solid var(--line);
    background: radial-gradient(ellipse at 50% 30%, #152242, transparent 73%), var(--base);
    color: var(--ink);
    font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
    font-variant-numeric: tabular-nums;
  }

  .splash[data-phase="downloading"] { --signal: #82ecff; }
  .splash[data-phase="verifying"] { --signal: #c9b3ff; }
  .splash:is([data-phase="installing"], [data-phase="restarting"], [data-phase="starting"]) { --signal: #95efca; }
  .splash[data-phase="error"] { --signal: var(--error); }

  /* Tauri starts dragging when the pointer targets the drag region itself. */
  .splash > * { pointer-events: none; }

  .brand {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    min-height: 30px;
    gap: 8px;
    border-bottom: 1px solid var(--line);
  }

  .wordmark { font: 19px/1 var(--font-titlebar); }

  .version {
    color: var(--muted);
    font-size: 11px;
    overflow-wrap: anywhere;
  }

  .status {
    display: grid;
    grid-template-rows: minmax(49px, auto) 4px 16px;
    gap: 5px;
    align-content: start;
    min-width: 0;
  }

  .summary {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .message { min-width: 0; }

  h1 {
    display: block;
    margin: 0;
    background: none;
    color: var(--ink);
    -webkit-text-fill-color: currentColor;
    filter: none;
    font: 640 clamp(16px, 6.5vw, 21px)/1.22 "Segoe UI Variable Display", "Segoe UI", sans-serif;
    letter-spacing: -0.035em;
  }

  .detail,
  .recovery {
    margin-top: 4px;
    color: var(--muted);
    font-size: 11px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }

  [data-phase="error"] .status { grid-template-rows: minmax(79px, auto); }
  [data-phase="error"] h1 { color: var(--error); }

  [data-phase="error"] .detail {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .recovery { color: var(--ink); }

  .percent {
    color: var(--signal);
    font-size: 14px;
    font-weight: 650;
  }

  .track {
    overflow: hidden;
    border-radius: 4px;
    background: #26364d;
  }

  .fill {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    border-radius: inherit;
    background: linear-gradient(90deg, color-mix(in srgb, var(--signal) 65%, #556ed9), var(--signal));
    transform: scaleX(var(--progress));
    transform-origin: left;
  }

  .indeterminate .fill {
    width: 34%;
    transform: translateX(97%);
  }

  .transfer {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    color: var(--muted);
    font-size: 10.5px;
    line-height: 16px;
  }

  .transfer span:last-child { text-align: right; }

  .steps {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    align-items: center;
    list-style: none;
    margin: 9px 0 0;
    padding: 12px 0 0;
    border-top: 1px solid var(--line);
  }

  .steps li {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: 10.5px;
    line-height: 1.2;
  }

  .steps li + li::before {
    position: absolute;
    top: 4px;
    right: calc(50% + 10px);
    left: calc(-50% + 10px);
    border-top: 1px solid #34445e;
    content: "";
  }

  .steps [data-step="skipped"]::before,
  .steps [data-step="skipped"] + li::before { border-top-style: dashed; }

  .node {
    width: 9px;
    height: 9px;
    border: 1px solid #7589a6;
    border-radius: 50%;
  }

  [data-step="complete"] .node { background: #7589a6; }
  [data-step="skipped"] .node { border-style: dashed; }
  [data-step="failed"] .node { border-color: var(--error); }
  .steps [aria-current="step"] { color: var(--signal); }

  [aria-current="step"] .node {
    background: var(--signal);
    border-color: var(--signal);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--signal) 22%, transparent);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  @media (prefers-reduced-motion: no-preference) {
    .message h1,
    .detail,
    .recovery { animation: reveal 280ms var(--ease-out) both; }

    .fill { transition: transform 190ms linear; }

    [data-phase="downloading"] .fill::after {
      position: absolute;
      inset: 0;
      background: linear-gradient(90deg, transparent 30%, rgb(255 255 255 / 0.6), transparent 70%);
      animation: glint 2s ease-in-out infinite;
      content: "";
    }

    .indeterminate .fill { animation: sweep 1.5s var(--ease-in-out) infinite; }
    [aria-current="step"] .node { animation: step-in 380ms var(--ease-out) both; }
  }

  @keyframes reveal {
    from { opacity: 0; transform: translateY(5px); }
  }

  @keyframes glint {
    from { transform: translateX(-100%); }
    to { transform: translateX(100%); }
  }

  @keyframes sweep {
    from { transform: translateX(-100%); }
    to { transform: translateX(295%); }
  }

  @keyframes step-in {
    from { transform: scale(0.65); }
    55% { transform: scale(1.27); }
  }
</style>
