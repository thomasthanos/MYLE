<script lang="ts">
  import { onMount } from "svelte";
  import AppWindow from "@lucide/svelte/icons/app-window";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Image from "@lucide/svelte/icons/image";
  import Globe from "@lucide/svelte/icons/globe";
  import CloudAlert from "@lucide/svelte/icons/cloud-alert";
  import Download from "@lucide/svelte/icons/download";
  import Fingerprint from "@lucide/svelte/icons/fingerprint";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import UserKey from "@lucide/svelte/icons/user-key";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Star from "@lucide/svelte/icons/star";
  import Timer from "@lucide/svelte/icons/timer";
  import Upload from "@lucide/svelte/icons/upload";
  import Popover from "../../../lib/components/Popover.svelte";
  import { DEVICE, MOBILE, QUICK_UNLOCK } from "../../../lib/platform";
  import { toast } from "../../../lib/toast.svelte";
  import { passwordsApi as api } from "./api";
  import EntryEditor from "./EntryEditor.svelte";
  import EntryView from "./EntryView.svelte";
  import Favicon from "./Favicon.svelte";
  import Generator from "./Generator.svelte";
  import { passwords as p, type Filter } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";
  import SyncStatus from "./SyncStatus.svelte";
  import BrowserFilling from "./BrowserFilling.svelte";
  import WindowsFill from "./WindowsFill.svelte";
  import VaultDialog, { type DialogKind } from "./VaultDialog.svelte";

  let dialog = $state<DialogKind | null>(null);
  let browserOpen = $state(false);
  /** The hotkey for Windows programs; null when taken, undefined until known. */
  let windowsHotkey = $state<string | null | undefined>(undefined);

  onMount(() => {
    if (MOBILE) return;
    void api.windowsHotkey().then((label) => (windowsHotkey = label)).catch(() => (windowsHotkey = null));
  });

  const filters: { id: Filter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "favorites", label: "Favorites" },
    { id: "weak", label: "Weak" },
    { id: "reused", label: "Reused" },
  ];
  const lockTimes = [1, 5, 15, 60, 0];

  async function startImport() {
    try {
      const preview = await api.importPick();
      if (preview) dialog = { kind: "import", preview };
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  }

  function host(url: string) {
    try {
      return new URL(/^[a-z]+:\/\//i.test(url) ? url : `https://${url}`).hostname.replace(/^www\./, "");
    } catch {
      return url;
    }
  }

  const selectedId = $derived(p.panel.kind === "view" || p.panel.kind === "edit" ? p.panel.id : null);
</script>

<div class="vault">
  <div class="toolbar">
    <label class="search">
      <Search size={15} />
      <input placeholder="Search names, user names, sites…" bind:value={p.query} spellcheck="false" />
    </label>
    <button class="btn primary" title="New login" onclick={() => (p.panel = { kind: "edit", id: null })}><Plus size={15} /> <span class="label">New</span></button>
    <Popover align="end">
      {#snippet trigger({ toggle })}
        <button class="btn" title="Password generator" onclick={toggle}><Sparkles size={14} /> <span class="label">Generator</span></button>
      {/snippet}
      {#snippet children()}
        <div class="menu"><Generator /></div>
      {/snippet}
    </Popover>
    <Popover align="end">
      {#snippet trigger({ toggle })}
        <button class="icon-btn more" aria-label="More" onclick={toggle}><Ellipsis size={17} /></button>
      {/snippet}
      {#snippet children({ close })}
        <div class="menu">
          {#if !MOBILE}
            <button class="menu-item" onclick={() => (close(), startImport())}><Download size={15} /> Import passwords…</button>
            <button class="menu-item" onclick={() => (close(), (dialog = { kind: "export" }))}><Upload size={15} /> Export encrypted backup…</button>
            <button class="menu-item" onclick={() => (close(), (browserOpen = true))}><Globe size={15} /> Browser filling…</button>
          {/if}
          <button class="menu-item" onclick={() => (close(), p.setWebsiteIcons(!p.websiteIcons))}>
            <Image size={15} /> {p.websiteIcons ? "Hide website icons" : "Show website icons"}
          </button>
          <button class="menu-item" onclick={() => (close(), (dialog = { kind: "master" }))}><KeyRound size={15} /> Change master password…</button>
          {#if p.hello.available}
            <button class="menu-item" onclick={() => (close(), p.setHello(!p.hello.enabled))}>
              <Fingerprint size={15} />
              {p.hello.enabled ? `Stop using ${QUICK_UNLOCK}` : `Open with ${QUICK_UNLOCK}…`}
            </button>
          {/if}
          <div class="menu-label"><Timer size={11} /> Lock after</div>
          <div class="lock-times">
            {#each lockTimes as minutes (minutes)}
              <button class="chip" class:active={p.autoLockMinutes === minutes} onclick={() => p.setAutoLock(minutes)}>
                {minutes === 0 ? "Never" : minutes === 60 ? "1 h" : `${minutes} min`}
              </button>
            {/each}
          </div>
        </div>
      {/snippet}
    </Popover>
    <!-- On a phone the bar at the top has it. -->
    {#if !MOBILE}
      <button class="icon-btn" title="Lock now" aria-label="Lock the vault" onclick={() => p.lock()}><Lock size={16} /></button>
    {/if}
  </div>

  {#if p.sync.kind === "otherVault"}
    <div class="other-vault">
      <CloudAlert size={16} />
      <span>
        <strong>Your account holds a different password vault.</strong>
        This {DEVICE}'s vault is not synced. Use the account's vault here, or keep this one only on this {DEVICE}.
      </span>
      <button class="btn small" onclick={() => p.useAccountVault()}>Use the account's vault</button>
    </div>
  {/if}

  {#if p.damaged > 0}
    <div class="other-vault">
      <ShieldAlert size={16} />
      <span>
        <strong>{p.damaged} {p.damaged === 1 ? "entry does" : "entries do"} not open with this vault's key.</strong>
        {p.damaged === 1 ? "It was" : "They were"} damaged or changed outside MYLE, so {p.damaged === 1 ? "it is" : "they are"} left out and never filled. Restore a backup to get {p.damaged === 1 ? "it" : "them"} back.
      </span>
    </div>
  {/if}

  {#if !MOBILE}
    <WindowsFill hotkey={windowsHotkey} />

    {#if windowsHotkey === null}
      <p class="windows-hotkey-error">Filling Windows programs is off: other programs hold both Ctrl+Shift+L and Ctrl+Alt+Shift+L.</p>
    {/if}
  {/if}

  <div class="filters">
    {#each filters as f (f.id)}
      <button class="chip" class:active={p.filter === f.id} aria-pressed={p.filter === f.id} onclick={() => (p.filter = f.id)}>
        {f.label} <span class="count">{p.counts[f.id]}</span>
      </button>
    {/each}
    <SyncStatus />
  </div>

  <div class="split" class:has-selection={p.panel.kind !== "none"}>
    <div class="list glass">
      <div class="list-head">
        <div>
          <h2>Saved logins</h2>
          <span>{p.visible.length === p.entries.length ? `${p.entries.length} in your vault` : `${p.visible.length} of ${p.entries.length} shown`}</span>
        </div>
        <KeyRound size={17} aria-hidden="true" />
      </div>
      <div class="list-scroll" role="listbox" aria-label="Saved logins">
        {#each p.visible as entry (entry.id)}
          <button
            class="item"
            class:selected={selectedId === entry.id}
            role="option"
            aria-selected={selectedId === entry.id}
            onclick={() => (p.panel = { kind: "view", id: entry.id })}
          >
            <Favicon title={entry.title} urls={entry.urls} size={36} />
            <span class="text">
              <strong>{entry.title}</strong>
              <small>{entry.username || (entry.urls[0] ? host(entry.urls[0]) : "No user name")}</small>
            </span>
            <span class="marks">
              {#if entry.apps.length}<span title="Linked to a Windows program"><AppWindow size={13} /></span>{/if}
              {#if entry.hasTotp}<span title="Has 2FA codes"><ShieldCheck size={13} /></span>{/if}
              {#if entry.passkeys.length}<span title="Has a passkey"><UserKey size={13} /></span>{/if}
              {#if entry.favorite}<span class="fav" title="Favorite"><Star size={13} /></span>{/if}
              {#if entry.hasPassword}<StrengthMeter strength={entry.strength} compact />{/if}
            </span>
          </button>
        {:else}
          <div class="empty">
            {#if p.entries.length}
              <Search size={22} aria-hidden="true" />
              <strong>No matching logins</strong>
              <span>Try a different search or choose another filter.</span>
              <button class="btn small" onclick={() => ((p.query = ""), (p.filter = "all"))}>Clear search and filters</button>
            {:else}
              <KeyRound size={24} aria-hidden="true" />
              <strong>Your vault is empty</strong>
              {#if MOBILE}
                <span>Add a login here, or import your passwords in MYLE on your PC: they sync here.</span>
                <button class="btn small" onclick={() => (p.panel = { kind: "edit", id: null })}><Plus size={13} /> Add login</button>
              {:else}
                <span>Add a login, or import them from your browser or another password manager.</span>
                <button class="btn small" onclick={startImport}><Download size={13} /> Import passwords</button>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    </div>

    <div class="panel glass">
      {#if p.panel.kind !== "none"}
        <button class="back" onclick={() => (p.panel = { kind: "none" })}><ArrowLeft size={15} /> All logins</button>
      {/if}
      <div class="panel-scroll">
        {#if p.panel.kind === "view"}
          {#key p.panel.id}<EntryView id={p.panel.id} />{/key}
        {:else if p.panel.kind === "edit"}
          {#key p.panel.id}<EntryEditor id={p.panel.id} />{/key}
        {:else}
          <div class="overview">
            <div class="overview-main">
              <div class="overview-symbol" aria-hidden="true"><KeyRound size={34} strokeWidth={1.5} /></div>
              <div class="overview-copy">
                <h2>{p.entries.length ? "Your logins, all in one place" : "Start your password vault"}</h2>
                <p>{p.entries.length ? "Choose a login from the list to see its details, copy a password, or open its website." : "Add your first login, or bring existing passwords into your vault."}</p>
                <div class="overview-actions">
                  <button class="btn primary" onclick={() => (p.panel = { kind: "edit", id: null })}><Plus size={15} /> Add login</button>
                  {#if MOBILE}
                    <!-- Import and browser filling are MYLE's on the PC. -->
                  {:else if p.entries.length}
                    <button class="btn" onclick={() => (browserOpen = true)}><Globe size={15} /> Browser filling</button>
                  {:else}
                    <button class="btn" onclick={startImport}><Download size={15} /> Import passwords</button>
                  {/if}
                </div>
              </div>
            </div>
            {#if p.entries.length}
              <div class="overview-health">
                <div class="overview-health-heading">
                  <strong>Password health</strong>
                  <span>Review passwords that need attention</span>
                </div>
                <div class="overview-health-items">
                  <button disabled={p.counts.weak === 0} onclick={() => ((p.query = ""), (p.filter = "weak"))}>
                    <span class="health-count weak">{p.counts.weak}</span>
                    <span>Weak passwords</span>
                    <span class="health-action">{p.counts.weak ? "Review" : "None"}</span>
                  </button>
                  <button disabled={p.counts.reused === 0} onclick={() => ((p.query = ""), (p.filter = "reused"))}>
                    <span class="health-count reused">{p.counts.reused}</span>
                    <span>Reused passwords</span>
                    <span class="health-action">{p.counts.reused ? "Review" : "None"}</span>
                  </button>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

{#if dialog}
  <VaultDialog {dialog} onclose={() => (dialog = null)} />
{/if}
{#if browserOpen}
  <BrowserFilling onclose={() => (browserOpen = false)} />
{/if}

<style>
  /* Laid out by its own width, not the window's: the sidebar takes a share.
     It fills the page's height; the list and the entry scroll inside it. */
  .vault {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 14px;
    min-width: 0;
    min-height: 0;
    container: vault / inline-size;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
  }

  .search {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 14px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 10px;
    background: rgb(0 0 0 / 0.2);
    color: var(--text-3);
  }

  .search:focus-within {
    border-color: rgb(var(--accent-rgb) / 0.55);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--text-1);
    font: inherit;
    font-size: 13.5px;
  }

  .toolbar > :global(.btn) {
    height: 40px;
    flex: none;
  }

  .more {
    width: 40px;
    height: 40px;
  }

  .menu {
    padding: 5px;
  }

  .menu-label {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-top: 4px;
  }

  .lock-times {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    padding: 0 8px 8px;
  }

  .lock-times .chip {
    height: 24px;
    padding: 0 9px;
    font-size: 11.5px;
  }

  .filters {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .filters :global(.sync) {
    margin-left: auto;
  }

  .other-vault {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 11px 14px;
    border: 1px solid rgb(255 180 84 / 0.3);
    border-radius: 12px;
    background: rgb(255 180 84 / 0.08);
    color: #ffd08a;
    font-size: 12.5px;
  }

  .other-vault span {
    flex: 1;
    color: var(--text-2);
  }

  .other-vault strong {
    display: block;
    color: #ffd08a;
  }

  .windows-hotkey-error {
    margin: 0;
    color: var(--text-3);
    font-size: 11.5px;
  }

  /* Whatever height is left: a banner above makes it shorter, never the
     page longer. */
  .split {
    display: grid;
    flex: 1 1 auto;
    grid-template-columns: minmax(300px, 380px) minmax(0, 1fr);
    gap: 14px;
    min-height: 300px;
  }

  @container vault (min-width: 1100px) {
    .split {
      grid-template-columns: minmax(360px, 30%) minmax(0, 1fr);
    }
  }

  @container vault (min-width: 1700px) {
    .split {
      grid-template-columns: 500px minmax(0, 1fr);
    }
  }

  .list {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 65px;
    padding: 12px 20px;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
    color: var(--text-3);
  }

  .list-head h2 {
    margin: 0 0 2px;
    color: var(--text-1);
    font-size: 14px;
    font-weight: 650;
  }

  .list-head span {
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  .list-scroll,
  .panel-scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    scrollbar-color: rgb(210 220 245 / 0.2) transparent;
    scrollbar-width: thin;
  }

  .list-scroll {
    display: grid;
    align-content: start;
    gap: 2px;
    padding: 8px;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 11px;
    width: 100%;
    min-height: 59px;
    padding: 9px 12px;
    border: 1px solid transparent;
    border-radius: 10px;
    text-align: left;
    transition: background var(--dur-fast), border-color var(--dur-fast);
  }

  .item:hover {
    background: var(--hover);
  }

  .item.selected {
    border-color: rgb(var(--accent-rgb) / 0.33);
    background: rgb(var(--accent-rgb) / 0.13);
  }

  .item:focus-visible {
    outline: 2px solid rgb(var(--accent-rgb) / 0.75);
    outline-offset: -2px;
  }

  .text {
    display: grid;
    flex: 1;
    min-width: 0;
    gap: 2px;
  }

  .text strong,
  .text small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text strong {
    font-size: 13.5px;
    font-weight: 620;
    line-height: 1.2;
  }

  .text small {
    color: var(--text-2);
    font-size: 11.8px;
    line-height: 1.25;
  }

  .marks {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
  }

  .fav {
    color: #ffd166;
  }

  .empty {
    display: grid;
    justify-items: center;
    align-content: center;
    gap: 9px;
    min-height: 250px;
    padding: 35px 20px;
    color: var(--text-3);
    font-size: 12.5px;
    text-align: center;
  }

  .empty strong {
    color: var(--text-1);
    font-size: 14px;
  }

  .panel {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .panel-scroll {
    padding: clamp(20px, 2.2vw, 34px) clamp(18px, 2.6vw, 40px);
  }

  /* Only on a narrow page, where the list and the entry take turns. */
  .back {
    display: none;
    align-items: center;
    gap: 7px;
    align-self: flex-start;
    margin: 12px 0 0 14px;
    padding: 6px 10px;
    border-radius: 8px;
    color: var(--text-2);
    font-size: 12.5px;
  }

  .back:hover {
    background: var(--hover);
    color: var(--text-1);
  }

  .overview {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: clamp(26px, 5vh, 54px);
    width: min(100%, 730px);
    min-height: 100%;
    margin: 0 auto;
  }

  .overview-main {
    display: flex;
    align-items: center;
    gap: clamp(22px, 4vw, 48px);
  }

  .overview-symbol {
    display: grid;
    place-items: center;
    width: clamp(86px, 11vw, 132px);
    aspect-ratio: 1;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.27);
    border-radius: 32px;
    background:
      radial-gradient(circle at 28% 24%, rgb(255 255 255 / 0.13), transparent 50%),
      linear-gradient(145deg, rgb(var(--accent-rgb) / 0.21), rgb(111 179 198 / 0.07));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1), 0 20px 45px -28px rgb(var(--accent-rgb) / 0.45);
    color: #c5cafd;
  }

  .overview-copy {
    min-width: 0;
  }

  .overview-copy h2 {
    max-width: 20ch;
    margin: 0;
    font-family: var(--font-brand);
    font-size: clamp(24px, 2.4vw, 34px);
    font-weight: 600;
    line-height: 1.15;
    letter-spacing: -0.025em;
  }

  .overview-copy p {
    max-width: 47ch;
    margin: 12px 0 0;
    color: var(--text-2);
    font-size: 13px;
    line-height: 1.5;
  }

  .overview-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 22px;
  }

  .overview-actions .btn {
    height: 36px;
  }

  .overview-health {
    border-top: 1px solid rgb(255 255 255 / 0.09);
    padding-top: 22px;
  }

  .overview-health-heading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .overview-health-heading strong {
    font-size: 13px;
    font-weight: 620;
  }

  .overview-health-heading span {
    color: var(--text-3);
    font-size: 11.5px;
  }

  .overview-health-items {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 9px;
  }

  .overview-health-items button {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    min-height: 55px;
    padding: 10px 12px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 11px;
    background: rgb(255 255 255 / 0.035);
    font-size: 12px;
    text-align: left;
    transition: background var(--dur-fast), border-color var(--dur-fast);
  }

  .overview-health-items button:not(:disabled):hover {
    border-color: rgb(var(--accent-rgb) / 0.3);
    background: rgb(var(--accent-rgb) / 0.09);
  }

  .overview-health-items button:disabled {
    cursor: default;
    opacity: 0.68;
  }

  .overview-health-items button:focus-visible {
    outline: 2px solid rgb(var(--accent-rgb) / 0.75);
    outline-offset: 2px;
  }

  .health-count {
    display: grid;
    place-items: center;
    min-width: 32px;
    height: 32px;
    padding: 0 6px;
    border-radius: 9px;
    font-family: var(--font-brand);
    font-size: 15px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .health-count.weak {
    background: rgb(255 143 143 / 0.12);
    color: #ffb3b3;
  }

  .health-count.reused {
    background: rgb(255 196 102 / 0.12);
    color: #ffd18f;
  }

  .health-action {
    margin-left: auto;
    color: #b7bef5;
    font-size: 11px;
  }

  @container vault (max-width: 760px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
    }

    .split.has-selection .list,
    .split:not(.has-selection) .panel {
      display: none;
    }

    .back {
      display: inline-flex;
    }
  }

  @container vault (max-width: 640px) {
    .toolbar .label {
      display: none;
    }

    .filters {
      flex-wrap: wrap;
    }

    .filters :global(.sync) {
      margin-left: 0;
    }

    .panel-scroll {
      padding: 24px 20px;
    }

    .overview-main {
      align-items: flex-start;
      flex-direction: column;
    }

    .overview-symbol {
      width: 72px;
      border-radius: 22px;
    }

    .overview-health-items {
      grid-template-columns: 1fr;
    }
  }

  /* A phone: rows and bars a finger can hit, whatever the screen's height. */
  :global(html.mobile) .search,
  :global(html.mobile) .toolbar > :global(.btn) {
    height: 44px;
  }

  :global(html.mobile) .more {
    width: 44px;
    height: 44px;
  }

  :global(html.mobile) .search input {
    font-size: 15px;
  }

  :global(html.mobile) .item {
    min-height: 62px;
    padding: 9px 12px;
  }

  :global(html.mobile) .text strong {
    font-size: 15px;
  }

  :global(html.mobile) .text small {
    font-size: 13px;
  }

  :global(html.mobile) .list-head {
    display: none;
  }

  /* An open entry has the whole screen; "All logins" goes back. */
  :global(html.mobile) .vault:has(.split.has-selection) > .toolbar,
  :global(html.mobile) .vault:has(.split.has-selection) > .filters {
    display: none;
  }

  /* Last, so it wins over the rules above: a big screen gets roomier rows. */
  @container vault (min-width: 1700px) {
    .item {
      min-height: 64px;
    }

    .text strong {
      font-size: 14px;
    }

    .text small {
      font-size: 12.3px;
    }
  }

  /* A 1080p screen (and anything short): tighter bars and rows, so more
     logins show and nothing but the list scrolls. */
  @media (max-height: 1000px) {
    .vault,
    .split {
      gap: 10px;
    }

    .search,
    .toolbar > :global(.btn) {
      height: 36px;
    }

    .more {
      width: 36px;
      height: 36px;
    }

    .list-head {
      min-height: 50px;
      padding: 8px 16px;
    }

    .list-scroll {
      padding: 6px;
    }

    .item {
      min-height: 50px;
      padding: 6px 10px;
    }

    .panel-scroll {
      padding: 18px clamp(16px, 2.2vw, 32px);
    }
  }

  @media (max-height: 820px) {
    .search,
    .toolbar > :global(.btn) {
      height: 34px;
    }

    .more {
      width: 34px;
      height: 34px;
    }

    .list-head {
      min-height: 44px;
      padding: 6px 14px;
    }

    .item {
      min-height: 46px;
      padding: 5px 10px;
    }

    .panel-scroll {
      padding: 14px 20px;
    }
  }
</style>
