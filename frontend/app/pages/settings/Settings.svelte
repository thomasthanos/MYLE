<script lang="ts">
  import LocalExtensionUpdate from "../../../lib/components/LocalExtensionUpdate.svelte";
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import Moon from "@lucide/svelte/icons/moon";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import PanelBottomClose from "@lucide/svelte/icons/panel-bottom-close";
  import Power from "@lucide/svelte/icons/power";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Rocket from "@lucide/svelte/icons/rocket";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Logo from "../../../lib/components/Logo.svelte";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import { settings } from "../../../lib/settings.svelte";
  import { whatsNew } from "../../../lib/whats-new.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import type { UpdateAsset } from "../../../lib/updater";
  import { settingsUpdater } from "../../../lib/updater-state.svelte";
  import AccountCard from "./AccountCard.svelte";
  import SyncedData from "./SyncedData.svelte";

  const REPO_URL = "https://github.com/thomasthanos/MYLE";

  let version = $state("");
  const update = $derived(settingsUpdater.view);
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

  function check() {
    return settingsUpdater.check();
  }

  /** Same path as the splash: download, verify, run the installer, restart. */
  function install(latest: string, asset: UpdateAsset) {
    return settingsUpdater.install(latest, asset);
  }

  // --- Sections, the side navigation and the search ---------------------

  type SectionId = "account" | "appearance" | "startup" | "updates" | "about";

  /** The words each setting is found by, beyond its title. */
  const words: Record<string, string> = {
    account: "account sign in sign out discord google sync cloud profile saved data preferences backup login",
    theme: "appearance theme dark light matte slate blurple black grey gray colour color blue default look",
    startWindows: "startup start with windows sign in boot launch autostart",
    startMinimized: "startup minimized minimised tray hidden background launch",
    tray: "tray close quit background notification area clock shortcut ctrl shift l browser filling",
    updates: "updates update version check install download release",
    about: "about github repository releases what's new changelog notes version",
  };

  let query = $state("");
  const terms = $derived(query.toLowerCase().split(/\s+/).filter(Boolean));

  /** Whether a setting shows for the search (always, with none). */
  function shows(key: string, title: string): boolean {
    if (!terms.length) return true;
    const text = `${title} ${words[key] ?? ""}`.toLowerCase();
    return terms.every((term) => text.includes(term));
  }

  const showing = $derived({
    account: shows("account", "Account & sync"),
    theme: shows("theme", "Theme"),
    startWindows: shows("startWindows", "Start with Windows"),
    startMinimized: shows("startMinimized", "Start minimized"),
    tray: shows("tray", "Keep running in the tray"),
    updates: shows("updates", "Updates"),
    about: shows("about", "About MYLE"),
  });
  const sectionShown = $derived<Record<SectionId, boolean>>({
    account: showing.account,
    appearance: showing.theme,
    startup: showing.startWindows || showing.startMinimized || showing.tray,
    updates: showing.updates,
    about: showing.about,
  });
  const nothingFound = $derived(terms.length > 0 && !Object.values(sectionShown).some(Boolean));

  const hiddenSection = (id: SectionId) => !sectionShown[id];

  function onKey(event: KeyboardEvent) {
    // Ctrl+F (or "/") searches the settings.
    const target = event.target as HTMLElement | null;
    const typing = !!target?.closest("input, textarea, [contenteditable='true']");
    if ((event.ctrlKey && event.key.toLowerCase() === "f") || (event.key === "/" && !typing)) {
      event.preventDefault();
      document.getElementById("settings-search")?.focus();
    }
  }

  function open(url: string) {
    void openUrl(url).catch((error) => toast.error(`Could not open the link: ${message(error)}`));
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="frame">
<PageHeader title="Settings" subtitle="Your account, how MYLE looks and starts, and updates." />

<div class="layout">
  <nav class="side" aria-label="Settings sections">
    <label class="search">
      <Search size={14} />
      <input
        id="settings-search"
        type="search"
        placeholder="Search settings…"
        autocomplete="off"
        spellcheck="false"
        bind:value={query}
        onkeydown={(e) => e.key === "Escape" && query && ((query = ""), e.stopPropagation())}
      />
      {#if query}
        <button class="clear" aria-label="Clear the search" onclick={() => (query = "")}><X size={13} /></button>
      {/if}
    </label>

  </nav>

  <div class="content">
    {#if nothingFound}
      <div class="empty">
        <Search size={22} />
        <strong>No setting matches “{query}”</strong>
        <small>Try another word, like “theme”, “startup”, “tray” or “update”.</small>
        <button class="btn small" onclick={() => (query = "")}>Clear the search</button>
      </div>
    {/if}

    <div class="col">
      <section id="settings-account" class="group" hidden={hiddenSection("account")} aria-labelledby="account-heading">
        <header class="group-head">
          <h2 id="account-heading">Account &amp; sync</h2>
          <p>Sign in to keep your choices the same on every PC. Everything works without an account too.</p>
        </header>
        <div class="stack">
          <AccountCard />
          <details class="more">
            <summary>What is saved in your account, and what stays on this PC</summary>
            <SyncedData />
          </details>
        </div>
      </section>
      <section id="settings-startup" class="group" hidden={hiddenSection("startup")} aria-labelledby="startup-heading">
        <header class="group-head">
          <h2 id="startup-heading">Startup &amp; tray</h2>
          <p>Whether MYLE opens with Windows, and what closing its window does.</p>
        </header>
        <div class="panel rows">
          <label class="row" hidden={!showing.startWindows}>
            <span class="row-icon"><Power size={15} /></span>
            <span class="text">
              <strong>Start with Windows</strong>
              <small>
                {startup.canChange
                  ? "Opens MYLE when you sign in to Windows. Game Saves' scheduled backups run even without it."
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

          <label class="row" hidden={!showing.startMinimized}>
            <span class="row-icon"><Minimize2 size={15} /></span>
            <span class="text">
              <strong>Start minimized</strong>
              <small>
                {startup.enabled
                  ? "When it starts with Windows, MYLE waits in the tray (or the taskbar) instead of opening on screen."
                  : "Turn on “Start with Windows” first."}
              </small>
            </span>
            <input
              type="checkbox"
              class="switch"
              checked={startup.minimized}
              disabled={!startup.enabled || startupBusy}
              onchange={(e) => void setStartup("minimized", e.currentTarget.checked)}
            />
          </label>

          <label class="row" hidden={!showing.tray}>
            <span class="row-icon"><PanelBottomClose size={15} /></span>
            <span class="text">
              <strong>Keep running in the tray</strong>
              <small>
                {keepInTray
                  ? "Closing the window keeps MYLE next to the clock: Ctrl+Shift+L and browser filling keep working. Quit from the tray icon."
                  : "Closing the window quits MYLE. Turn on to keep Ctrl+Shift+L and browser filling working after you close it."}
              </small>
            </span>
            <input
              type="checkbox"
              class="switch"
              checked={keepInTray}
              onchange={(e) => void setKeepInTray(e.currentTarget.checked)}
            />
          </label>
        </div>
      </section>
      <section id="settings-about" class="group" hidden={hiddenSection("about")} aria-labelledby="about-heading">
        <header class="group-head">
          <h2 id="about-heading">About</h2>
          <p>What changed lately, and where MYLE lives.</p>
        </header>
        <div class="panel">
          <div class="about-row">
            <span class="brand-mark"><Logo size={38} /></span>
            <div class="text">
              <strong class="brand">MYLE</strong>
              <small>Windows utility &amp; optimization suite · © 2026 ThomasThanos</small>
            </div>
          </div>

          <div class="links">
            <button class="btn small" disabled={whatsNew.loading} onclick={() => void whatsNew.showCurrent()}>
              {#if whatsNew.loading}<LoaderCircle size={13} class="spin" />{:else}<Sparkles size={13} />{/if} What's new
            </button>
            <button class="btn small" onclick={() => open(`${REPO_URL}/releases`)}>
              <ExternalLink size={13} /> All releases
            </button>
            <button class="btn small" onclick={() => open(REPO_URL)}>
              <ExternalLink size={13} /> GitHub repository
            </button>
          </div>
        </div>
      </section>
    </div>
    <div class="col">
      <section id="settings-appearance" class="group" hidden={hiddenSection("appearance")} aria-labelledby="appearance-heading">
        <header class="group-head">
          <h2 id="appearance-heading">Appearance</h2>
          <p>Pick a theme. It applies at once, to every MYLE window.</p>
        </header>
        <div class="panel">
          <div class="setting-head">
            <span class="row-icon"><Moon size={15} /></span>
            <span class="text">
              <strong>Theme</strong>
              <small>Choose your surfaces and accents. The layout stays the same.</small>
            </span>
          </div>
          <div class="themes" role="radiogroup" aria-label="Theme">
            <button
              class="theme"
              role="radio"
              aria-checked={!settings.dark}
              class:chosen={!settings.dark}
              onclick={() => settings.setDark(false)}
            >
              <span class="preview default" aria-hidden="true"><i></i><i></i><i></i></span>
              <span class="theme-text">
                <b>Default</b>
                <small>Deep blue-tinted panels</small>
              </span>
              <span class="tick" aria-hidden="true"><CircleCheck size={16} /></span>
            </button>
            <button
              class="theme"
              role="radio"
              aria-checked={settings.dark}
              class:chosen={settings.dark}
              onclick={() => settings.setDark(true)}
            >
              <span class="preview matte" aria-hidden="true"><i></i><i></i><i></i></span>
              <span class="theme-text">
                <b>Dark</b>
                <small>Matte surfaces, blurple accents</small>
              </span>
              <span class="tick" aria-hidden="true"><CircleCheck size={16} /></span>
            </button>
          </div>
        </div>
      </section>
      <section id="settings-updates" class="group" hidden={hiddenSection("updates")} aria-labelledby="updates-heading">
        <header class="group-head">
          <h2 id="updates-heading">Updates</h2>
          <p>MYLE checks for a new version every time it starts. You can also check now.</p>
        </header>
          <LocalExtensionUpdate />
        <div class="panel">
          <div class="setting-head">
            <span class="row-icon"><Rocket size={15} /></span>
            <span class="text">
              <strong>MYLE {version ? `v${version}` : "(dev preview)"}</strong>
              <small>Downloads come from downloads.thomast.uk, with GitHub as the fallback.</small>
            </span>
            <span class="version-pill">{version ? `v${version}` : "dev preview"}</span>
          </div>

          <div class="update" aria-live="polite">
            {#if update.state === "idle"}
              <span class="status"><span class="ok-dot" aria-hidden="true"></span> Automatic updates are on</span>
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
            {:else if update.state === "verifying"}
              <span class="status">
                <LoaderCircle size={14} class="spin" /> Verifying update…
              </span>
            {:else if update.state === "installing"}
              <span class="status">
                <LoaderCircle size={14} class="spin" /> Installing; the app will restart by itself.
              </span>
            {:else if update.state === "restarting"}
              <span class="status">
                <LoaderCircle size={14} class="spin" /> Opening v{update.version}…
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
    </div>
  </div>
</div>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 228px minmax(0, 1fr);
    align-items: start;
    gap: 22px;
  }

  /* --- Side navigation --- */

  .side {
    position: sticky;
    top: 0;
    display: grid;
    gap: 10px;
    min-width: 0;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 8px 0 11px;
    border: 1px solid var(--btn-border);
    border-radius: 10px;
    background: rgb(0 0 0 / 0.2);
    color: var(--text-3);
    transition: border-color var(--dur-fast);
  }

  .search:focus-within {
    border-color: rgb(var(--accent-rgb) / 0.55);
    color: var(--text-2);
  }

  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--text-1);
    font: inherit;
    font-size: 12.5px;
  }

  .search input::placeholder {
    color: var(--text-3);
  }

  .search input::-webkit-search-cancel-button {
    display: none;
  }

  .clear {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: var(--text-3);
  }

  .clear:hover {
    background: var(--hover);
    color: var(--text-1);
  }











  /* --- Sections --- */

  .content {
    display: grid;
    gap: 26px;
    min-width: 0;
    max-width: 820px;
  }

  .group {
    display: grid;
    gap: 12px;
    scroll-margin-top: 4px;
  }

  .group[hidden],
  .row[hidden] {
    display: none;
  }

  .group-head h2 {
    font-size: 16px;
    line-height: 1.25;
  }

  .group-head p {
    margin-top: 3px;
    color: var(--text-2);
    font-size: 12.5px;
  }

  .stack {
    display: grid;
    gap: 14px;
  }

  .panel {
    display: grid;
    gap: 14px;
    padding: 16px;
    border: 1px solid var(--glass-border);
    border-radius: var(--radius-lg);
    background: linear-gradient(180deg, rgb(255 255 255 / 0.04), rgb(255 255 255 / 0.015));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.05);
  }

  .panel.rows {
    gap: 0;
    padding: 6px;
  }

  .setting-head {
    display: flex;
    align-items: center;
    gap: 12px;
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
    white-space: nowrap;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 10px;
    border-radius: 11px;
    cursor: pointer;
    transition: background var(--dur-fast);
  }

  .row + .row {
    border-top: 1px solid rgb(255 255 255 / 0.05);
    border-top-left-radius: 0;
    border-top-right-radius: 0;
  }

  .row:hover {
    background: var(--hover);
  }

  .row:has(input:disabled) {
    cursor: default;
  }

  .row:has(input:disabled) .text {
    opacity: 0.7;
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

  .text {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  strong {
    font-size: 13px;
    font-weight: 600;
  }

  small {
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.45;
  }

  /* Theme cards */

  .themes {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .theme {
    position: relative;
    display: grid;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--btn-border);
    border-radius: 12px;
    background: rgb(0 0 0 / 0.16);
    text-align: left;
    transition:
      border-color var(--dur-fast),
      background var(--dur-fast);
  }

  .theme:hover {
    border-color: rgb(var(--accent-rgb) / 0.35);
  }

  .theme.chosen {
    border-color: rgb(var(--accent-rgb) / 0.7);
    background: rgb(var(--accent-rgb) / 0.08);
    box-shadow: 0 0 0 1px rgb(var(--accent-rgb) / 0.35);
  }

  .preview {
    display: grid;
    grid-template-columns: 26% 1fr;
    grid-template-rows: 1fr 1fr;
    gap: 5px;
    height: 74px;
    padding: 6px;
    border-radius: 8px;
    border: 1px solid rgb(255 255 255 / 0.06);
  }

  .preview i {
    border-radius: 5px;
  }

  .preview i:first-child {
    grid-row: span 2;
  }

  .preview.default {
    background: #0a0c12;
  }

  .preview.default i {
    background: linear-gradient(180deg, #171b27, #12151e);
  }

  .preview.matte {
    background: #1a1a1e;
  }

  .preview.matte i {
    background: #222226;
  }

  .preview.matte i:first-child {
    background: #121214;
  }

  .preview i:last-child {
    position: relative;
  }

  .preview i:last-child::after {
    content: "";
    position: absolute;
    right: 6px;
    bottom: 6px;
    width: 34%;
    height: 9px;
    border-radius: 4px;
    background: var(--accent-grad);
  }

  .preview.default i:last-child::after {
    background: linear-gradient(135deg, #9ba3e2, #7f8ad6, #6aa9bf);
  }

  .preview.matte i:last-child::after {
    background: #5865f2;
  }

  :global(:root.dark) .panel {
    background: var(--surface-fill);
    box-shadow: none;
  }

  :global(:root.dark) .search {
    background: var(--input-fill);
  }

  .theme-text {
    display: grid;
    gap: 1px;
    padding: 0 2px;
  }

  .theme-text b {
    font-size: 12.5px;
    font-weight: 600;
  }

  .tick {
    position: absolute;
    top: 14px;
    right: 14px;
    display: none;
    color: rgb(var(--accent-soft-rgb));
    filter: drop-shadow(0 1px 3px rgb(0 0 0 / 0.6));
  }

  .theme.chosen .tick {
    display: block;
  }

  /* Updates */

  .update {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding-top: 12px;
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

  /* About */

  .about-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .brand {
    font-family: var(--font-brand);
    font-size: 15px;
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

  /* Search: nothing found */

  .empty {
    display: grid;
    justify-items: center;
    gap: 6px;
    padding: 40px 20px;
    border: 1px dashed rgb(255 255 255 / 0.1);
    border-radius: var(--radius-lg);
    color: var(--text-3);
    text-align: center;
  }

  .empty strong {
    color: var(--text-1);
  }

  .empty .btn {
    margin-top: 8px;
  }

  @media (max-width: 900px) {
    .layout {
      grid-template-columns: 1fr;
    }

    .side {
      position: static;
    }


  }

  /* --- Compact: tabs across the top, one section at a time --- */

  .layout {
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }

  .side {
    position: static;
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 8px;
  }


  .search {
    order: 2;
    flex: 1 1 160px;
    min-width: 110px;
    max-width: 240px;
    height: 32px;
    margin-left: auto;
  }




  .content {
    gap: 14px;
  }

  .group {
    gap: 8px;
  }

  .group-head h2 {
    font-size: 14px;
  }

  .group-head p {
    margin-top: 1px;
    font-size: 11.5px;
  }

  .stack {
    gap: 10px;
  }

  .panel {
    gap: 10px;
    padding: 12px;
  }

  .panel.rows {
    padding: 4px;
  }

  .row {
    gap: 10px;
    padding: 8px 8px;
  }

  .row-icon {
    width: 26px;
    height: 26px;
  }

  .text strong {
    font-size: 12.5px;
  }

  .text small {
    font-size: 11px;
    line-height: 1.35;
  }

  .preview {
    height: 52px;
  }

  .theme {
    padding: 8px;
  }

  .update,
  .links {
    padding-top: 8px;
  }

  .empty {
    padding: 24px 16px;
  }

  .more > summary {
    padding: 8px 12px;
    border: 1px solid var(--btn-border);
    border-radius: 10px;
    background: var(--btn-fill);
    color: var(--text-2);
    font-size: 12px;
    cursor: pointer;
    list-style-position: inside;
  }

  .more > summary:hover {
    background: var(--btn-fill-hover);
    color: var(--text-1);
  }

  .more[open] > summary {
    margin-bottom: 10px;
  }

  /* --- All settings on one page, dense: two columns when there is room --- */

  .side {
    justify-content: flex-end;
  }

  .content {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
    align-items: start;
    gap: 12px 16px;
  }

  .content > .empty {
    grid-column: 1 / -1;
  }

  .group-head h2 {
    color: var(--text-2);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .group-head p {
    display: none;
  }

  .group {
    gap: 6px;
  }

  .panel {
    padding: 10px;
  }

  /* --- Responsive frame: centred, columns that fill it --- */

  .frame {
    width: 100%;
    max-width: 1240px;
    margin: 0 auto;
  }

  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }

  .side {
    position: static;
    justify-content: flex-end;
  }

  .content {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    align-items: start;
    gap: 16px;
    max-width: none;
  }

  .col {
    display: grid;
    align-content: start;
    gap: 16px;
    min-width: 0;
  }

  .col > :global(section) {
    width: 100%;
  }

  @media (max-width: 1000px) {
    .content {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
