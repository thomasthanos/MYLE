<script lang="ts">
  import AppWindow from "@lucide/svelte/icons/app-window";
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import History from "@lucide/svelte/icons/history";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import UserKey from "@lucide/svelte/icons/user-key";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ANDROID } from "../../../lib/platform";
  import { onDestroy } from "svelte";
  import { passwordsApi as api, type OldPassword } from "./api";
  import Favicon from "./Favicon.svelte";
  import { iconHost, passwords as p } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";
  import TotpCode from "./TotpCode.svelte";

  let { id }: { id: string } = $props();

  const entry = $derived(p.entry(id));
  const shown = $derived(p.revealed[id]);
  let history = $state<OldPassword[] | null>(null);
  let hideHistory: ReturnType<typeof setTimeout> | undefined;
  const site = $derived(entry?.urls.map(iconHost).find(Boolean) ?? null);

  onDestroy(() => clearTimeout(hideHistory));

  function when(seconds: number) {
    return new Date(seconds * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  }

  /** "today", "3 days ago", "2 months ago"…, then the date itself after a year. */
  function ago(seconds: number) {
    const days = Math.floor((Date.now() / 1000 - seconds) / 86400);
    if (days < 1) return "today";
    if (days < 2) return "yesterday";
    if (days < 30) return `${days} days ago`;
    if (days < 365) {
      const months = Math.max(1, Math.round(days / 30));
      return months === 1 ? "a month ago" : `${months} months ago`;
    }
    return `on ${when(seconds)}`;
  }

  const copied = (field: string) => p.copied === `${id}:${field}`;

  async function toggleHistory() {
    clearTimeout(hideHistory);
    history = history ? null : await api.history(id).catch(() => []);
    // Old passwords are not left on screen either.
    if (history) hideHistory = setTimeout(() => (history = null), 30_000);
  }

  function open(url: string) {
    const target = /^https?:\/\//i.test(url) ? url : `https://${url}`;
    void openUrl(target).catch(() => {});
  }
</script>

{#if entry}
  <article class="view">
    <header>
      <Favicon title={entry.title} urls={entry.urls} size={52} />
      <div class="titles">
        <h2 title={entry.title}>{entry.title}</h2>
        <div class="subtitle">
          {#if site}
            <button class="site" title="Open {entry.urls[0]}" onclick={() => open(entry.urls[0])}>{site}</button>
          {:else if entry.apps.length}
            <span>{entry.apps.map((app) => app.name).join(", ")}</span>
          {/if}
          {#if entry.folder}<span class="folder">{entry.folder}</span>{/if}
        </div>
      </div>
      <button
        class="icon-btn star"
        class:on={entry.favorite}
        title={entry.favorite ? "Remove from favorites" : "Add to favorites"}
        aria-label="Favorite"
        onclick={() => p.toggleFavorite(id)}><Star size={16} /></button
      >
    </header>

    {#if entry.strength === "weak" || entry.reused}
      <p class="warning">
        <TriangleAlert size={13} />
        {entry.reused ? "This password is used for other entries too." : "This password is easy to guess."}
        Change it on the site, then here.
      </p>
    {/if}

    <dl class="fields">
      {#if entry.username}
        <div class="row">
          <dt>User name</dt>
          <dd class="selectable">{entry.username}</dd>
          <button class="icon-btn" class:done={copied("username")} title="Copy" aria-label="Copy user name" onclick={() => p.copy(id, "username")}>
            {#if copied("username")}<Check size={14} />{:else}<Copy size={14} />{/if}
          </button>
        </div>
      {/if}
      {#if entry.hasPassword}
        <div class="row">
          <dt>Password</dt>
          <dd class="secret" class:selectable={shown !== undefined}>{shown ?? "••••••••••••"}</dd>
          <button class="icon-btn" title={shown ? "Hide" : "Show"} aria-label={shown ? "Hide password" : "Show password"} onclick={() => p.toggleReveal(id)}>
            {#if shown !== undefined}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
          </button>
          <button class="icon-btn" class:done={copied("password")} title="Copy (the clipboard clears in 30 seconds)" aria-label="Copy password" onclick={() => p.copy(id, "password")}>
            {#if copied("password")}<Check size={14} />{:else}<Copy size={14} />{/if}
          </button>
        </div>
        <div class="row meta">
          <dt></dt>
          <dd><StrengthMeter strength={entry.strength} /></dd>
        </div>
      {/if}
      {#if entry.hasTotp}
        <TotpCode {id} />
      {/if}
      {#each entry.urls as url (url)}
        <div class="row">
          <dt>Website</dt>
          <dd class="selectable link">{url}</dd>
          <button class="icon-btn" title="Open" aria-label="Open website" onclick={() => open(url)}><ExternalLink size={14} /></button>
        </div>
      {/each}
      {#if entry.apps.length}
        <div class="row">
          <dt>Programs</dt>
          <dd class="apps">
            {#each entry.apps as app (app.exe)}<span class="app"><AppWindow size={12} /> {app.name}</span>{/each}
          </dd>
        </div>
      {/if}
      {#if entry.notes}
        <div class="row">
          <dt>Notes</dt>
          <dd class="notes selectable">{entry.notes}</dd>
        </div>
      {/if}
    </dl>

    {#if !ANDROID && entry.passkeys.length}
      <section class="passkeys" aria-label="Passkeys">
        <h3><UserKey size={13} /> Passkeys</h3>
        {#each entry.passkeys as key (key.credentialId)}
          <div class="passkey">
            <span class="passkey-text">
              <strong>{key.userName || key.userDisplayName || "Passkey"}</strong>
              <small>{key.rpId} · saved {when(key.createdAt)}</small>
            </span>
            <button class="icon-btn" title="Delete passkey" aria-label="Delete the passkey for {key.rpId}" onclick={() => p.removePasskey(id, key.credentialId)}><Trash2 size={14} /></button>
          </div>
        {/each}
      </section>
    {/if}

    {#if entry.historyCount}
      <button class="history-toggle" onclick={toggleHistory}>
        <History size={13} /> {history ? "Hide" : "Show"} {entry.historyCount} earlier password{entry.historyCount === 1 ? "" : "s"}
      </button>
      {#if history}
        <ul class="history" data-sensitive>
          {#each history as old, i (i)}
            <li><code class="selectable">{old.password}</code><span>until {when(old.changedAt)}</span></li>
          {/each}
        </ul>
      {/if}
    {/if}

    <footer>
      <span class="updated" title={when(entry.updatedAt)}>Changed {ago(entry.updatedAt)}</span>
      <button class="btn ghost small danger-text" onclick={() => p.remove(id)}><Trash2 size={13} /> Delete</button>
      <button class="btn small" onclick={() => (p.panel = { kind: "edit", id })}><Pencil size={13} /> Edit</button>
    </footer>
  </article>
{/if}

<style>
  .icon-btn.done { color: var(--ok); }

  /* Scale with the detail panel, including when the sidebar is expanded. */
  .view { --entry-icon: 56px; --entry-title: 24px; --entry-value: 15px; --entry-label: 13px; --entry-row: 60px; --entry-gap: 20px; --entry-padding: 12px; --entry-label-width: 116px; --entry-action: 36px; display: grid; gap: var(--entry-gap); width: 100%; min-width: 0; container: entry / inline-size; }

  header { display: flex; align-items: center; gap: 14px; }
  header :global(.favicon) { --size: var(--entry-icon) !important; }

  .icon-btn { width: var(--entry-action); height: var(--entry-action); }
  .icon-btn :global(svg) { width: 18px; height: 18px; }

  .titles { display: grid; flex: 1; gap: 4px; min-width: 0; }

  h2 { overflow: hidden; font-size: var(--entry-title); line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }

  .subtitle { display: flex; align-items: center; gap: 8px; min-width: 0; color: var(--text-3); font-size: var(--entry-label); }

  .subtitle > * { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .site { color: #b9c2ff; }

  .site:hover { text-decoration: underline; }

  .folder { flex: none; padding: 1px 8px; border-radius: 999px; background: rgb(255 255 255 / 0.06); color: var(--text-2); font-size: 11.5px; }

  .star.on { color: #ffd166; }

  .warning { display: flex; align-items: center; gap: 7px; padding: 8px 11px; border: 1px solid rgb(255 180 84 / 0.25); border-radius: 10px; background: rgb(255 180 84 / 0.08); color: #ffd08a; font-size: var(--entry-label); }

  .fields { display: grid; gap: 4px; margin: 0; padding: var(--entry-padding); border: 1px solid rgb(255 255 255 / 0.06); border-radius: 18px; background: rgb(0 0 0 / 0.14); }

  .row { display: grid; grid-template-columns: var(--entry-label-width) minmax(0, 1fr) auto auto; align-items: center; gap: 10px; min-height: var(--entry-row); padding: 4px 4px 4px 12px; border-radius: 10px; }

  /* A narrow panel: each label above its value. */
  @container entry (max-width: 420px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto auto;
      row-gap: 0;
      padding-top: 6px;
      padding-bottom: 6px;
    }

    .row dt {
      grid-column: 1 / -1;
    }

    .row.meta dt {
      display: none;
    }
  }

  .row:not(.meta):hover { background: rgb(255 255 255 / 0.035); }

  .row.meta { min-height: 24px; margin-top: -10px; padding-block: 0 8px; }
  .row.meta :global(.meter) { font-size: var(--entry-label); gap: 9px; }
  .row.meta :global(.meter i) { width: 18px; height: 5px; }

  dt { color: var(--text-3); font-size: var(--entry-label); }

  dd { margin: 0; overflow: hidden; font-size: var(--entry-value); text-overflow: ellipsis; white-space: nowrap; }

  .secret { font-family: var(--font-mono); letter-spacing: 0.02em; }

  .link { color: #b9c2ff; }

  .notes { padding: 8px 0; white-space: pre-wrap; }

  .apps { display: flex; flex-wrap: wrap; gap: 6px; white-space: normal; }

  .app { display: inline-flex; align-items: center; gap: 5px; padding: 2px 8px; border-radius: 999px; background: rgb(var(--accent-rgb) / 0.12); font-size: 11.5px; }

  .passkeys { display: grid; gap: 4px; }

  .passkeys h3 { display: flex; align-items: center; gap: 6px; margin: 0 0 2px; color: var(--text-2); font-size: var(--entry-label); font-weight: 600; }

  .passkey { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 4px 4px 12px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 10px; background: rgb(0 0 0 / 0.14); }

  .passkey-text { display: grid; flex: 1; min-width: 0; }

  .passkey-text strong,
  .passkey-text small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .passkey-text strong { font-size: var(--entry-value); font-weight: 600; }

  .passkey-text small { color: var(--text-3); font-size: var(--entry-label); }

  .history-toggle { display: inline-flex; align-items: center; gap: 6px; justify-self: start; color: var(--text-3); font-size: var(--entry-label); }

  .history-toggle:hover { color: var(--text-1); }

  .history { display: grid; gap: 4px; margin: -6px 0 0; padding: 0; list-style: none; }

  .history li { display: flex; justify-content: space-between; gap: 12px; padding: 6px 12px; border-radius: 8px; background: rgb(0 0 0 / 0.18); font-size: 12px; }

  .history span { color: var(--text-3); }

  footer { display: flex; align-items: center; gap: 12px; padding-top: 16px; border-top: 1px solid rgb(255 255 255 / 0.06); }
  footer .btn { min-height: var(--entry-action); padding-inline: 14px; font-size: var(--entry-label); }
  footer :global(svg) { width: 16px; height: 16px; }

  .updated { flex: 1; min-width: 0; color: var(--text-3); font-size: var(--entry-label); }

  .danger-text { color: #ff9d9d; }

  @container entry-panel (min-width: 1100px) {
    .view {
      --entry-icon: 68px;
      --entry-title: 28px;
      --entry-value: 18px;
      --entry-label: 15px;
      --entry-row: 76px;
      --entry-gap: 26px;
      --entry-padding: 18px;
      --entry-label-width: 144px;
      --entry-action: 42px;
    }
  }

  @container entry-panel (min-width: 1800px) {
    .view {
      --entry-icon: 80px;
      --entry-title: 34px;
      --entry-value: 21px;
      --entry-label: 17px;
      --entry-row: 96px;
      --entry-gap: 32px;
      --entry-padding: 24px;
      --entry-label-width: 176px;
      --entry-action: 48px;
    }
    .icon-btn :global(svg) { width: 22px; height: 22px; }
    header { gap: 20px; }
  }

  /* Keep phone panels and short desktop windows comfortable to scroll. */
  @container entry-panel (max-width: 480px) {
    .view {
      --entry-icon: 48px;
      --entry-title: 22px;
      --entry-label-width: 96px;
      --entry-padding: 8px;
      --entry-gap: 18px;
    }
    footer { gap: 6px; }
    footer .btn { padding-inline: 10px; }
  }

  @media (max-height: 700px) {
    .view { --entry-row: 48px; --entry-gap: 16px; }
  }
</style>
