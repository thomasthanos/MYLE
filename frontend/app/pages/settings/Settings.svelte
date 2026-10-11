<script lang="ts">
  import LocalExtensionUpdate from "../../../lib/components/LocalExtensionUpdate.svelte";
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderSync from "@lucide/svelte/icons/folder-sync";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import Moon from "@lucide/svelte/icons/moon";
  import PanelBottomClose from "@lucide/svelte/icons/panel-bottom-close";
  import Power from "@lucide/svelte/icons/power";
  import Puzzle from "@lucide/svelte/icons/puzzle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Rocket from "@lucide/svelte/icons/rocket";
  import Search from "@lucide/svelte/icons/search";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import X from "@lucide/svelte/icons/x";
  import Logo from "../../../lib/components/Logo.svelte";
  import { settings } from "../../../lib/settings.svelte";
  import { whatsNew } from "../../../lib/whats-new.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import type { UpdateAsset } from "../../../lib/updater";
  import { settingsUpdater } from "../../../lib/updater-state.svelte";
  import AccountCard from "./AccountCard.svelte";
  import SyncedData from "./SyncedData.svelte";
  import { passwordsApi, type BrowserSetup } from "../password-manager/api";
  import { EXTENSION_URL } from "../../../lib/extension";

  const REPO_URL = "https://github.com/thomasthanos/MYLE";

  /** How the browsers reach MYLE, for the extension's line; null until known. */
  let browser = $state<BrowserSetup | null>(null);
  let browserError = $state("");
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
    void passwordsApi.browserGet().then(
      (setup) => (browser = setup),
      (error) => (browserError = message(error)),
    );
  });

  /** The extension's state in a few words: what the user can do next. */
  const extensionState = $derived.by(() => {
    if (browserError) return { title: "Browser connection unknown", detail: browserError };
    if (!browser) return { title: "Checking browser…", detail: "" };
    if (!browser.enabled) return { title: "Browser filling is off", detail: "Turn it on in Password Manager → Browser filling." };
    if (browser.registrationError) return { title: "Cannot reach MYLE", detail: browser.registrationError };
    if (browser.lastContact) return { title: `Connected to ${browser.lastContact.browser}`, detail: "Autofill is active on web login fields." };
    return { title: "Not connected yet", detail: "Install extension to fill passwords in browser." };
  });

  async function setKeepInTray(value: boolean) {
    keepInTray = value;
    try {
      await invoke("tray_set", { enabled: value });
    } catch (error) {
      keepInTray = !value;
      toast.error(`Could not save setting: ${message(error)}`);
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
      toast.error(`Could not save setting: ${message(error)}`);
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

  // --- Sections and the search ---------------------
  type SectionId = "account" | "appearance" | "extension" | "startup" | "updates" | "about";

  /** The words each setting is found by, beyond its title. */
  const words: Record<string, string> = {
    account: "account sign in sign out discord google sync cloud profile saved data preferences backup login",
    theme: "appearance theme dark light navy matte slate blurple discord charcoal black grey gray colour color blue default look",
    startWindows: "startup start with windows sign in boot launch autostart",
    startMinimized: "startup minimized minimised tray hidden background launch",
    tray: "tray close quit background notification area clock shortcut ctrl shift l browser filling",
    updates: "updates update version check install download release",
    extension: "browser extension chrome web store edge brave firefox passwords autofill fill passkeys 2fa install",
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
    extension: shows("extension", "Browser extension"),
    about: shows("about", "About MYLE"),
  });

  const sectionShown = $derived<Record<SectionId, boolean>>({
    account: showing.account,
    appearance: showing.theme,
    extension: showing.extension,
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
    void openUrl(url).catch((error) => toast.error(`Could not open link: ${message(error)}`));
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="frame">
  <div class="settings-header">
    <div class="header-titles">
      <h1 class="page-title">Settings</h1>
      <p class="page-subtitle">Your account, how MYLE looks and starts, and updates.</p>
    </div>
    <label class="search-bar" aria-label="Search settings">
      <Search size={13} />
      <input
        id="settings-search"
        type="search"
        placeholder="Search settings… (Ctrl+F)"
        autocomplete="off"
        spellcheck="false"
        bind:value={query}
        onkeydown={(e) => e.key === "Escape" && query && ((query = ""), e.stopPropagation())}
      />
      {#if query}
        <button class="clear" aria-label="Clear search" onclick={() => (query = "")}>
          <X size={12} />
        </button>
      {/if}
    </label>
  </div>

  {#if nothingFound}
    <div class="empty">
      <Search size={22} />
      <strong>No setting matches “{query}”</strong>
      <small>Try searching for “theme”, “startup”, “tray”, “account” or “update”.</small>
      <button class="btn small" onclick={() => (query = "")}>Clear search</button>
    </div>
  {/if}

  <div class="content" class:hidden={nothingFound}>
    <!-- Left Column -->
    <div class="col">
      <!-- Account & Sync -->
      <section id="settings-account" class="group" hidden={hiddenSection("account")} aria-labelledby="account-heading">
        <header class="group-head">
          <h2 id="account-heading">Account &amp; sync</h2>
          <p>Sign in to sync choices and game saves.</p>
        </header>
        <div class="stack">
          <AccountCard />
          <details class="more">
            <summary class="more-summary">
              <span class="more-label">
                <FolderSync size={13} />
                <span>What is saved in your account vs. this PC</span>
              </span>
              <span class="more-chevron" aria-hidden="true">
                <ChevronDown size={13} />
              </span>
            </summary>
            <div class="more-body">
              <SyncedData />
            </div>
          </details>
        </div>
      </section>

      <!-- Startup & Tray -->
      <section id="settings-startup" class="group" hidden={hiddenSection("startup")} aria-labelledby="startup-heading">
        <header class="group-head">
          <h2 id="startup-heading">Startup &amp; tray</h2>
          <p>Windows startup options and background tray behavior.</p>
        </header>
        <div class="panel rows">
          <label class="row" hidden={!showing.startWindows}>
            <span class="row-icon"><Power size={13} /></span>
            <span class="text">
              <strong>Start with Windows</strong>
              <small>{startup.canChange ? "Opens MYLE when you sign in to Windows." : "Available in installed app."}</small>
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
            <span class="row-icon"><Minimize2 size={13} /></span>
            <span class="text">
              <strong>Start minimized</strong>
              <small>
                {startup.enabled
                  ? "Waits in system tray instead of opening on screen."
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
            <span class="row-icon"><PanelBottomClose size={13} /></span>
            <span class="text">
              <strong>Keep running in the tray</strong>
              <small>
                {keepInTray
                  ? "Closing window keeps MYLE in tray (Ctrl+Shift+L works)."
                  : "Closing window quits MYLE completely."}
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

      <!-- About -->
      <section id="settings-about" class="group" hidden={hiddenSection("about")} aria-labelledby="about-heading">
        <header class="group-head">
          <h2 id="about-heading">About</h2>
          <p>Release notes and repository.</p>
        </header>
        <div class="panel about-panel">
          <div class="about-row">
            <span class="brand-mark"><Logo size={26} /></span>
            <div class="text">
              <strong class="brand">MYLE</strong>
              <small>© 2026 ThomasThanos</small>
            </div>
          </div>
          <div class="links">
            <button class="btn small" disabled={whatsNew.loading} onclick={() => void whatsNew.showCurrent()}>
              {#if whatsNew.loading}<LoaderCircle size={12} class="spin" />{:else}<Sparkles size={12} />{/if} What's new
            </button>
            <button class="btn small" onclick={() => open(`${REPO_URL}/releases`)}>
              <ExternalLink size={12} /> Releases
            </button>
            <button class="btn small" onclick={() => open(REPO_URL)}>
              <ExternalLink size={12} /> GitHub
            </button>
          </div>
        </div>
      </section>
    </div>

    <!-- Right Column -->
    <div class="col">
      <!-- Appearance -->
      <section id="settings-appearance" class="group" hidden={hiddenSection("appearance")} aria-labelledby="appearance-heading">
        <header class="group-head">
          <h2 id="appearance-heading">Appearance</h2>
          <p>Pick a theme.</p>
        </header>
        <div class="panel appearance-panel">
          <div class="setting-head">
            <span class="row-icon"><Moon size={13} /></span>
            <span class="text">
              <strong>Theme</strong>
              <small>Choose surfaces and accents.</small>
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
              <div class="mockup default" aria-hidden="true">
                <div class="mock-chrome">
                  <span class="dot red"></span>
                  <span class="dot yellow"></span>
                  <span class="dot green"></span>
                  <span class="mock-title"></span>
                </div>
                <div class="mock-body">
                  <div class="mock-sidebar">
                    <span class="mock-nav active"></span>
                    <span class="mock-nav"></span>
                  </div>
                  <div class="mock-canvas">
                    <div class="mock-card">
                      <span class="mock-line title"></span>
                      <span class="mock-line desc"></span>
                    </div>
                    <div class="mock-action-pill"></div>
                  </div>
                </div>
              </div>
              <div class="theme-footer">
                <span class="theme-text">
                  <b>Default</b>
                  <small>Black &amp; Blue</small>
                </span>
                <span class="tick" aria-hidden="true"><CircleCheck size={14} /></span>
              </div>
            </button>
            <button
              class="theme"
              role="radio"
              aria-checked={settings.dark}
              class:chosen={settings.dark}
              onclick={() => settings.setDark(true)}
            >
              <div class="mockup navy" aria-hidden="true">
                <div class="mock-chrome">
                  <span class="dot red"></span>
                  <span class="dot yellow"></span>
                  <span class="dot green"></span>
                  <span class="mock-title"></span>
                </div>
                <div class="mock-body">
                  <div class="mock-sidebar">
                    <span class="mock-nav active"></span>
                    <span class="mock-nav"></span>
                  </div>
                  <div class="mock-canvas">
                    <div class="mock-card">
                      <span class="mock-line title"></span>
                      <span class="mock-line desc"></span>
                    </div>
                    <div class="mock-action-pill"></div>
                  </div>
                </div>
              </div>
              <div class="theme-footer">
                <span class="theme-text">
                  <b>Navy</b>
                  <small>Navy Glass depth</small>
                </span>
                <span class="tick" aria-hidden="true"><CircleCheck size={14} /></span>
              </div>
            </button>
          </div>
        </div>
      </section>

      <!-- Browser Extension -->
      <section id="settings-extension" class="group" hidden={hiddenSection("extension")} aria-labelledby="extension-heading">
        <header class="group-head">
          <h2 id="extension-heading">Browser extension</h2>
          <p>MYLE Passwords extension autofill.</p>
        </header>
        <div class="panel extension-panel">
          <div class="extension-row">
            <span class="row-icon"><Puzzle size={13} /></span>
            <span class="text">
              <strong>{extensionState.title}</strong>
              {#if extensionState.detail}<small>{extensionState.detail}</small>{/if}
            </span>
            {#if EXTENSION_URL.startsWith("https://")}
              <button class="btn small primary" onclick={() => open(EXTENSION_URL)}>
                <Download size={12} /> Install from Chrome Web Store
              </button>
            {/if}
          </div>
        </div>
      </section>

      <!-- Updates -->
      <section id="settings-updates" class="group" hidden={hiddenSection("updates")} aria-labelledby="updates-heading">
        <header class="group-head">
          <h2 id="updates-heading">Updates</h2>
          <p>Application version and releases.</p>
        </header>
        <LocalExtensionUpdate />
        <div class="panel updates-panel">
          <div class="setting-head">
            <span class="row-icon"><Rocket size={13} /></span>
            <span class="text">
              <strong>MYLE {version ? `v${version}` : "(dev preview)"}</strong>
              <small>Auto updates via downloads.thomast.uk</small>
            </span>
            <span class="version-pill">{version ? `v${version}` : "dev"}</span>
          </div>
          <div class="update" aria-live="polite">
            {#if update.state === "idle"}
              <span class="status"><span class="ok-dot" aria-hidden="true"></span> Automatic updates active</span>
            {:else if update.state === "checking"}
              <span class="status"><LoaderCircle size={13} class="spin" /> Checking…</span>
            {:else if update.state === "upToDate"}
              <span class="status ok"><CircleCheck size={13} /> Up to date</span>
            {:else if update.state === "available"}
              {@const available = update}
              <span class="status accent"><Download size={13} /> v{available.latest} available</span>
              <button class="btn small primary" onclick={() => install(available.latest, available.asset)}>
                Install
              </button>
            {:else if update.state === "downloading"}
              <span class="status">
                <LoaderCircle size={13} class="spin" /> Downloading… {update.detail}
              </span>
              <span class="bar" class:indeterminate={update.progress === null}>
                <span style:transform={update.progress === null ? undefined : `scaleX(${update.progress})`}></span>
              </span>
            {:else if update.state === "verifying"}
              <span class="status"><LoaderCircle size={13} class="spin" /> Verifying…</span>
            {:else if update.state === "installing"}
              <span class="status"><LoaderCircle size={13} class="spin" /> Installing…</span>
            {:else if update.state === "restarting"}
              <span class="status"><LoaderCircle size={13} class="spin" /> Restarting…</span>
            {:else if update.state === "error"}
              <span class="status error" title={update.message}>{update.message}</span>
            {/if}
            {#if update.state === "idle" || update.state === "upToDate" || update.state === "error"}
              <button class="btn small" onclick={check}><RefreshCw size={12} /> Check</button>
            {/if}
          </div>
        </div>
      </section>
    </div>
  </div>
</div>

<style>
  /* --- Layout Frame & Scroller Tuning --- */
  .frame { width: 100%; margin: 0 auto; padding-bottom: 4px; }

  /* Scroller padding restriction: ensure settings page fits without scroll */
  :global(.scroller:has(.frame)) { padding: 16px 26px !important; }

  /* --- Top Header with Integrated Search --- */
  .settings-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 12px; }
  .header-titles { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .page-title { font-family: var(--font-handwritten); font-size: 21px; font-weight: 700; font-synthesis: none; line-height: 1.25; letter-spacing: 0; background: linear-gradient(180deg, #ffffff 15%, #c5cde0 100%); -webkit-background-clip: text; background-clip: text; -webkit-text-fill-color: transparent; filter: drop-shadow(0 2px 8px rgb(0 0 0 / 0.45)); margin: 0; }
  .page-subtitle { margin: 0; color: var(--text-3); font-size: 11.5px; line-height: 1.3; }

  /* Compact search bar */
  .search-bar { display: flex; align-items: center; gap: 7px; width: 220px; height: 30px; padding: 0 8px 0 10px; border: 1px solid var(--glass-border); border-radius: 9px; background: var(--input-fill); color: var(--text-3); box-shadow: inset 0 1px 2px rgb(0 0 0 / 0.2); transition: border-color var(--dur-fast), background var(--dur-fast); }
  .search-bar:focus-within { border-color: rgb(var(--accent-rgb) / 0.6); background: var(--surface-fill); box-shadow: 0 0 0 2px rgb(var(--accent-rgb) / 0.14); color: rgb(var(--accent-soft-rgb)); }
  .search-bar input { flex: 1; min-width: 0; height: 100%; padding: 0; border: 0; outline: none; background: transparent; color: var(--text-1); font: inherit; font-size: 11.5px; }
  .search-bar input::placeholder { color: var(--text-3); }
  .search-bar input::-webkit-search-cancel-button { display: none; }
  .clear { display: grid; place-items: center; width: 18px; height: 18px; border-radius: 5px; color: var(--text-3); }
  .clear:hover { background: var(--hover); color: var(--text-1); }

  /* --- Grid Content Columns --- */
  .content { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: start; gap: 12px; }
  .content.hidden { display: none; }
  .col { display: grid; align-content: start; gap: 10px; min-width: 0; }

  /* --- Section Groups (Crisp, zero-bloat headings) --- */
  .group { display: grid; gap: 4px; scroll-margin-top: 10px; }
  .group[hidden],
  .row[hidden] { display: none; }
  .group-head { display: flex; align-items: center; justify-content: space-between; padding-left: 2px; }
  .group-head h2 { font-size: 10.5px; font-weight: 700; letter-spacing: 0.07em; text-transform: uppercase; color: var(--text-3); margin: 0; }
  .group-head p { display: none; /* Hide verbose subtitle to prevent any scroll overflow */ }

  .stack { display: grid; gap: 8px; }

  /* --- Panels --- */
  .panel { display: grid; gap: 8px; padding: 10px 12px; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.05), var(--surface-depth); }
  .panel.rows { gap: 0; padding: 3px; }
  .setting-head { display: flex; align-items: center; gap: 9px; }

  .version-pill { padding: 1px 7px; border: 1px solid rgb(var(--accent-rgb) / 0.28); border-radius: 999px; background: rgb(var(--accent-rgb) / 0.1); color: rgb(var(--accent-soft-rgb)); font-family: var(--font-mono); font-size: 10px; font-weight: 600; white-space: nowrap; }

  .row { display: flex; align-items: center; gap: 9px; padding: 6px 8px; border-radius: 8px; cursor: pointer; transition: background var(--dur-fast); }
  .row + .row { border-top: 1px solid rgb(255 255 255 / 0.04); border-top-left-radius: 0; border-top-right-radius: 0; }
  .row:hover { background: var(--hover); }
  .row:has(input:disabled) { cursor: default; }
  .row:has(input:disabled) .text { opacity: 0.7; }

  .row-icon { display: grid; place-items: center; width: 24px; height: 24px; flex: none; border: 1px solid rgb(var(--accent-rgb) / 0.2); border-radius: 6px; background: rgb(var(--accent-rgb) / 0.08); color: rgb(var(--accent-soft-rgb)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08); }

  .text { display: grid; gap: 1px; flex: 1; min-width: 0; }
  strong { font-size: 12px; font-weight: 600; }
  small { color: var(--text-3); font-size: 10.5px; line-height: 1.25; }

  /* Compact Switches */
  :global(.switch) { width: 34px !important; height: 18px !important; }
  :global(.switch::before) { width: 14px !important; height: 14px !important; top: 2px !important; left: 2px !important; }
  :global(.switch:checked::before) { transform: translateX(16px) !important; }

  /* --- Accordion Details --- */
  .more { border: 1px solid var(--glass-border); border-radius: var(--radius-sm); background: var(--surface-fill); overflow: hidden; transition: border-color var(--dur-fast); }
  .more:hover { border-color: rgb(var(--accent-rgb) / 0.35); }
  .more-summary { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 10px; cursor: pointer; user-select: none; list-style: none; color: var(--text-2); font-size: 11.5px; font-weight: 500; transition: background var(--dur-fast), color var(--dur-fast); }
  .more-summary::-webkit-details-marker { display: none; }
  .more-summary:hover { background: var(--hover); color: var(--text-1); }
  .more-label { display: flex; align-items: center; gap: 7px; min-width: 0; }
  .more-label :global(svg) { flex: none; color: rgb(var(--accent-soft-rgb)); }
  .more-chevron { display: grid; place-items: center; color: var(--text-3); transition: transform 200ms cubic-bezier(0.16, 1, 0.3, 1); }
  .more[open] .more-chevron { transform: rotate(180deg); color: rgb(var(--accent-soft-rgb)); }
  .more[open] .more-summary { border-bottom: 1px solid var(--glass-border); }
  .more-body { padding: 6px; background: rgb(0 0 0 / 0.15); }

  /* --- Themes Radio Cards --- */
  .themes { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .theme { position: relative; display: flex; flex-direction: column; gap: 6px; padding: 7px; border: 1px solid var(--glass-border); border-radius: 9px; background: var(--input-fill); text-align: left; box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.04); cursor: pointer; transition: border-color var(--dur-fast), background var(--dur-fast); }
  .theme:hover { border-color: rgb(var(--accent-rgb) / 0.45); background: var(--surface-fill); }
  .theme.chosen { border-color: rgb(var(--accent-rgb) / 0.8); background: rgb(var(--accent-rgb) / 0.09); box-shadow: 0 0 0 1px rgb(var(--accent-rgb) / 0.45); }

  /* Compact Miniature App Mockups */
  .mockup { display: flex; flex-direction: column; height: 46px; border-radius: 6px; overflow: hidden; box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08), 0 3px 8px -2px rgb(0 0 0 / 0.5); }
  .mock-chrome { display: flex; align-items: center; gap: 3px; height: 11px; padding: 0 5px; border-bottom: 1px solid rgb(255 255 255 / 0.05); }
  .mock-chrome .dot { width: 3.5px; height: 3.5px; border-radius: 50%; }
  .mock-chrome .dot.red { background: #ff5f56; }
  .mock-chrome .dot.yellow { background: #ffbd2e; }
  .mock-chrome .dot.green { background: #27c93f; }
  .mock-title { width: 18px; height: 2px; margin-left: 3px; border-radius: 1px; background: rgb(255 255 255 / 0.12); }
  .mock-body { display: grid; grid-template-columns: 24% 1fr; flex: 1; min-height: 0; }
  .mock-sidebar { display: flex; flex-direction: column; gap: 3px; padding: 4px; border-right: 1px solid rgb(255 255 255 / 0.04); }
  .mock-nav { width: 100%; height: 3.5px; border-radius: 1.5px; }
  .mock-canvas { display: flex; flex-direction: column; justify-content: space-between; padding: 4px 6px; }
  .mock-card { display: flex; flex-direction: column; gap: 2px; padding: 3px 4px; border-radius: 3px; }
  .mock-line { border-radius: 1px; }
  .mock-line.title { width: 45%; height: 2.5px; }
  .mock-line.desc { width: 75%; height: 1.5px; }
  .mock-action-pill { align-self: flex-end; width: 20px; height: 5px; border-radius: 2px; }

  /* Mockup: Default (Black & Blue) */
  .mockup.default { background: #0d0d0d; border: 1px solid rgb(255 255 255 / 0.1); }
  .mockup.default .mock-chrome { background: #0f0f0f; }
  .mockup.default .mock-sidebar { background: #161616; }
  .mockup.default .mock-nav { background: #24304c; }
  .mockup.default .mock-nav.active { background: #6b85ff; }
  .mockup.default .mock-canvas { background: #171717; }
  .mockup.default .mock-card { background: #262626; border: 1px solid rgb(255 255 255 / 0.1); }
  .mockup.default .mock-line.title { background: #969696; }
  .mockup.default .mock-line.desc { background: #505050; }
  .mockup.default .mock-action-pill { background: #4960da; }

  /* Mockup: Navy */
  .mockup.navy { background: #1b202b; border: 1px solid rgb(255 255 255 / 0.1); }
  .mockup.navy .mock-chrome { background: #171c26; }
  .mockup.navy .mock-sidebar { background: #141820; }
  .mockup.navy .mock-nav { background: #252d3d; }
  .mockup.navy .mock-nav.active { background: #6875d1; box-shadow: 0 0 4px rgb(104 117 209 / 0.5); }
  .mockup.navy .mock-canvas { background: #1b202b; }
  .mockup.navy .mock-card { background: #242a37; border: 1px solid rgb(255 255 255 / 0.08); }
  .mockup.navy .mock-line.title { background: #4a5775; }
  .mockup.navy .mock-line.desc { background: #2f3747; }
  .mockup.navy .mock-action-pill { background: #6875d1; }

  .theme-footer { display: flex; align-items: center; justify-content: space-between; gap: 6px; }
  .theme-text { display: grid; gap: 1px; }
  .theme-text b { font-size: 11.5px; font-weight: 600; color: var(--text-1); }
  .theme-text small { color: var(--text-3); font-size: 9.5px; line-height: 1.2; }
  .tick { display: none; color: rgb(var(--accent-soft-rgb)); }
  .theme.chosen .tick { display: block; }

  /* --- Extension Single-Row Panel --- */
  .extension-row { display: flex; align-items: center; gap: 9px; }
  .extension-row .btn { height: 25px; padding: 0 9px; font-size: 11px; flex: none; }

  /* --- Updates Compact Panel --- */
  .update { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 6px; padding-top: 6px; border-top: 1px solid rgb(255 255 255 / 0.05); }
  .status { display: inline-flex; align-items: center; gap: 6px; flex: 1; min-width: 0; color: var(--text-2); font-size: 11px; }
  .status.ok { color: rgb(110 225 175); }
  .status.accent { color: rgb(var(--accent-soft-rgb)); }
  .status.error { overflow: hidden; color: rgb(255 170 150 / 0.9); white-space: nowrap; text-overflow: ellipsis; }
  .ok-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--ok); box-shadow: 0 0 6px var(--ok-glow); }
  .bar { position: relative; flex-basis: 100%; height: 3px; overflow: hidden; border-radius: 999px; background: rgb(0 0 0 / 0.3); }
  .bar span { position: absolute; inset: 0; border-radius: inherit; background: var(--accent-grad); transform-origin: left; transition: transform 160ms linear; }
  .bar.indeterminate span { width: 34%; animation: sweep 1.2s var(--ease-in-out) infinite; }
  @keyframes sweep {
    from { transform: translateX(-100%); }
    to { transform: translateX(300%); }
  }
  .update .btn { height: 25px; padding: 0 9px; font-size: 11px; }

  /* --- About Compact Panel --- */
  .about-panel { gap: 6px; }
  .about-row { display: flex; align-items: center; gap: 10px; }
  .brand { font-family: var(--font-brand); font-size: 13px; color: var(--text-1); }
  .brand-mark { display: grid; place-items: center; flex: none; }
  .links { display: flex; flex-wrap: wrap; gap: 6px; padding-top: 6px; border-top: 1px solid rgb(255 255 255 / 0.05); }
  .links .btn { height: 25px; padding: 0 8px; font-size: 11px; }

  /* --- Empty State --- */
  .empty { display: grid; justify-items: center; gap: 5px; padding: 30px 16px; border: 1px dashed rgb(255 255 255 / 0.12); border-radius: var(--radius-md); background: rgb(0 0 0 / 0.1); color: var(--text-3); text-align: center; }
  .empty strong { color: var(--text-1); font-size: 13px; }
  .empty .btn { margin-top: 8px; height: 26px; font-size: 11px; }

  /* ============================================================== */
  /* --- 3-TIER ZERO-SCROLL RESPONSIVE ARCHITECTURE ---             */
  /* ============================================================== */

  /* TIER 1: Ultra-Wide / 1440p / 4K Displays (large canvas, generous scale) */
  @media (min-width: 1550px) and (min-height: 850px), (min-width: 2200px) {
    .frame {
      max-width: 1680px;
    }
    :global(.scroller:has(.frame)) {
      padding: 26px 36px !important;
    }
    .settings-header {
      margin-bottom: 22px;
    }
    .page-title {
      font-size: 28px;
    }
    .page-subtitle {
      font-size: 14px;
    }
    .search-bar {
      width: 320px;
      height: 36px;
      border-radius: 11px;
      padding: 0 12px;
    }
    .search-bar input {
      font-size: 13px;
    }
    .content {
      gap: 24px;
    }
    .col {
      gap: 18px;
    }
    .group {
      gap: 7px;
    }
    .group-head h2 {
      font-size: 12.5px;
      letter-spacing: 0.08em;
    }
    .group-head p {
      display: block;
      font-size: 12px;
      color: var(--text-3);
      margin: 0;
    }
    .panel {
      padding: 16px 18px;
      gap: 12px;
      border-radius: 14px;
    }
    .panel.rows {
      padding: 5px;
    }
    .row {
      padding: 9px 12px;
      gap: 12px;
      border-radius: 9px;
    }
    .row-icon {
      width: 32px;
      height: 32px;
      border-radius: 8px;
    }
    strong {
      font-size: 13.5px;
    }
    small {
      font-size: 12px;
      line-height: 1.35;
    }
    :global(.switch) {
      width: 44px !important;
      height: 24px !important;
    }
    :global(.switch::before) {
      width: 18px !important;
      height: 18px !important;
      top: 3px !important;
      left: 3px !important;
    }
    :global(.switch:checked::before) {
      transform: translateX(20px) !important;
    }
    .themes {
      gap: 14px;
    }
    .theme {
      padding: 11px;
      border-radius: 12px;
    }
    .mockup {
      height: 72px;
      border-radius: 8px;
    }
    .mock-chrome {
      height: 15px;
      padding: 0 7px;
      gap: 4px;
    }
    .mock-chrome .dot {
      width: 4.5px;
      height: 4.5px;
    }
    .mock-title {
      width: 24px;
      height: 3px;
    }
    .mock-sidebar {
      padding: 6px;
      gap: 4px;
    }
    .mock-nav {
      height: 4.5px;
    }
    .mock-canvas {
      padding: 6px 8px;
    }
    .mock-line.title {
      height: 3.5px;
    }
    .mock-line.desc {
      height: 2px;
    }
    .mock-action-pill {
      width: 26px;
      height: 7px;
    }
    .theme-footer {
      margin-top: 8px;
      padding: 0 4px;
    }
    .theme-text b {
      font-size: 13.5px;
    }
    .theme-text small {
      font-size: 11.5px;
    }
    .more-summary {
      padding: 9px 14px;
      font-size: 12.5px;
    }
    .more-label {
      gap: 8px;
      font-size: 12.5px;
    }
    .version-pill {
      font-size: 11px;
      padding: 2px 9px;
    }
    .about-row {
      gap: 14px;
    }
    .brand {
      font-size: 15px;
    }
    .links .btn,
    .extension-row .btn {
      height: 29px;
      padding: 0 11px;
      font-size: 12px;
    }
  }

  /* TIER 2: Standard Desktop 1080p (901px to 1549px, standard vertical room) */
  @media (min-width: 901px) and (max-width: 1549px) {
    .frame {
      max-width: 1180px;
    }
    .content {
      gap: 14px;
    }
    .col {
      gap: 11px;
    }
  }

  /* TIER 3: Compact 720p Window / Laptops / Low-Height (max-height: 750px), (max-width: 900px) */
  @media (max-height: 750px), (max-width: 900px) {
    .frame {
      max-width: 100%;
    }
    :global(.scroller:has(.frame)) {
      padding: 12px 18px !important;
    }
    .settings-header {
      margin-bottom: 8px;
    }
    .page-title {
      font-size: 20px;
    }
    .page-subtitle {
      font-size: 11px;
    }
    .search-bar {
      width: 210px;
      height: 27px;
      font-size: 11px;
    }
    .content {
      gap: 10px;
    }
    .col {
      gap: 8px;
    }
    .group {
      gap: 3px;
    }
    .group-head h2 {
      font-size: 10px;
    }
    .group-head p {
      display: none;
    }
    .panel {
      padding: 8px 10px;
      gap: 6px;
    }
    .panel.rows {
      padding: 2px;
    }
    .row {
      padding: 5px 7px;
      gap: 8px;
    }
    .row-icon {
      width: 22px;
      height: 22px;
    }
    strong {
      font-size: 11.5px;
    }
    small {
      font-size: 10px;
    }
    :global(.switch) {
      width: 32px !important;
      height: 17px !important;
    }
    :global(.switch::before) {
      width: 13px !important;
      height: 13px !important;
      top: 2px !important;
      left: 2px !important;
    }
    :global(.switch:checked::before) {
      transform: translateX(15px) !important;
    }
    .mockup {
      height: 42px;
    }
    .links .btn,
    .extension-row .btn {
      height: 24px;
      font-size: 10.5px;
    }
  }

  /* Single Column Layout for narrow/split screens */
  @media (max-width: 760px) {
    .settings-header {
      flex-direction: column;
      align-items: stretch;
      gap: 10px;
      margin-bottom: 12px;
    }
    .search-bar {
      width: 100%;
      height: 32px;
    }
    .content {
      grid-template-columns: minmax(0, 1fr);
      gap: 12px;
    }
    .col {
      gap: 10px;
    }
  }

  @media (max-width: 560px) {
    .themes {
      grid-template-columns: 1fr;
    }
  }
</style>
