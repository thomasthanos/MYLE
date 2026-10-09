<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, fly, scale } from "svelte/transition";
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ArrowLeftRight from "@lucide/svelte/icons/arrow-left-right";
  import Check from "@lucide/svelte/icons/check";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Power from "@lucide/svelte/icons/power";
  import Puzzle from "@lucide/svelte/icons/puzzle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Rocket from "@lucide/svelte/icons/rocket";
  import { formatBytes } from "../lib/updater";
  import { loadApi, type Progress, type SetupApi, type SetupState, type Stage } from "./api";
  import Brand from "./Brand.svelte";
  import Titlebar from "./Titlebar.svelte";
  import Toggle from "./Toggle.svelte";
  import { compareVersions } from "./versions";
  import Working, { type Step } from "./Working.svelte";

  type Screen = "loading" | "options" | "working" | "done" | "error" | "unavailable";

  interface StepDef {
    label: string;
    stages: Stage[];
    /** The share of the whole bar this step fills, in percent. */
    from: number;
    to: number;
    onlyWithData?: boolean;
  }

  const INSTALL_STEPS: StepDef[] = [
    { label: "Getting ready", stages: ["closingApp", "preparing"], from: 0, to: 4 },
    { label: "Copying files", stages: ["copying"], from: 4, to: 90 },
    { label: "Adding it to Installed apps", stages: ["registering"], from: 90, to: 94 },
    { label: "Setting up shortcuts", stages: ["shortcuts"], from: 94, to: 98 },
    { label: "Finishing up", stages: ["finishing"], from: 98, to: 100 },
  ];
  const UNINSTALL_STEPS: StepDef[] = [
    { label: "Closing the app", stages: ["closingApp", "preparing"], from: 0, to: 8 },
    { label: "Removing shortcuts", stages: ["shortcuts"], from: 8, to: 16 },
    { label: "Removing files", stages: ["removingFiles"], from: 16, to: 80 },
    { label: "Removing it from Installed apps", stages: ["registering"], from: 80, to: 88 },
    { label: "Removing settings and data", stages: ["removingData"], from: 88, to: 96, onlyWithData: true },
    { label: "Finishing up", stages: ["finishing"], from: 96, to: 100 },
  ];

  /** Passive mode: how long "Updated to …" shows, then how long the setup
   *  stays once the app's window is on screen (`launch` waits for it). */
  const PASSIVE_SHOW_MS = 250;
  const PASSIVE_HANDOVER_MS = 300;

  let api: SetupApi | null = null;
  let info = $state<SetupState | null>(null);
  let screen = $state<Screen>("loading");

  // Choices
  let dir = $state("");
  let desktop = $state(true);
  let startMenu = $state(true);
  let startup = $state(false);
  let startMinimized = $state(true);
  let launchAfter = $state(true);
  /** Open the browser extension's page when done: offered on a first install. */
  let getExtension = $state(false);
  let removeData = $state(false);

  let folderError = $state<string | null>(null);
  let launchError = $state<string | null>(null);
  /** Shortcuts that could not be made; the install itself worked. */
  let shortcutWarnings = $state<string[]>([]);
  let finishing = $state(false);
  /** Programs to close before going on; set, it shows the question. */
  let running = $state<string[] | null>(null);
  /** Uninstall: the app is open right now (a heads-up, not a question). */
  let openNow = $state(false);

  // Progress
  let stage = $state<Stage>("preparing");
  let files = $state({ done: 0, total: 0, file: "" });
  let error = $state("");
  let installedDir = $state("");

  const uninstalling = $derived(info?.mode === "uninstall");
  const busy = $derived(screen === "working");

  /** Where this setup's version stands against the installed one. */
  const relation = $derived.by(() => {
    const installed = info?.installed?.version;
    if (!info || uninstalling || !installed) return "fresh" as const;
    const order = compareVersions(info.version, installed);
    return order > 0 ? ("update" as const) : order === 0 ? ("same" as const) : ("older" as const);
  });

  const copy = $derived.by(() => {
    const version = info?.version ?? "";
    const installed = info?.installed?.version ?? "";
    switch (relation) {
      case "update":
        return {
          title: `Update to ${version}`,
          lead: `Version ${installed} is installed. Your settings and data are kept.`,
          action: "Update",
          working: "Updating…",
          done: `Updated to ${version}`,
        };
      case "same":
        return {
          title: `Reinstall version ${version}`,
          lead: `Version ${version} is already installed. Reinstalling restores the app files; your settings and data are kept.`,
          action: `Reinstall ${version}`,
          working: "Reinstalling…",
          done: "Reinstalled",
        };
      case "older":
        return {
          title: `Install version ${version}`,
          lead: `A newer version (${installed}) is installed; this replaces it with ${version}.`,
          action: "Install anyway",
          working: "Installing…",
          done: `Version ${version} is installed`,
        };
      default:
        return {
          title: "Install MYLE",
          lead: "Choose where it goes and how it starts. No administrator rights needed.",
          action: "Install",
          working: "Installing…",
          done: "You're all set",
        };
    }
  });

  const steps = $derived(
    (uninstalling ? UNINSTALL_STEPS : INSTALL_STEPS).filter((step) => !step.onlyWithData || removeData),
  );
  const activeStep = $derived(Math.max(0, steps.findIndex((step) => step.stages.includes(stage))));
  const stepStates = $derived<Step[]>(
    steps.map((step, index) => ({
      label: step.label,
      state:
        screen === "done" || index < activeStep ? "done" : index === activeStep ? "active" : "pending",
    })),
  );
  const percent = $derived.by(() => {
    if (screen === "done") return 100;
    const step = steps[activeStep];
    if (!step) return 0;
    const counted = stage === "copying" || stage === "removingFiles";
    const fraction = counted && files.total > 0 ? files.done / files.total : 0.35;
    return step.from + (step.to - step.from) * fraction;
  });
  const action = $derived(actionText(stage));
  const detail = $derived.by(() => {
    if (stage === "copying" && files.total > 0) {
      return `${files.file.replaceAll("/", "\\")}  ·  ${formatBytes(files.done)} of ${formatBytes(files.total)}`;
    }
    if (stage === "removingFiles") return files.file.replaceAll("/", "\\");
    return "";
  });

  function actionText(current: Stage): string {
    const app = info?.product ?? "the app";
    switch (current) {
      case "closingApp":
        return `Closing ${app}…`;
      case "preparing":
        return uninstalling ? "Getting ready…" : "Preparing the install folder…";
      case "copying":
        return "Copying files…";
      case "registering":
        return uninstalling ? "Removing it from Installed apps…" : "Adding it to Installed apps…";
      case "shortcuts":
        return uninstalling ? "Removing shortcuts…" : "Setting up shortcuts…";
      case "removingFiles":
        return "Removing files…";
      case "removingData":
        return "Removing settings and data…";
      case "finishing":
        return "Finishing up…";
    }
  }

  function message(err: unknown): string {
    return err instanceof Error ? err.message : String(err);
  }

  onMount(() => {
    if (isTauri()) {
      // Show the window only after the first frame is painted (no blank flash).
      requestAnimationFrame(() => requestAnimationFrame(() => void getCurrentWindow().show()));
    }
    void load();
  });

  async function load() {
    try {
      await loadState();
    } catch (err) {
      // Nothing to show without the setup's answer: say why, and let it ask again.
      error = message(err);
      screen = "error";
    }
  }

  async function loadState() {
    api = await loadApi();
    if (!api) {
      screen = "unavailable";
      return;
    }
    const state = await api.state();
    info = state;
    dir = state.dir;
    desktop = state.shortcuts.desktop;
    startMenu = state.shortcuts.startMenu;
    startup = state.shortcuts.startup;
    startMinimized = state.shortcuts.startMinimized;
    getExtension = !!state.extensionUrl && !state.installed;
    if (!state.ready) {
      screen = "unavailable";
      return;
    }
    screen = "options";
    if (state.mode === "uninstall") {
      openNow = (await api.running(state.dir).catch(() => [])).length > 0;
    }
    if (state.passive) void start();
  }

  /** "Try again": the install (or uninstall) again, or the first question
   *  to the setup when even that failed. */
  function retry() {
    if (info) {
      void start();
    } else {
      screen = "loading";
      void load();
    }
  }

  async function start() {
    if (!api || !info || busy) return;
    folderError = null;
    if (!uninstalling) {
      try {
        await api.checkFolder(dir);
      } catch (err) {
        folderError = message(err);
        return;
      }
    }
    const apps = await api.running(uninstalling ? info.dir : dir).catch(() => []);
    // The uninstaller already said the app will be closed; passive asks nothing.
    if (apps.length && !uninstalling && !info.passive) {
      running = apps;
      return;
    }
    await run();
  }

  async function run() {
    if (!api || !info) return;
    running = null;
    launchError = null;
    shortcutWarnings = [];
    screen = "working";
    stage = "closingApp";
    files = { done: 0, total: 0, file: "" };
    error = "";
    const onEvent = (e: Progress) => {
      if (e.event === "stage") stage = e.data.stage;
      else files = e.data;
    };
    try {
      if (uninstalling) await api.uninstall(removeData, onEvent);
      else {
        const done = await api.install({ desktop, startMenu, startup, startMinimized }, onEvent);
        installedDir = done.dir;
        shortcutWarnings = done.warnings;
      }
      screen = "done";
      if (info.passive) void finishPassive();
      else if (!uninstalling && launchAfter) void finish();
    } catch (err) {
      error = message(err);
      screen = "error";
    }
  }

  async function finish() {
    if (!api || !info || finishing) return;
    finishing = true;
    if (!uninstalling && (launchAfter || info.passive)) {
      try {
        await api.launch();
      } catch (err) {
        launchError = message(err);
        finishing = false;
        return;
      }
    }
    // The page opens in the browser, over the app: a nicety, never a blocker.
    if (!uninstalling && !info.passive && getExtension && info.extensionUrl) {
      await api.openExtension().catch(() => {});
    }
    await api.exit();
  }

  /** Passive (`/P`, the in-app updater): show the result for a moment, open
   *  the app, and close half a second after its window is up, so something
   *  is always on screen. */
  async function finishPassive() {
    await sleep(PASSIVE_SHOW_MS);
    if (!api || !info || finishing) return;
    finishing = true;
    if (!uninstalling) {
      try {
        await api.launch();
      } catch (err) {
        launchError = message(err);
        finishing = false;
        return;
      }
      await sleep(PASSIVE_HANDOVER_MS);
    }
    await api.exit();
  }

  function sleep(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function closeAfterInstall() {
    if (api) void api.exit();
  }

  function close() {
    if (busy) return;
    if (api) void api.exit();
  }

  function onKeydown(e: KeyboardEvent) {
    if (running) {
      if (e.key === "Escape") running = null;
      return;
    }
    if (e.key === "Escape" && !busy) close();
    if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      if (screen === "options") void start();
      else if (screen === "done") void finish();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app-backdrop" aria-hidden="true"></div>

<div class="window">
  <Titlebar label={uninstalling ? "Uninstall" : "Setup"} {busy} onclose={close} />

  <div class="body">
    <Brand
      version={info?.version ?? "…"}
      note="Apps, cleanup, game saves and Windows tweaks, all in one place."
      paused={screen === "error" || screen === "done"}
    />

    <main class="main">
      {#key screen}
        <div class="screen" in:fly={{ y: 10, duration: 280, easing: cubicOut }}>
          {#if screen === "loading"}
            <div class="center"><span class="loader" aria-label="Loading"></span></div>
          {:else if screen === "unavailable"}
            <div class="center">
              <h2>Nothing to install</h2>
              <p class="lead">
                {info && !info.ready
                  ? "This setup was built without the app inside. Build it with npm run build."
                  : "Open this page inside the setup, or add ?demo=install to preview it."}
              </p>
            </div>
          {:else if screen === "options" && info}
            {#if uninstalling}
              <h2>Uninstall {info.product}?</h2>
              <p class="lead">
                Version {info.version} · {formatBytes(info.size)} in
                <span class="path selectable" title={info.dir}>{info.dir}</span>
              </p>

              <div class="card">
                <Toggle
                  bind:checked={removeData}
                  label="Also remove my settings and data"
                  hint="Settings, sign-in, Game Saves setup and restore safety copies, caches. Your Game Saves backups are always kept."
                />
              </div>

              {#if openNow}
                <p class="notice">
                  <span class="dot"></span>
                  {info.product} is open. It will be closed first.
                </p>
              {/if}
            {:else}
              <div class="title-row">
                <h2>{copy.title}</h2>
                {#if relation === "same"}
                  <span class="version-pill"><RefreshCw size={12} strokeWidth={2} /> Same version installed</span>
                {/if}
              </div>
              <p class="lead">{copy.lead}</p>

              <div class="section-heading location-heading">
                <span class="label">Install location</span>
                <span class="section-note">This account only</span>
              </div>
              <div class="location" class:invalid={folderError}>
                <span class="location-icon"><FolderOpen size={16} strokeWidth={1.8} /></span>
                <span class="path selectable" title={dir}>{dir}</span>
              </div>
              {#if folderError}
                <p class="field-error" transition:fade={{ duration: 150 }}>{folderError}</p>
              {:else}
                <p class="field-note">Needs {formatBytes(info.size)}</p>
              {/if}

              <div class="section-heading shortcut-heading">
                <span class="label">Shortcuts and startup</span>
                <span class="section-note">Choose how to access the app</span>
              </div>
              <div class="toggles">
                <Toggle bind:checked={desktop} icon={Monitor} label="Desktop shortcut" />
                <Toggle bind:checked={startMenu} icon={LayoutGrid} label="Start menu" />
                <Toggle bind:checked={startup} icon={Power} label="Start with Windows">
                  {#snippet extra()}
                    <button
                      type="button"
                      class="start-mode"
                      disabled={!startup}
                      title={`At sign-in the app opens ${startMinimized ? "minimized in the taskbar" : "on screen"}. Click to switch.`}
                      aria-label={`At sign-in: ${startMinimized ? "minimized" : "on screen"}. Click to switch.`}
                      onclick={() => (startMinimized = !startMinimized)}
                    >
                      <ArrowLeftRight size={10} strokeWidth={2.2} />
                      {startMinimized ? "Minimized" : "On screen"}
                    </button>
                  {/snippet}
                </Toggle>
                <Toggle bind:checked={launchAfter} icon={Rocket} label="Open when finished" />
              </div>

            {/if}

            <div class="footer">
              {#if !uninstalling && info.extensionUrl}
                <!-- In the footer's free corner: the window keeps its size. -->
                <button
                  type="button"
                  class="extension"
                  class:on={getExtension}
                  aria-pressed={getExtension}
                  title="MYLE Passwords fills your saved logins in Chrome, Edge and Firefox. Its page opens in your browser when setup finishes."
                  onclick={() => (getExtension = !getExtension)}
                >
                  <Puzzle size={14} strokeWidth={1.9} />
                  Browser extension
                  <span class="tick" aria-hidden="true">{#if getExtension}<Check size={11} strokeWidth={3} />{/if}</span>
                </button>
              {/if}
              <span class="spacer"></span>
              <button class="btn" onclick={close}>Cancel</button>
              <button class="btn primary big" class:danger={uninstalling} onclick={start}>
                {uninstalling ? "Uninstall" : copy.action}
              </button>
            </div>
          {:else if screen === "working"}
            <Working
              title={uninstalling ? "Uninstalling…" : copy.working}
              {percent}
              {action}
              {detail}
              steps={stepStates}
            />
          {:else if screen === "done" && info}
            <div class="result">
              <svg class="mark-ok" viewBox="0 0 64 64" aria-hidden="true">
                <circle cx="32" cy="32" r="29" />
                <path d="M19 33.5l9 9 17-19" />
              </svg>
              <h2>{uninstalling ? `${info.product} was removed` : copy.done}</h2>
              <p class="lead">
                {#if uninstalling}
                  {removeData
                    ? "Its settings and data were removed too."
                    : "Your settings are kept, in case you come back."}
                {:else}
                  {info.product} {info.version} is ready in
                  <span class="path selectable" title={installedDir}>{installedDir}</span>
                {/if}
              </p>
              {#if shortcutWarnings.length}
                <div class="launch-error" role="status">
                  <strong>Installed, but some shortcuts are missing.</strong>
                  {#each shortcutWarnings as warning (warning)}<span>{warning}</span>{/each}
                  <span>Run the setup again to retry them.</span>
                </div>
              {/if}
              {#if launchError}
                <div class="launch-error" role="alert">
                  <strong>Installed successfully, but the app did not open.</strong>
                  <span>{launchError}</span>
                </div>
              {/if}
            </div>
            <div class="footer">
              {#if info.passive && !launchError}
                <span class="opening" role="status">
                  <span class="dot" aria-hidden="true"></span>
                  {uninstalling ? "Closing…" : `Opening ${info.product}…`}
                </span>
              {:else if !uninstalling && !launchError}
                <Toggle bind:checked={launchAfter} icon={Rocket} label="Open {info.product}" />
              {:else if launchError}
                <button class="btn" onclick={closeAfterInstall}>Close setup</button>
              {/if}
              <span class="spacer"></span>
              {#if !info.passive || launchError}
                <button class="btn primary big" onclick={finish} disabled={finishing}>
                  {launchError ? "Try again" : !uninstalling && launchAfter ? "Finish and open" : "Finish"}
                </button>
              {/if}
            </div>
          {:else if screen === "error"}
            <div class="result failed">
              <svg class="mark-fail" viewBox="0 0 64 64" aria-hidden="true">
                <circle cx="32" cy="32" r="29" />
                <path d="M23 23l18 18m0-18L23 41" />
              </svg>
              <h2>{!info ? "Setup could not start" : uninstalling ? "Uninstall could not finish" : "Setup could not finish"}</h2>
              <p class="error-text selectable">{error}</p>
            </div>
            <div class="footer">
              <span class="spacer"></span>
              <button class="btn" onclick={close}>Close</button>
              <button class="btn primary big" onclick={retry}>Try again</button>
            </div>
          {/if}
        </div>
      {/key}
    </main>
  </div>

  {#if running}
    <div class="overlay" transition:fade={{ duration: 150 }}>
      <div class="dialog" role="alertdialog" aria-labelledby="running-title" transition:scale={{ start: 0.96, duration: 180 }}>
        <h3 id="running-title">{info?.product} is running</h3>
        <p>It has to close before setup can go on. Anything unsaved in it may be lost.</p>
        <ul>
          {#each running as name (name)}<li>{name}</li>{/each}
        </ul>
        <div class="dialog-actions">
          <button class="btn" onclick={() => (running = null)}>Cancel</button>
          <button class="btn primary" onclick={run}>Close it and continue</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .window {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .main {
    flex: 1;
    min-width: 0;
    padding: 22px 25px 17px;
  }

  .screen {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  h2 {
    font-size: 20px;
    line-height: 1.2;
  }

  .title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 9px;
    min-width: 0;
  }

  .version-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    padding: 5px 8px;
    border: 1px solid rgb(var(--accent-rgb) / 0.2);
    border-radius: 999px;
    background: rgb(var(--accent-rgb) / 0.1);
    color: #cfd6ff;
    font-size: 10.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  .lead {
    margin-top: 7px;
    font-size: 13px;
    color: var(--text-2);
    line-height: 1.45;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .location-heading {
    margin-top: 16px;
  }

  .shortcut-heading {
    margin-top: 14px;
    margin-bottom: 7px;
  }

  .label {
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }

  .section-note {
    font-size: 10.5px;
    color: var(--text-3);
    white-space: nowrap;
  }

  .location {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 38px;
    padding: 7px 11px;
    border: 1px solid rgb(255 255 255 / 0.065);
    border-radius: 12px;
    background: linear-gradient(145deg, rgb(255 255 255 / 0.04), rgb(0 0 0 / 0.16));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.025);
  }

  .location.invalid {
    border-color: rgb(229 72 77 / 0.6);
    box-shadow: 0 0 0 2px rgb(229 72 77 / 0.08);
  }

  .location-icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex: none;
    border-radius: 8px;
    color: #bfc8ff;
    background: rgb(var(--accent-rgb) / 0.1);
  }

  .location .path {
    flex: 1;
    font-size: 12px;
  }

  .path {
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--text-2);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .lead .path {
    display: inline-block;
    max-width: 100%;
    vertical-align: bottom;
  }

  .field-note,
  .field-error {
    margin-top: 4px;
    font-size: 10.5px;
    color: var(--text-3);
  }

  .field-error {
    color: #ff9f9b;
  }

  .toggles {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 7px;
    margin: 0;
  }

  .toggles :global(.toggle) {
    min-height: 55px;
  }

  .toggles :global(.toggle .label) {
    color: var(--text-1);
    font-size: 12px;
  }

  /* "Get the browser extension": its page opens once setup is done. */
  .extension {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 34px;
    padding: 0 10px 0 11px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 11px;
    background: rgb(255 255 255 / 0.03);
    color: var(--text-2);
    font-size: 12px;
    font-weight: 560;
    transition:
      background var(--dur-fast),
      border-color var(--dur-fast),
      color var(--dur-fast);
  }

  .extension:hover {
    border-color: rgb(255 255 255 / 0.14);
    color: var(--text-1);
  }

  .extension.on {
    border-color: rgb(var(--accent-rgb) / 0.4);
    background: rgb(var(--accent-rgb) / 0.12);
    color: #dce0ff;
  }

  .extension:focus-visible {
    outline: 2px solid rgb(var(--accent-rgb) / 0.75);
    outline-offset: 2px;
  }

  .tick {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border: 1.5px solid rgb(255 255 255 / 0.25);
    border-radius: 5px;
  }

  .extension.on .tick {
    border-color: transparent;
    background: rgb(var(--accent-rgb));
    color: #fff;
  }

  /* "Minimized / On screen at sign-in", under "Start with Windows". */
  .start-mode {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    margin-top: 2px;
    padding: 1px 7px 1px 6px;
    border: 1px solid rgb(var(--accent-rgb) / 0.22);
    border-radius: 999px;
    background: rgb(var(--accent-rgb) / 0.1);
    color: #c9cffa;
    font-size: 10.5px;
    font-weight: 500;
    white-space: nowrap;
    transition:
      background var(--dur-fast),
      border-color var(--dur-fast);
  }

  .start-mode:hover {
    border-color: rgb(var(--accent-rgb) / 0.4);
    background: rgb(var(--accent-rgb) / 0.18);
  }

  .start-mode:disabled {
    border-color: rgb(255 255 255 / 0.06);
    background: none;
    color: var(--text-3);
  }

  .card {
    margin-top: 20px;
    padding: 4px;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.03);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.06);
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: 14px;
    font-size: 12.5px;
    color: var(--text-2);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #ffb454;
    box-shadow: 0 0 8px rgb(255 180 84 / 0.6);
  }

  .footer {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: auto;
    padding-top: 16px;
  }

  .footer :global(.toggle) {
    margin-left: 0;
  }

  .spacer {
    flex: 1;
  }

  .btn.big {
    height: 36px;
    min-width: 112px;
    padding: 0 18px;
    font-weight: 600;
  }

  .btn.primary.danger {
    background: linear-gradient(135deg, #ff7a70, #e5484d);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.25),
      0 6px 16px -6px rgb(229 72 77 / 0.6);
  }

  .center {
    display: grid;
    place-content: center;
    gap: 8px;
    height: 100%;
    text-align: center;
  }

  .loader {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 2.5px solid rgb(var(--accent-rgb) / 0.2);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }

  .result {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    min-height: 0;
    text-align: center;
  }

  .result h2 {
    margin-top: 18px;
  }

  .result .lead {
    max-width: 380px;
  }

  .opening {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
    font-size: 13px;
  }

  .opening .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ok, #3ecf8e);
    box-shadow: 0 0 10px var(--ok-glow, rgb(62 207 142 / 0.6));
    animation: opening-pulse 0.9s ease-in-out infinite alternate;
  }

  @keyframes opening-pulse {
    from {
      opacity: 0.45;
      transform: scale(0.8);
    }
  }

  .launch-error {
    display: grid;
    gap: 5px;
    width: min(100%, 400px);
    margin-top: 16px;
    padding: 11px 13px;
    border: 1px solid rgb(255 180 84 / 0.22);
    border-radius: 12px;
    background: rgb(255 180 84 / 0.065);
    color: #ffd6a1;
    text-align: left;
  }

  .launch-error strong {
    font-size: 12px;
  }

  .launch-error span {
    overflow-wrap: anywhere;
    color: var(--text-2);
    font-size: 11.5px;
    line-height: 1.45;
  }

  .mark-ok,
  .mark-fail {
    width: 76px;
    height: 76px;
    fill: none;
    stroke-linecap: round;
    stroke-linejoin: round;
    filter: drop-shadow(0 0 18px var(--accent-glow));
  }

  .mark-ok circle,
  .mark-fail circle {
    stroke: var(--accent);
    stroke-width: 3;
    stroke-dasharray: 183;
    stroke-dashoffset: 183;
    animation: draw 620ms var(--ease-out) forwards;
  }

  .mark-ok path {
    stroke: var(--accent-2);
    stroke-width: 4;
    stroke-dasharray: 42;
    stroke-dashoffset: 42;
    animation: draw 380ms var(--ease-out) 420ms forwards;
  }

  .mark-fail {
    filter: drop-shadow(0 0 18px rgb(229 72 77 / 0.45));
  }

  .mark-fail circle {
    stroke: #e5484d;
  }

  .mark-fail path {
    stroke: #ff9f9b;
    stroke-width: 4;
    stroke-dasharray: 60;
    stroke-dashoffset: 60;
    animation: draw 380ms var(--ease-out) 420ms forwards;
  }

  .error-text {
    max-width: 400px;
    max-height: 120px;
    margin-top: 10px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.55;
    color: #ffc2bf;
    text-align: left;
  }

  .overlay {
    position: absolute;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: rgb(4 6 12 / 0.6);
  }

  .dialog {
    width: 380px;
    padding: 20px 20px 16px;
    border-radius: 16px;
    background: linear-gradient(180deg, #171c2b, #10141f);
    box-shadow:
      inset 0 0 0 1px rgb(255 255 255 / 0.08),
      0 24px 60px -20px rgb(0 0 0 / 0.9);
  }

  .dialog h3 {
    font-size: 16px;
  }

  .dialog p {
    margin-top: 8px;
    font-size: 13px;
    color: var(--text-2);
  }

  .dialog ul {
    margin: 10px 0 0;
    padding-left: 18px;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-3);
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }

  @keyframes draw {
    to {
      stroke-dashoffset: 0;
    }
  }

  @keyframes spin {
    to {
      transform: rotate(1turn);
    }
  }
</style>
