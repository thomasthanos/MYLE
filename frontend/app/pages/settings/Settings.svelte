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
  import Info from "@lucide/svelte/icons/info";
  import Moon from "@lucide/svelte/icons/moon";
  import Search from "@lucide/svelte/icons/search";
  import UserRound from "@lucide/svelte/icons/user-round";
  import X from "@lucide/svelte/icons/x";
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
    | { state: "restarting"; version: string }
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
        } else if (event.event === "restarting") {
          // This window stays until the new version's is on screen.
          update = { state: "restarting", version: event.data.version || latest };
        }
      });
    } catch (error) {
      update = { state: "error", message: message(error) };
      toast.error(`The update failed: ${message(error)}`);
    }
  }

  // --- Sections, the side navigation and the search ---------------------

  type SectionId = "account" | "appearance" | "startup" | "updates" | "about";
  const sections: { id: SectionId; label: string; hint: string; icon: typeof Palette }[] = [
    { id: "account", label: "Account & sync", hint: "Sign in, and what follows you to other PCs", icon: UserRound },
    { id: "appearance", label: "Appearance", hint: "The theme of every window", icon: Palette },
    { id: "startup", label: "Startup & tray", hint: "How MYLE starts and keeps running", icon: Power },
    { id: "updates", label: "Updates", hint: "Your version, and new ones", icon: Rocket },
    { id: "about", label: "About", hint: "Links, release notes", icon: Info },
  ];

  /** The words each setting is found by, beyond its title. */
  const words: Record<string, string> = {
    account: "account sign in sign out discord google sync cloud profile saved data preferences backup login",
    theme: "appearance theme dark light charcoal black grey gray colour color blue default look",
    startWindows: "startup start with windows sign in boot launch autostart",
    startMinimized: "startup minimized minimised tray hidden background launch",
    tray: "tray close quit background notification area clock shortcut ctrl shift l browser filling",
    updates: "updates update version check install download release",
    about: "about github repository releases what's new changelog notes version",
  };

  let query = $state("");
  let active = $state<SectionId>("account");
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

  let content = $state<HTMLElement>();

  function goTo(id: SectionId) {
    const target = content?.querySelector<HTMLElement>(`#settings-${id}`);
    const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
    target?.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
  }

  // The section in view is the one marked in the navigation: the last one
  // whose heading has passed the top of the page (the last one at the end).
  $effect(() => {
    if (!content) return;
    const scroller = content.closest<HTMLElement>(".scroller");
    const target: HTMLElement | Window = scroller ?? window;
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (!content) return;
        const shown = sections.filter((section) => sectionShown[section.id]);
        const top = (scroller?.getBoundingClientRect().top ?? 0) + (scroller?.clientHeight ?? innerHeight) * 0.35;
        const atEnd = scroller ? scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4 : false;
        let current = shown[0]?.id;
        for (const section of shown) {
          const element = content.querySelector<HTMLElement>(`#settings-${section.id}`);
          if (element && element.getBoundingClientRect().top <= top) current = section.id;
        }
        if (atEnd && shown.length) current = shown[shown.length - 1].id;
        if (current) active = current;
      });
    };
    update();
    target.addEventListener("scroll", update, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      target.removeEventListener("scroll", update);
    };
  });

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

    <ul>
      {#each sections as section (section.id)}
        {@const Icon = section.icon}
        <li>
          <button
            class="nav-item"
            class:active={active === section.id && !terms.length}
            disabled={!sectionShown[section.id]}
            aria-current={active === section.id ? "true" : undefined}
            onclick={() => goTo(section.id)}
          >
            <span class="nav-icon"><Icon size={15} /></span>
            <span class="nav-text">
              <b>{section.label}</b>
              <small>{section.hint}</small>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </nav>

  <div class="content" bind:this={content}>
    {#if nothingFound}
      <div class="empty">
        <Search size={22} />
        <strong>No setting matches “{query}”</strong>
        <small>Try another word, like “theme”, “startup”, “tray” or “update”.</small>
        <button class="btn small" onclick={() => (query = "")}>Clear the search</button>
      </div>
    {/if}

    <section id="settings-account" class="group" hidden={!sectionShown.account} aria-labelledby="account-heading">
      <header class="group-head">
        <h2 id="account-heading">Account &amp; sync</h2>
        <p>Sign in to keep your choices the same on every PC. Everything works without an account too.</p>
      </header>
      <div class="stack">
        <AccountCard />
        <SyncedData />
      </div>
    </section>

    <section id="settings-appearance" class="group" hidden={!sectionShown.appearance} aria-labelledby="appearance-heading">
      <header class="group-head">
        <h2 id="appearance-heading">Appearance</h2>
        <p>Pick a theme. It applies at once, to every MYLE window.</p>
      </header>
      <div class="panel">
        <div class="setting-head">
          <span class="row-icon"><Moon size={15} /></span>
          <span class="text">
            <strong>Theme</strong>
            <small>The accent colour, sizes and layout stay the same in both.</small>
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
            <span class="preview charcoal" aria-hidden="true"><i></i><i></i><i></i></span>
            <span class="theme-text">
              <b>Dark</b>
              <small>Charcoal grey-black, neutral text</small>
            </span>
            <span class="tick" aria-hidden="true"><CircleCheck size={16} /></span>
          </button>
        </div>
      </div>
    </section>

    <section id="settings-startup" class="group" hidden={!sectionShown.startup} aria-labelledby="startup-heading">
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

    <section id="settings-updates" class="group" hidden={!sectionShown.updates} aria-labelledby="updates-heading">
      <header class="group-head">
        <h2 id="updates-heading">Updates</h2>
        <p>MYLE checks for a new version every time it starts. You can also check now.</p>
      </header>
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

    <section id="settings-about" class="group" hidden={!sectionShown.about} aria-labelledby="about-heading">
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

  .side ul {
    display: grid;
    gap: 3px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 9px;
    border: 1px solid transparent;
    border-radius: 10px;
    color: var(--text-2);
    text-align: left;
    transition:
      background var(--dur-fast),
      border-color var(--dur-fast),
      color var(--dur-fast);
  }

  .nav-item:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text-1);
  }

  .nav-item.active {
    border-color: rgb(var(--accent-rgb) / 0.22);
    background: var(--selected);
    color: var(--text-1);
  }

  .nav-item:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .nav-icon {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    flex: none;
    border-radius: 8px;
    background: rgb(255 255 255 / 0.04);
    color: rgb(var(--accent-soft-rgb) / 0.9);
  }

  .nav-item.active .nav-icon {
    background: rgb(var(--accent-rgb) / 0.16);
  }

  .nav-text {
    display: grid;
    min-width: 0;
  }

  .nav-text b {
    font-size: 12.5px;
    font-weight: 600;
  }

  .nav-text small {
    overflow: hidden;
    color: var(--text-3);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
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

  .preview.charcoal {
    background: #0b0b0c;
  }

  .preview.charcoal i {
    background: linear-gradient(180deg, #1c1c1e, #161618);
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

  .preview.charcoal i:last-child::after {
    background: linear-gradient(180deg, #6f78c6, #5b63b0);
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

    .side ul {
      display: flex;
      flex-wrap: wrap;
    }

    .nav-item {
      width: auto;
    }

    .nav-text small {
      display: none;
    }
  }
</style>
