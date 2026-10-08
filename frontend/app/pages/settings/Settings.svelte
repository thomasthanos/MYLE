<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import PanelBottomClose from "@lucide/svelte/icons/panel-bottom-close";
  import Palette from "@lucide/svelte/icons/palette";
  import Power from "@lucide/svelte/icons/power";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Rocket from "@lucide/svelte/icons/rocket";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Logo from "../../../lib/components/Logo.svelte";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import { settings } from "../../../lib/settings.svelte";
  import { whatsNew } from "../../../lib/whats-new.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import { checkForUpdate, formatBytes, installUpdate, type UpdateAsset } from "../../../lib/updater";
  import AccountCard from "./AccountCard.svelte";
  import SyncedData from "./SyncedData.svelte";

  const REPO_URL = "https://github.com/thomasthanos/MYLE";

  type UpdateView =
    | { state: "idle" }
    | { state: "checking" }
    | { state: "upToDate"; latest: string }
    | { state: "available"; latest: string; asset: UpdateAsset }
    | { state: "downloading"; latest: string; progress: number | null; detail: string }
    | { state: "installing" }
    | { state: "error"; message: string };

  let version = $state("");
  let update = $state<UpdateView>({ state: "idle" });
  /** Starting with Windows (the Startup shortcut), and how the app opens then. */
  let startup = $state({ enabled: false, minimized: true, canChange: false });
  let startupBusy = $state(false);
  /** Closing the window keeps the app running next to the clock. */
  let keepInTray = $state(false);

  onMount(() => {
    if (!isTauri()) return;
    void getVersion().then((v) => (version = v));
    void invoke<typeof startup>("startup_get").then((value) => (startup = value));
    void invoke<boolean>("tray_get").then((value) => (keepInTray = value));
  });

  async function setKeepInTray(value: boolean) {
    keepInTray = value;
    try {
      await invoke("tray_set", { enabled: value });
    } catch (error) {
      keepInTray = !value;
      toast.error(`Could not save the setting: ${message(error)}`);
    }
  }

  async function setStartup(field: "enabled" | "minimized", value: boolean) {
    startup[field] = value;
    startupBusy = true;
    try {
      if (field === "enabled") await invoke("startup_set_enabled", { enabled: value });
      else await invoke("startup_set_minimized", { minimized: value });
    } catch (error) {
      startup[field] = !value;
      toast.error(`Could not save the setting: ${message(error)}`);
    } finally {
      startupBusy = false;
    }
  }

  function message(error: unknown) {
    return error instanceof Error ? error.message : String(error);
  }

  async function check() {
    update = { state: "checking" };
    try {
      const result = await checkForUpdate();
      if (result.status === "available") update = { state: "available", latest: result.latest, asset: result.asset };
      else if (result.status === "upToDate") update = { state: "upToDate", latest: result.latest };
      else if (result.status === "justUpdated") update = { state: "upToDate", latest: result.current };
      else update = { state: "error", message: "Updates are not configured in this build." };
    } catch (error) {
      update = { state: "error", message: message(error) };
    }
  }

  /** Same path as the splash: download, verify, run the installer, restart. */
  async function install(latest: string, asset: UpdateAsset) {
    update = { state: "downloading", latest, progress: 0, detail: "" };
    try {
      await installUpdate(asset, (event) => {
        if (event.event === "started" || event.event === "progress") {
          const total = event.data.total;
          const done = event.event === "progress" ? event.data.downloaded : 0;
          update = {
            state: "downloading",
            latest,
            progress: total ? Math.min(done / total, 1) : null,
            detail: total ? `${formatBytes(done)} of ${formatBytes(total)}` : formatBytes(done),
          };
        } else if (event.event === "installing") {
          update = { state: "installing" };
        }
      });
    } catch (error) {
      update = { state: "error", message: message(error) };
      toast.error(`The update failed: ${message(error)}`);
    }
  }

  function open(url: string) {
    void openUrl(url).catch((error) => toast.error(`Could not open the link: ${message(error)}`));
  }
</script>

<PageHeader title="Settings" subtitle="Your account, what it keeps in sync, and this PC." />

<div class="layout">
  <div class="column">
    <AccountCard />
    <SyncedData />
  </div>

  <div class="column">
    <section class="panel" aria-labelledby="appearance-title">
      <header class="head">
        <div class="head-left">
          <span class="card-icon"><Palette size={16} /></span>
          <div>
            <h2 id="appearance-title">Appearance</h2>
            <p class="sub">Visual surfaces and how the window opens</p>
          </div>
        </div>
      </header>

      <label class="row">
        <span class="row-icon"><Sparkles size={15} /></span>
        <span class="text">
          <strong>Glass effects</strong>
          <small>Translucent, blurred panels with glow and motion. Uses more graphics power.</small>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={settings.glass}
          onchange={(e) => settings.setGlass(e.currentTarget.checked)}
        />
      </label>

      <label class="row">
        <span class="row-icon"><Power size={15} /></span>
        <span class="text">
          <strong>Start with Windows</strong>
          <small>
            {startup.canChange
              ? "Open the app when you sign in. Game Saves' scheduled backups run without it."
              : "Available in the installed app."}
          </small>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={startup.enabled}
          disabled={!startup.canChange || startupBusy}
          onchange={(e) => void setStartup("enabled", e.currentTarget.checked)}
        />
      </label>

      <label class="row">
        <span class="row-icon"><Minimize2 size={15} /></span>
        <span class="text">
          <strong>Start minimized</strong>
          <small>When it starts with Windows, the app waits in the tray (or the taskbar) instead of opening on screen.</small>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={startup.minimized}
          disabled={!startup.enabled || startupBusy}
          onchange={(e) => void setStartup("minimized", e.currentTarget.checked)}
        />
      </label>

      <label class="row">
        <span class="row-icon"><PanelBottomClose size={15} /></span>
        <span class="text">
          <strong>Keep running in the tray</strong>
          <small>
            Off: closing the window quits MYLE. On: it stays next to the clock, so Ctrl+Shift+L and browser filling
            keep working; quit from the tray icon.
          </small>
        </span>
        <input
          type="checkbox"
          class="switch"
          checked={keepInTray}
          onchange={(e) => void setKeepInTray(e.currentTarget.checked)}
        />
      </label>
    </section>

    <section class="panel" aria-labelledby="updates-title">
      <header class="head">
        <div class="head-left">
          <span class="card-icon"><Rocket size={16} /></span>
          <div>
            <h2 id="updates-title">Updates</h2>
            <p class="sub">Automatic release checks at launch</p>
          </div>
        </div>
        <span class="version-pill">{version ? `v${version}` : "dev preview"}</span>
      </header>

      <div class="surface">
        <span class="text">
          <strong>MYLE {version ? `v${version}` : "(dev preview)"}</strong>
          <small>Checked at every start. Downloads from downloads.thomast.uk, with GitHub as the fallback.</small>
        </span>

        <div class="update" aria-live="polite">
          {#if update.state === "idle"}
            <span class="status"><span class="ok-dot" aria-hidden="true"></span> Automatic updates enabled</span>
          {:else if update.state === "checking"}
            <span class="status"><LoaderCircle size={14} class="spin" /> Checking…</span>
          {:else if update.state === "upToDate"}
            <span class="status ok"><CircleCheck size={14} /> You're on the latest version.</span>
          {:else if update.state === "available"}
            {@const available = update}
            <span class="status accent"><Download size={14} /> Version {available.latest} is available.</span>
            <button class="btn small primary" onclick={() => install(available.latest, available.asset)}>
              Install and restart
            </button>
          {:else if update.state === "downloading"}
            <span class="status">
              <LoaderCircle size={14} class="spin" /> Downloading {update.latest}… {update.detail}
            </span>
            <span class="bar" class:indeterminate={update.progress === null}>
              <span style:transform={update.progress === null ? undefined : `scaleX(${update.progress})`}></span>
            </span>
          {:else if update.state === "installing"}
            <span class="status">
              <LoaderCircle size={14} class="spin" /> Installing; the app will restart by itself.
            </span>
          {:else if update.state === "error"}
            <span class="status error" title={update.message}>{update.message}</span>
          {/if}
          {#if update.state === "idle" || update.state === "upToDate" || update.state === "error"}
            <button class="btn small" onclick={check}><RefreshCw size={13} /> Check for updates</button>
          {/if}
        </div>
      </div>
    </section>

    <section class="panel" aria-labelledby="about-title">
      <div class="about-row">
        <span class="brand-mark"><Logo size={38} /></span>
        <div class="text">
          <h2 id="about-title">MYLE</h2>
          <small>Windows utility &amp; optimization suite · © 2026 ThomasThanos</small>
        </div>
      </div>

      <div class="links">
        <button class="btn small" onclick={() => open(REPO_URL)}>
          <ExternalLink size={13} /> GitHub repository
        </button>
        <button class="btn small" disabled={whatsNew.loading} onclick={() => void whatsNew.showCurrent()}>
          {#if whatsNew.loading}<LoaderCircle size={13} class="spin" />{:else}<Sparkles size={13} />{/if} What's new
        </button>
        <button class="btn small" onclick={() => open(`${REPO_URL}/releases`)}>
          <ExternalLink size={13} /> All releases
        </button>
      </div>
    </section>
  </div>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1.18fr) minmax(0, 1fr);
    align-items: start;
    gap: 14px;
  }

  .column {
    display: grid;
    gap: 14px;
    min-width: 0;
  }

  .panel {
    display: grid;
    gap: 14px;
    padding: 18px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: var(--radius-lg);
    background: linear-gradient(180deg, rgb(255 255 255 / 0.045), rgb(255 255 255 / 0.018));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.06);
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .head-left {
    display: flex;
    align-items: center;
    gap: 11px;
    min-width: 0;
  }

  .card-icon {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.22);
    border-radius: 10px;
    background: linear-gradient(160deg, rgb(var(--accent-rgb) / 0.16), rgb(var(--accent-rgb) / 0.04));
    color: rgb(var(--accent-soft-rgb));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1);
  }

  h2 {
    font-size: 14.5px;
    line-height: 1.2;
  }

  .sub {
    margin-top: 2px;
    color: var(--text-3);
    font-size: 11.5px;
  }

  .version-pill {
    padding: 3px 10px;
    border: 1px solid rgb(var(--accent-rgb) / 0.28);
    border-radius: 999px;
    background: rgb(var(--accent-rgb) / 0.1);
    color: rgb(var(--accent-soft-rgb));
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 11px 13px;
    border: 1px solid rgb(255 255 255 / 0.055);
    border-radius: 11px;
    background: rgb(0 0 0 / 0.16);
    cursor: pointer;
    transition: border-color var(--dur-fast);
  }

  .row:hover {
    border-color: rgb(var(--accent-rgb) / 0.22);
  }

  .row-icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.18);
    border-radius: 9px;
    background: rgb(var(--accent-rgb) / 0.08);
    color: rgb(var(--accent-soft-rgb) / 0.92);
  }

  .surface {
    display: grid;
    gap: 12px;
    padding: 12px 13px;
    border: 1px solid rgb(255 255 255 / 0.055);
    border-radius: 11px;
    background: rgb(0 0 0 / 0.16);
  }

  .text {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  strong {
    font-size: 12.5px;
    font-weight: 600;
  }

  small {
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.45;
  }

  .update {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid rgb(255 255 255 / 0.055);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    flex: 1;
    min-width: 0;
    color: var(--text-2);
    font-size: 12px;
  }

  .status.ok {
    color: rgb(110 225 175);
  }

  .status.accent {
    color: rgb(var(--accent-soft-rgb));
  }

  .status.error {
    overflow: hidden;
    color: rgb(255 170 150 / 0.9);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .ok-dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 8px var(--ok-glow);
  }

  .bar {
    position: relative;
    flex-basis: 100%;
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.3);
  }

  .bar span {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--accent-grad);
    transform-origin: left;
    transition: transform 160ms linear;
  }

  .bar.indeterminate span {
    width: 34%;
    animation: sweep 1.2s var(--ease-in-out) infinite;
  }

  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }

  .about-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .brand-mark {
    display: grid;
    place-items: center;
    flex: none;
    filter: drop-shadow(0 6px 14px rgb(0 0 0 / 0.35));
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-top: 12px;
    border-top: 1px solid rgb(255 255 255 / 0.055);
  }

  @media (max-width: 1000px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
</style>
