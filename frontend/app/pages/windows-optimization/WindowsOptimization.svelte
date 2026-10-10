<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Gauge from "@lucide/svelte/icons/gauge";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import LogIn from "@lucide/svelte/icons/log-in";
  import Power from "@lucide/svelte/icons/power";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import UserRound from "@lucide/svelte/icons/user-round";
  import Package from "@lucide/svelte/icons/package";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import Wrench from "@lucide/svelte/icons/wrench";
  import BiosRestartDialog from "./BiosRestartDialog.svelte";
  import AppsTab from "./debloat/AppsTab.svelte";
  import Progress from "./debloat/Progress.svelte";
  import QuickSetupTab from "./debloat/QuickSetupTab.svelte";
  import ReviewBar from "./debloat/ReviewBar.svelte";
  import ReviewPanel from "./debloat/ReviewPanel.svelte";
  import SettingsTab from "./debloat/SettingsTab.svelte";
  import StartMenuTab from "./debloat/StartMenuTab.svelte";
  import { debloat, type Tab } from "./debloat/state.svelte";
  import { windowsOptimizationState as tools } from "./state.svelte";
  onMount(() => {
    void tools.load();
    void debloat.load();
  });
  const tabs: { id: Tab; label: string; icon: typeof Wrench }[] = [
    { id: "quick", label: "Quick setup", icon: WandSparkles },
    { id: "settings", label: "Settings", icon: SlidersHorizontal },
    { id: "apps", label: "Apps", icon: Package },
    { id: "startMenu", label: "Start Menu", icon: LayoutGrid },
    { id: "tools", label: "Tools", icon: Wrench },
  ];
  /** What each tab is for, under the page's title. */
  const tabHints: Record<Tab, string> = {
    quick: "A faster, quieter, more private Windows in three steps. Everything can be undone.",
    settings: "Every setting MYLE can change, by topic, with what each one does.",
    apps: "Preinstalled apps you may not need. Pick them, then review and remove them together.",
    startMenu: "How the Start menu looks: alignment, layout, folders and pins.",
    tools: "Sign in to Windows by itself, or restart straight into the BIOS.",
  };
  /** Choices waiting on a tab, shown next to its name. */
  const waiting = $derived<Partial<Record<Tab, number>>>({
    settings: debloat.pending.on.length + debloat.pending.off.length,
    apps: debloat.pendingApps.length,
  });
  /** The tabs whose choices the bar below applies. */
  const choosing = $derived(debloat.tab === "quick" || debloat.tab === "settings" || debloat.tab === "apps");
  const state = $derived(tools.snapshot);
  const autoLogon = $derived(state?.autoLogon);
  const autoLogonActive = $derived(tools.autoLogonBusy);
  const autoLogonAction = $derived(
    tools.autoLogonRequest ?? autoLogon?.activeOperation ?? null,
  );
  const firmwareRestart = $derived(state?.firmwareRestart);
  const firmwareStatus = $derived(tools.firmwareRestartStatus());
  function statusTone(status: string) {
    if (status === "Completed" || status === "Enabled") return "ok";
    if (["Cancelled", "Waiting for UAC", "Conflict", "Unavailable"].includes(status)) return "warn";
    if (status === "Error") return "error";
    if (["Preparing", "Running", "Launching", "Downloading", "Verifying", "Extracting", "Resolving release", "Working", "Processing", "Enabling", "Disabling", "Checking"].includes(status)) return "active";
    return "idle";
  }
  function accountTypeLabel(value: string | undefined) {
    if (value === "microsoft") return "Microsoft account";
    if (value === "domain") return "Domain account";
    if (value === "entra") return "Microsoft Entra";
    if (value === "local") return "Local account";
    return "Windows account";
  }
</script>
<div class="page">
  <div class="top">
    <div class="title">
      <span class="title-icon"><Gauge size={17} /></span>
      <div class="title-text">
        <h1>Windows Optimization</h1>
        <p>{tabHints[debloat.tab]}</p>
      </div>
    </div>
    <div class="tabs" role="tablist" aria-label="Windows Optimization">
      {#each tabs as tab (tab.id)}
        <button
          role="tab"
          class="tab"
          class:active={debloat.tab === tab.id}
          aria-selected={debloat.tab === tab.id}
          onclick={() => (debloat.tab = tab.id)}
        >
          <tab.icon size={14} /> {tab.label}
          {#if waiting[tab.id]}<span class="waiting" aria-label="{waiting[tab.id]} chosen">{waiting[tab.id]}</span>{/if}
        </button>
      {/each}
    </div>
  </div>
  {#if tools.error}
    <div class="error-banner surface" role="alert"><CircleAlert size={16} /><span>{tools.error}</span></div>
  {/if}
  {#if debloat.error}
    <div class="error-banner surface" role="alert"><CircleAlert size={16} /><span>{debloat.error}</span></div>
  {/if}
  {#if tools.externallyLocked && !tools.ownBusy && !debloat.busy && !debloat.startMenuBusy}
    <div class="lock-banner surface"><LoaderCircle size={14} class="spin" /><span>Another app task is running. Optimization tools are temporarily locked.</span></div>
  {/if}
  <Progress />
  <div class="panel">
  {#if debloat.tab === "quick"}
    <QuickSetupTab />
  {:else if debloat.tab === "settings"}
    <SettingsTab />
  {:else if debloat.tab === "startMenu"}
    <StartMenuTab />
  {:else if debloat.tab === "apps"}
    <AppsTab />
  {:else}
  <div class="cards">
    <article class="tool-card surface autologon" class:active={autoLogonActive}>
      <span class="rim"></span>
      <div class="card-head">
        <span class="tool-icon"><img src="/icons/Windows-AutoLogon.svg" alt="" width="42" height="42" /></span>
        <div class="identity">
          <div class="name-row">
            <h2>Windows Auto-Logon</h2>
            <span class="badge admin"><ShieldCheck size={11} /> Admin required</span>
          </div>
          <p>Sign in to the current Windows account automatically after startup.</p>
        </div>
        <span class="status {statusTone(tools.autoLogonStatus())}"><i></i>{tools.autoLogonStatus()}</span>
      </div>
      <div class="account surface" class:account-conflict={autoLogon?.status === "conflict"}>
        <span class="account-icon"><UserRound size={18} /></span>
        <div class="account-copy">
          <span>Current Windows user</span>
          <strong>{autoLogon?.displayName || "Detecting current user…"}</strong>
          <code>{autoLogon?.accountName || "Waiting for Windows account details"}</code>
        </div>
        <span class="badge account-type">{accountTypeLabel(autoLogon?.accountType)}</span>
      </div>
      <div class="auto-details">
        <div>
          <ShieldCheck size={13} />
          <div>
            <strong>SID Verified</strong>
            <span>No extra account created</span>
          </div>
        </div>
        <div>
          <KeyRound size={13} />
          <div>
            <strong>Native Control</strong>
            <span>Built directly into app</span>
          </div>
        </div>
      </div>
      {#if autoLogon?.status === "conflict"}
        <div class="info-box auto-message conflict" role="alert">
          <CircleAlert size={15} />
          <div>
            <strong>Account conflict detected</strong>
            <p>Configured for <strong>{autoLogon.configuredUser ?? "another account"}</strong>. You can safely disable it below.</p>
          </div>
        </div>
      {:else if autoLogon?.blockedReason}
        <div class="info-box auto-message unavailable" role="note">
          <CircleAlert size={15} />
          <div>
            <strong>Auto-Logon unavailable</strong>
            <p>{autoLogon.blockedReason}</p>
          </div>
        </div>
      {:else}
        <div class="info-box auto-message secure">
          <KeyRound size={15} />
          <div>
            <strong>Zero-knowledge credential prompt</strong>
            <p>Windows asks for your password directly; credentials never pass through IPC.</p>
          </div>
        </div>
      {/if}
      <div class="actions auto-actions">
        <button
          class="btn primary launch"
          disabled={tools.locked || autoLogon?.status !== "disabled"}
          onclick={() => tools.setAutoLogon(true)}
        >
          {#if autoLogonAction === "enable"}<LoaderCircle size={14} class="spin" /> Enabling…{:else}<LogIn size={14} /> Enable Auto-Logon{/if}
        </button>
        <button
          class="btn disable"
          disabled={tools.locked || !autoLogon?.accountName || autoLogon?.status === "disabled"}
          onclick={() => tools.setAutoLogon(false)}
        >
          {#if autoLogonAction === "disable"}<LoaderCircle size={14} class="spin" /> Disabling…{:else}<Power size={14} /> Disable{/if}
        </button>
      </div>
    </article>
  </div>
  <section
    class="firmware-action surface"
    class:active={tools.firmwareRestartBusy}
    aria-labelledby="firmware-action-title"
  >
    <span class="firmware-rim" aria-hidden="true"></span>
    <span class="tool-icon firmware-icon"><img src="/icons/Restart-to-BIOS.svg" alt="" width="42" height="42" /></span>
    <div class="firmware-copy">
      <div class="firmware-title">
        <h2 id="firmware-action-title">Restart to BIOS / UEFI</h2>
        <span class="badge firmware-badge">BIOS / UEFI</span>
        <span class="badge admin"><ShieldCheck size={11} /> Admin required</span>
      </div>
      <p>
        {#if firmwareRestart?.available}
          Restart directly into this PC's UEFI firmware settings—no startup key required.
        {:else if firmwareRestart?.blockedReason}
          {firmwareRestart.blockedReason}
        {:else}
          Checking whether this Windows installation supports direct UEFI startup…
        {/if}
      </p>
    </div>
    <span class="firmware-status status {statusTone(firmwareStatus)}"><i></i>{firmwareStatus}</span>
    <button
      class="btn firmware-button"
      disabled={tools.locked || !firmwareRestart?.available}
      onclick={() => tools.openBiosDialog()}
    >
      {#if tools.firmwareRestartBusy}
        <LoaderCircle size={14} class="spin" /> Processing…
      {:else}
        <RotateCw size={14} /> Restart to BIOS
      {/if}
    </button>
  </section>
  {/if}
  </div>
  {#if choosing}<ReviewBar />{/if}
</div>
{#if tools.biosDialogOpen}<BiosRestartDialog />{/if}
{#if debloat.reviewing}<ReviewPanel />{/if}
<style>
  .page { min-width: 0; }
  .tabs { display: flex; flex-wrap: wrap; gap: 4px; padding: 3px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 11px; background: rgb(0 0 0 / 0.18); }
  .tab { display: inline-flex; align-items: center; gap: 7px; height: 30px; padding: 0 12px; border-radius: 8px; color: var(--text-2); font-size: 12.5px; font-weight: 560; transition: background var(--dur-fast), color var(--dur-fast); }
  .waiting { display: grid; place-items: center; min-width: 18px; height: 18px; padding: 0 5px; border-radius: 999px; background: var(--accent-grad); color: #fff; font-size: 10px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .tab:hover { color: var(--text-1); background: var(--hover); }
  .tab.active { color: var(--text-1); background: var(--selected); }
  .tab:focus-visible { outline: 2px solid rgb(var(--accent-rgb) / 0.75); outline-offset: 1px; }
  .panel { container: optimization-page / inline-size; margin-top: 12px; }
  .top { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px 20px; }
  .title { display: flex; align-items: center; gap: 11px; min-width: 0; }
  .title-text { display: grid; gap: 2px; min-width: 0; }
  .title-text p { color: var(--text-2); font-size: 12px; }
  .title-icon { display: grid; place-items: center; flex: none; width: 36px; height: 36px; border: 1px solid rgb(var(--accent-rgb) / 0.22); border-radius: 11px; background: rgb(var(--accent-rgb) / 0.1); color: rgb(var(--accent-soft-rgb)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08); }
  h1 { font-size: 21px; }
  .warning, .error-banner, .lock-banner { display: flex; align-items: center; gap: 9px; margin-top: 10px; padding: 10px 12px; color: rgb(229 218 176 / 0.78); font-size: 11.5px; }
  .warning :global(svg) { color: rgb(241 187 84 / 0.9); flex: none; }
  .error-banner { color: rgb(255 170 170 / 0.9); }
  .lock-banner { color: var(--text-2); }
  /* 3-Column Equal Height Grid */
  .cards { display: grid; grid-template-columns: minmax(0, 560px); align-items: stretch; gap: 14px; }
  .tool-card { position: relative; display: flex; flex-direction: column; overflow: hidden; min-width: 0; padding: 14px; box-shadow: var(--elev-1); transition: transform var(--dur-med) var(--ease-out), border-color var(--dur-fast), box-shadow var(--dur-med); }
  .tool-card:hover:not(.active) { transform: translateY(-2px); border-color: rgb(255 255 255 / 0.095); box-shadow: var(--elev-2); }
  .tool-card.active { border-color: rgb(125 151 255 / 0.22); }
  .rim { position: absolute; inset: 0 14% auto; height: 1px; pointer-events: none; }
  .autologon .rim { background: linear-gradient(90deg, transparent, rgb(71 217 166 / 0.62), transparent); box-shadow: 0 0 15px rgb(48 199 148 / 0.22); }
  .card-head { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 11px; position: relative; padding-right: 82px; }
  .tool-icon { display: grid; place-items: center; width: 42px; height: 42px; flex: none; border-radius: 10px; overflow: hidden; box-shadow: 0 8px 18px -11px rgb(0 0 0 / 0.8); }
  .tool-icon img { width: 100%; height: 100%; display: block; object-fit: cover; }
  .identity { min-width: 0; }
  .identity h2 { font-size: 16px; }
  .identity > p { margin-top: 3px; color: var(--text-2); font-size: 11.5px; line-height: 1.45; }
  .name-row { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .badge { display: inline-flex; align-items: center; gap: 4px; padding: 2px 6px; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 999px; color: var(--text-3); background: rgb(255 255 255 / 0.035); font-size: 9px; font-weight: 600; letter-spacing: 0.02em; }
  .badge.admin { border-color: rgb(94 176 255 / 0.14); color: rgb(142 199 255 / 0.72); }
  .badge.portable { border-color: rgb(149 119 255 / 0.18); color: rgb(190 171 255 / 0.76); }
  .badge.beta { border-color: rgb(240 177 73 / 0.16); color: rgb(239 191 111 / 0.72); }
  .status { position: absolute; top: 1px; right: 0; display: inline-flex; align-items: center; gap: 5px; color: var(--text-3); font-size: 10px; }
  .status i { width: 6px; height: 6px; border-radius: 50%; background: var(--idle); }
  .status.ok i { background: var(--ok); box-shadow: 0 0 7px var(--ok-glow); }
  .status.active i { background: var(--update); box-shadow: 0 0 7px var(--update-glow); }
  .status.warn i { background: #e8ad55; }
  .status.error i { background: var(--danger); }
  /* Info strip */
  .info-box { display: flex; align-items: center; gap: 10px; margin-top: 8px; padding: 8px 10px; border-radius: 10px; }
  .info-box strong { display: block; font-size: 10.8px; font-weight: 600; color: rgb(232 238 250 / 0.88); }
  .info-box p { margin-top: 2px; font-size: 9.6px; line-height: 1.4; color: var(--text-3); }
  /* Auto-Logon Specifics */
  .account { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 10px; margin-top: 10px; padding: 8px 10px; }
  .account.account-conflict { border-color: rgb(237 170 73 / 0.16); }
  .account-icon { display: grid; place-items: center; width: 34px; height: 34px; border: 1px solid rgb(81 215 166 / 0.12); border-radius: 10px; background: rgb(66 204 153 / 0.07); color: rgb(99 224 177 / 0.86); }
  .account-copy { display: grid; min-width: 0; }
  .account-copy > span { color: var(--text-3); font-size: 8.5px; text-transform: uppercase; letter-spacing: 0.06em; }
  .account-copy strong { overflow: hidden; margin-top: 1px; color: rgb(235 241 248 / 0.9); font-size: 11.5px; text-overflow: ellipsis; white-space: nowrap; }
  .account-copy code { overflow: hidden; margin-top: 1px; color: var(--text-3); font-family: var(--font-mono); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .badge.account-type { justify-self: end; border-color: rgb(77 209 159 / 0.12); color: rgb(117 218 181 / 0.7); white-space: nowrap; }
  .auto-details { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 8px; }
  .auto-details > div { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 8px 10px; border: 1px solid rgb(255 255 255 / 0.045); border-radius: 9px; background: linear-gradient(145deg, rgb(63 203 151 / 0.035), rgb(255 255 255 / 0.012)); color: rgb(93 211 167 / 0.72); }
  .auto-details :global(svg) { flex: none; }
  .auto-details > div > div { display: grid; min-width: 0; }
  .auto-details strong { color: rgb(228 235 244 / 0.86); font-size: 10.5px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .auto-details span { color: var(--text-3); font-size: 9px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .auto-message { border: 1px solid rgb(255 255 255 / 0.05); background: rgb(255 255 255 / 0.018); }
  .auto-message :global(svg) { flex: none; }
  .auto-message.secure { border-color: rgb(63 203 151 / 0.11); background: rgb(63 203 151 / 0.03); }
  .auto-message.secure :global(svg) { color: rgb(81 216 165 / 0.8); }
  .auto-message.conflict, .auto-message.unavailable { border-color: rgb(237 170 73 / 0.12); background: rgb(237 170 73 / 0.035); }
  .auto-message.conflict :global(svg), .auto-message.unavailable :global(svg) { color: rgb(236 174 78 / 0.85); }
  /* Card Actions Pinned to Bottom */
  .actions { display: flex; align-items: center; gap: 8px; margin-top: auto; padding-top: 12px; }
  .launch { flex: 1; min-width: 126px; justify-content: center; }
  .github { color: var(--text-2); }
  .disable { color: rgb(225 230 240 / 0.72); }
  /* Restart to BIOS / UEFI Card */
  .firmware-action { position: relative; display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto; align-items: center; gap: 12px; margin-top: 10px; padding: 10px 14px; overflow: hidden; box-shadow: var(--elev-1); transition: border-color var(--dur-fast), box-shadow var(--dur-med); }
  .firmware-action.active { border-color: rgb(112 157 242 / 0.22); }
  .firmware-rim { position: absolute; inset: 0 22% auto; height: 1px; background: linear-gradient(90deg, transparent, rgb(102 163 255 / 0.58), transparent); box-shadow: 0 0 14px rgb(86 143 242 / 0.2); pointer-events: none; }
  .firmware-copy { min-width: 0; }
  .firmware-title { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .firmware-title h2 { font-size: 14.5px; }
  .badge.firmware-badge { border-color: rgb(106 165 255 / 0.15); color: rgb(147 188 255 / 0.72); }
  .firmware-copy p { overflow: hidden; margin-top: 3px; color: var(--text-3); font-size: 11px; line-height: 1.45; text-overflow: ellipsis; white-space: nowrap; }
  .firmware-status.status { position: static; white-space: nowrap; }
  .firmware-button { min-width: 146px; white-space: nowrap; }
  :global(:root.solid) .tool-card, :global(:root.solid) .tool-card:hover:not(.active) { transform: none; box-shadow: none; }
  :global(:root.solid) .rim { box-shadow: none; }
  :global(:root.solid) .firmware-action { box-shadow: none; }
  :global(:root.solid) .firmware-rim { box-shadow: none; }
  @media (max-width: 860px) { .cards { grid-template-columns: 1fr; } }
  @media (max-width: 720px) {
    .card-head { padding-right: 0; }
    .status { position: static; grid-column: 2; justify-self: start; }
    .firmware-action { grid-template-columns: auto minmax(0, 1fr) auto; }
    .firmware-status.status { grid-column: 2; justify-self: start; }
    .firmware-button { grid-column: 3; grid-row: 1 / span 2; }
  }
  @media (max-width: 520px) {
    .auto-details { grid-template-columns: 1fr; }
    .account { grid-template-columns: auto minmax(0, 1fr); }
    .account .account-type { grid-column: 2; justify-self: start; }
    .actions { align-items: stretch; flex-direction: column; }
    .auto-actions { display: flex; }
    .actions .btn { width: 100%; }
    .firmware-action { grid-template-columns: auto minmax(0, 1fr); }
    .firmware-status.status { grid-column: 2; }
    .firmware-button { grid-column: 1 / -1; grid-row: auto; width: 100%; }
    .firmware-copy p { white-space: normal; }
  }
  @media (prefers-reduced-motion: reduce) {
    .tool-card { transition: none; }
    .tool-card:hover:not(.active) { transform: none; }
  }
</style>
