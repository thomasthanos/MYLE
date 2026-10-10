<script lang="ts">
  // The Windows program the hotkey was pressed in: the logins linked to it
  // and what to type into it, or linking a login to it.
  import AppWindow from "@lucide/svelte/icons/app-window";
  import Copy from "@lucide/svelte/icons/copy";
  import Link from "@lucide/svelte/icons/link";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { toast } from "../../../lib/toast.svelte";
  import { passwordsApi as api, type WindowsMatch } from "./api";
  import { passwords as p } from "./state.svelte";

  let { hotkey }: { hotkey: string | null | undefined } = $props();

  let matches = $state<WindowsMatch[]>([]);
  let linking = $state(false);
  let query = $state("");
  let busy = $state(false);

  // Asked again when the program or the logins change.
  $effect(() => {
    const target = p.windowsTarget;
    void p.entries;
    if (!target) {
      matches = [];
      linking = false;
      return;
    }
    void api.windowsMatches().then((found) => (matches = found)).catch(() => (matches = []));
  });

  const rows = $derived(
    matches.flatMap((match) => {
      const entry = p.entry(match.id);
      return entry ? [{ entry, exact: match.exact }] : [];
    }),
  );
  const linked = $derived(new Set(rows.filter((row) => row.exact).map((row) => row.entry.id)));
  const candidates = $derived.by(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return p.entries
      .filter((entry) => !linked.has(entry.id))
      .filter((entry) => {
        const text = [entry.title, entry.username, ...entry.urls].join(" ").toLowerCase();
        return words.every((word) => text.includes(word));
      })
      .slice(0, 6);
  });
  const key = $derived(hotkey ?? "the hotkey");
  /** The program's own name, as its file is written ("Battle.net"). */
  const programName = $derived((p.windowsTarget?.path.split(/[\\/]/).pop() ?? "").replace(/\.exe$/i, ""));

  async function fill(id: string, field: "username" | "password" | "both") {
    busy = true;
    try {
      await api.windowsFill(id, field);
      p.windowsTarget = null;
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      busy = false;
    }
  }

  async function link(id: string) {
    busy = true;
    try {
      await api.windowsLink(id);
      await p.refresh();
      linking = false;
      query = "";
      toast.success(`Linked to ${p.windowsTarget?.exe ?? "the program"}. Fill it in now.`);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      busy = false;
    }
  }

  function newLogin() {
    const target = p.windowsTarget;
    if (!target) return;
    p.panel = { kind: "edit", id: null, program: { exe: target.path, name: programName } };
  }
</script>

{#if p.windowsTarget}
  <div class="windows-fill">
    <div class="head">
      <AppWindow size={17} />
      <span>
        <strong>Fill in {p.windowsTarget.exe}</strong>
        <small>
          {#if p.windowsTarget.elevated}
            It runs as administrator, so Windows does not let MYLE type into it: copy the values instead.
          {:else}
            Pick what to type into the field you clicked before pressing {key}.
          {/if}
        </small>
        <code class="path" title={p.windowsTarget.path}>{p.windowsTarget.path}</code>
      </span>
      <button class="icon-btn" title="Dismiss" aria-label="Dismiss Windows fill" onclick={() => (p.windowsTarget = null)}><X size={15} /></button>
    </div>

    {#each rows as { entry, exact } (entry.id)}
      <div class="row" class:legacy={!exact}>
        <span class="name">
          <strong>{entry.title}</strong><small>{entry.username || "No user name"}</small>
          {#if !exact}<small class="guidance">Linked by file name only, which is not enough to fill: link it to this program.</small>{/if}
        </span>
        {#if !exact}
          <button class="btn small" disabled={busy} onclick={() => link(entry.id)}><Link size={13} /> Link to this program</button>
        {:else if p.windowsTarget.elevated}
          <button class="btn small" disabled={!entry.username} onclick={() => p.copy(entry.id, "username")}><Copy size={13} /> User name</button>
          <button class="btn small" disabled={!entry.hasPassword} onclick={() => p.copy(entry.id, "password")}><Copy size={13} /> Password</button>
        {:else}
          <button class="btn small primary" disabled={busy || !entry.username || !entry.hasPassword} title="The user name, Tab, then the password" onclick={() => fill(entry.id, "both")}>Fill both</button>
          <button class="btn small" disabled={busy || !entry.username} onclick={() => fill(entry.id, "username")}>User name</button>
          <button class="btn small" disabled={busy || !entry.hasPassword} onclick={() => fill(entry.id, "password")}>Password</button>
        {/if}
      </div>
    {/each}

    {#if !rows.length && !linking}
      <div class="empty">
        <span>No login is linked to <code>{p.windowsTarget.exe}</code> yet.</span>
        <button class="btn small primary" onclick={newLogin}><Plus size={13} /> New login for it</button>
        <button class="btn small" onclick={() => (linking = true)}><Link size={13} /> Link a login…</button>
      </div>
    {:else if !linking}
      <button class="more" onclick={() => (linking = true)}><Link size={12} /> Link another login…</button>
    {/if}

    {#if linking}
      <div class="linking">
        <label class="search">
          <Search size={13} />
          <!-- svelte-ignore a11y_autofocus -->
          <input bind:value={query} placeholder="Find the login for {programName}" spellcheck="false" autofocus />
        </label>
        {#each candidates as entry (entry.id)}
          <div class="row">
            <span class="name"><strong>{entry.title}</strong><small>{entry.username || "No user name"}</small></span>
            <button class="btn small" disabled={busy} onclick={() => link(entry.id)}><Link size={13} /> Link</button>
          </div>
        {:else}
          <p class="note">No login matches. <button class="text-btn" onclick={newLogin}>Make a new one</button></p>
        {/each}
        <button class="more" onclick={() => ((linking = false), (query = ""))}>Cancel</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .windows-fill { display: grid; gap: 8px; padding: 11px 13px; border: 1px solid rgb(var(--accent-rgb) / 0.34); border-radius: 12px; background: rgb(var(--accent-rgb) / 0.09); }

  .head,
  .row,
  .empty { display: flex; align-items: center; gap: 10px; }

  .head > span,
  .name { display: grid; flex: 1; min-width: 0; gap: 2px; }

  .head strong,
  .name strong { color: var(--text-1); font-size: 12.5px; }

  .head small,
  .name small,
  .empty,
  .note { color: var(--text-3); font-size: 11.5px; }

  .path { overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }

  .row { padding: 7px 0 0 27px; border-top: 1px solid rgb(255 255 255 / 0.06); }

  .row.legacy { opacity: 0.85; }

  .guidance { color: #ffd08a !important; }

  .empty { flex-wrap: wrap; padding: 4px 0 0 27px; }

  .empty span { flex: 1; }

  .more,
  .text-btn { display: inline-flex; align-items: center; gap: 5px; justify-self: start; margin-left: 27px; color: #b9c2ff; font-size: 11.5px; }

  .text-btn { margin: 0; }

  .more:hover,
  .text-btn:hover { text-decoration: underline; }

  .linking { display: grid; gap: 6px; }

  .search { display: flex; align-items: center; gap: 7px; height: 32px; margin-left: 27px; padding: 0 10px; border: 1px solid rgb(255 255 255 / 0.1); border-radius: 9px; background: rgb(0 0 0 / 0.2); color: var(--text-3); }

  .search input { flex: 1; min-width: 0; border: 0; outline: none; background: none; color: var(--text-1); font: inherit; font-size: 12.5px; }

  .note { margin: 0 0 0 27px; }

  @container vault (max-width: 640px) {
    .row {
      flex-wrap: wrap;
      padding-left: 0;
    }

    .name {
      flex-basis: 100%;
    }
  }
</style>
