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

    {#if entry.passkeys.length}
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

  /* A readable width however wide the window: the buttons stay by their values. */
  .view { display: grid; gap: 18px; width: min(100%, 760px); margin: 0 auto; container: entry / inline-size; }

  header { display: flex; align-items: center; gap: 14px; }

  .titles { display: grid; flex: 1; gap: 4px; min-width: 0; }

  h2 { overflow: hidden; font-size: 20px; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }

  .subtitle { display: flex; align-items: center; gap: 8px; min-width: 0; color: var(--text-3); font-size: 12.5px; }

  .subtitle > * { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .site { color: #b9c2ff; }

  .site:hover { text-decoration: underline; }

  .folder { flex: none; padding: 1px 8px; border-radius: 999px; background: rgb(255 255 255 / 0.06); color: var(--text-2); font-size: 11.5px; }

  .star.on { color: #ffd166; }

  .warning { display: flex; align-items: center; gap: 7px; padding: 8px 11px; border: 1px solid rgb(255 180 84 / 0.25); border-radius: 10px; background: rgb(255 180 84 / 0.08); color: #ffd08a; font-size: 12px; }

  .fields { display: grid; gap: 2px; margin: 0; padding: 6px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 14px; background: rgb(0 0 0 / 0.14); }

  .row { display: grid; grid-template-columns: 96px minmax(0, 1fr) auto auto; align-items: center; gap: 6px; min-height: 42px; padding: 0 4px 0 12px; border-radius: 10px; }

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

  .row.meta { min-height: 0; margin-top: -6px; }

  dt { color: var(--text-3); font-size: 12px; }

  dd { margin: 0; overflow: hidden; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }

  .secret { font-family: var(--font-mono); letter-spacing: 0.02em; }

  .link { color: #b9c2ff; }

  .notes { padding: 8px 0; white-space: pre-wrap; }

  .apps { display: flex; flex-wrap: wrap; gap: 6px; white-space: normal; }

  .app { display: inline-flex; align-items: center; gap: 5px; padding: 2px 8px; border-radius: 999px; background: rgb(var(--accent-rgb) / 0.12); font-size: 11.5px; }

  .passkeys { display: grid; gap: 4px; }

  .passkeys h3 { display: flex; align-items: center; gap: 6px; margin: 0 0 2px; color: var(--text-2); font-size: 12px; font-weight: 600; }

  .passkey { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 4px 4px 12px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 10px; background: rgb(0 0 0 / 0.14); }

  .passkey-text { display: grid; flex: 1; min-width: 0; }

  .passkey-text strong,
  .passkey-text small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .passkey-text strong { font-size: 13px; font-weight: 600; }

  .passkey-text small { color: var(--text-3); font-size: 11.5px; }

  .history-toggle { display: inline-flex; align-items: center; gap: 6px; justify-self: start; color: var(--text-3); font-size: 12px; }

  .history-toggle:hover { color: var(--text-1); }

  .history { display: grid; gap: 4px; margin: -6px 0 0; padding: 0; list-style: none; }

  .history li { display: flex; justify-content: space-between; gap: 12px; padding: 6px 12px; border-radius: 8px; background: rgb(0 0 0 / 0.18); font-size: 12px; }

  .history span { color: var(--text-3); }

  footer { display: flex; align-items: center; gap: 8px; padding-top: 8px; border-top: 1px solid rgb(255 255 255 / 0.06); }

  .updated { flex: 1; color: var(--text-3); font-size: 11.5px; }

  .danger-text { color: #ff9d9d; }

  /* A big screen: a little more room, and larger type. */
  @container vault (min-width: 1700px) {
    .view {
      width: min(100%, 880px);
      gap: 22px;
    }

    .row {
      min-height: 48px;
    }

    dd {
      font-size: 14px;
    }
  }

  /* A 1080p screen (and anything short): tighter, so it all shows at once. */
  @media (max-height: 1000px) {
    .view {
      gap: 12px;
    }

    header :global(.favicon) {
      --size: 42px !important;
    }

    h2 {
      font-size: 18px;
    }

    .row {
      min-height: 36px;
    }

    footer {
      padding-top: 6px;
    }
  }
</style>
