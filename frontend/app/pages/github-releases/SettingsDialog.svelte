<script lang="ts">
  // The GitHub account (the GitHub CLI's sign-in, or a pasted token) and the
  // AI providers: their keys (checked, then kept encrypted on this PC),
  // order and models.
  import { onDestroy, onMount, untrack } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Info from "@lucide/svelte/icons/info";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Clock from "@lucide/svelte/icons/clock";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import LogOut from "@lucide/svelte/icons/log-out";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import X from "@lucide/svelte/icons/x";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { confirm } from "../../../lib/confirm.svelte";
  import { portal } from "../../../lib/portal";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf, problemOf, type GhStatus, type ProviderId, type ProviderView } from "./api";
  import GithubMark from "./GithubMark.svelte";
  import { githubReleases as gr, installTool } from "./state.svelte";

  let { section, onclose }: { section: "account" | "ai"; onclose: () => void } = $props();

  let current = $state<"account" | "ai">(untrack(() => section));
  const account = $derived(gr.page?.account ?? null);
  const ai = $derived(gr.page?.ai ?? null);

  // ─── Account ───
  let gh = $state<GhStatus | null>(null);
  let connecting = $state(false);
  let token = $state("");
  let waitingForLogin = $state(false);
  let poll: ReturnType<typeof setInterval> | undefined;

  onMount(() => {
    void loadGh();
  });
  onDestroy(() => clearInterval(poll));

  async function loadGh() {
    try {
      gh = await api.ghStatus();
    } catch {
      gh = { installed: !!gr.page?.gh, signedIn: false };
    }
  }

  async function connectGh() {
    connecting = true;
    try {
      gr.setPage(await api.connectGh());
      toast.success(`Connected as ${gr.page?.account?.login}.`);
      void gr.refreshSelected(true);
    } catch (error) {
      gr.showProblem(problemOf(error));
    } finally {
      connecting = false;
    }
  }

  async function ghLogin() {
    try {
      await api.ghLogin();
      waitingForLogin = true;
      const started = Date.now();
      clearInterval(poll);
      poll = setInterval(async () => {
        if (Date.now() - started > 5 * 60_000) {
          clearInterval(poll);
          waitingForLogin = false;
          return;
        }
        const status = await api.ghStatus().catch(() => null);
        if (status?.signedIn) {
          clearInterval(poll);
          waitingForLogin = false;
          gh = status;
          await connectGh();
        }
      }, 3000);
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  async function connectToken() {
    if (!token.trim()) return;
    connecting = true;
    try {
      gr.setPage(await api.connectToken(token));
      token = "";
      toast.success(`Connected as ${gr.page?.account?.login}.`);
      void gr.refreshSelected(true);
    } catch (error) {
      gr.showProblem(problemOf(error));
    } finally {
      connecting = false;
    }
  }

  async function disconnect() {
    const ok = await confirm({
      title: "Disconnect GitHub?",
      message: "MYLE forgets its copy of the token. Your GitHub CLI sign-in (if any) and git's own credentials stay.",
      confirmLabel: "Disconnect",
      danger: true,
    });
    if (!ok) return;
    try {
      gr.setPage(await api.disconnect());
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  const missingWorkflow = $derived(!!account && account.scopes.length > 0 && !account.scopes.includes("workflow"));

  // ─── AI ───
  let keys = $state<Partial<Record<ProviderId, string>>>({});
  let models = $state<Partial<Record<ProviderId, string>>>({});
  let saving = $state<ProviderId | null>(null);
  let testing = $state<ProviderId | null>(null);
  let ollamaModels = $state<string[] | null>(null);
  let ollamaUrl = $state(gr.page?.ai.ollamaUrl ?? "");

  $effect(() => {
    if (!ai) return;
    for (const p of ai.providers) {
      if (models[p.id] === undefined) models[p.id] = p.model === p.defaultModel ? "" : p.model;
    }
  });

  // Dragging a row by its handle previews the new order; it is saved on drop.
  let dragging = $state<ProviderId | null>(null);
  let preview = $state<ProviderId[] | null>(null);

  const ordered = $derived.by<ProviderView[]>(() => {
    if (!ai) return [];
    return (preview ?? ai.order).map((id) => ai.providers.find((p) => p.id === id)).filter((p): p is ProviderView => !!p);
  });

  /** The provider AI features use first: the first one set up. */
  const active = $derived(ordered.find((p) => p.ready)?.id ?? null);

  /** One row open at a time; with nothing set up, the first one. */
  let expanded = $state<ProviderId | null>(untrack(() => (gr.page?.ai.providers.some((p) => p.ready) ? null : (gr.page?.ai.order[0] ?? null))));

  function toggle(id: ProviderId) {
    expanded = expanded === id ? null : id;
  }

  type Status = { tone: "ok" | "warn" | "off"; text: string };

  function statusOf(p: ProviderView): Status {
    const limit = gr.aiLimits[p.id];
    if (limit && limit > now) {
      const seconds = Math.ceil((limit - now) / 1000);
      return { tone: "warn", text: seconds > 90 ? `Limit reached · ~${Math.ceil(seconds / 60)} min` : "Limit reached" };
    }
    if (!p.needsKey) return ai?.ollamaEnabled ? { tone: p.ready ? "ok" : "warn", text: p.ready ? "On" : "On · not running" } : { tone: "off", text: "Off" };
    if (p.ready) return { tone: "ok", text: p.keyHint ? `Key ${p.keyHint}` : "Ready" };
    return { tone: "off", text: "Not set" };
  }

  let now = $state(Date.now());
  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(timer);
  });

  function startDrag(event: PointerEvent, id: ProviderId) {
    if (!ai || event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragging = id;
    preview = ai.order.slice();
  }

  function dragMove(event: PointerEvent) {
    if (!dragging || !preview) return;
    const row = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-provider]");
    const over = row?.dataset.provider as ProviderId | undefined;
    if (!over || over === dragging) return;
    const next = preview.slice();
    next.splice(next.indexOf(dragging), 1);
    next.splice(next.indexOf(over) + (preview.indexOf(over) > preview.indexOf(dragging) ? 1 : 0), 0, dragging);
    preview = next;
  }

  async function endDrag() {
    const order = preview;
    dragging = null;
    preview = null;
    if (!ai || !order || order.join() === ai.order.join()) return;
    ai.order = order;
    try {
      setAi(await api.aiSetSettings({ order }));
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  function handleKey(event: KeyboardEvent, id: ProviderId) {
    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      event.preventDefault();
      void move(id, event.key === "ArrowUp" ? -1 : 1);
    }
  }

  function setAi(view: typeof ai) {
    if (gr.page && view) gr.page.ai = view;
  }

  async function saveKey(p: ProviderView) {
    const key = keys[p.id]?.trim();
    if (!key) return;
    saving = p.id;
    try {
      setAi(await api.aiSetKey(p.id, key));
      keys[p.id] = "";
      toast.success(`${p.name} is ready.`);
    } catch (error) {
      toast.error(messageOf(error));
    } finally {
      saving = null;
    }
  }

  async function removeKey(p: ProviderView) {
    try {
      setAi(await api.aiRemoveKey(p.id));
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  async function move(id: ProviderId, by: number) {
    if (!ai) return;
    const order = ai.order.slice();
    const at = order.indexOf(id);
    const to = at + by;
    if (at < 0 || to < 0 || to >= order.length) return;
    [order[at], order[to]] = [order[to], order[at]];
    try {
      setAi(await api.aiSetSettings({ order }));
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  async function saveModel(p: ProviderView) {
    const all: Partial<Record<ProviderId, string>> = {};
    for (const [id, value] of Object.entries(models)) if (value?.trim()) all[id as ProviderId] = value.trim();
    try {
      setAi(await api.aiSetSettings({ models: all }));
    } catch (error) {
      toast.error(messageOf(error));
    }
    void p;
  }

  async function setOllama(enabled: boolean) {
    try {
      setAi(await api.aiSetSettings({ ollamaEnabled: enabled, ollamaUrl: ollamaUrl.trim() || null }));
      if (enabled) await listOllama();
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  async function saveOllamaUrl() {
    try {
      setAi(await api.aiSetSettings({ ollamaUrl: ollamaUrl.trim() || null }));
      await listOllama();
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  async function listOllama() {
    try {
      ollamaModels = await api.aiOllamaModels();
    } catch (error) {
      ollamaModels = null;
      toast.error(messageOf(error));
    }
  }

  async function test(p: ProviderView) {
    testing = p.id;
    try {
      const answer = await api.aiTest(p.id);
      toast.success(`${p.name} answered (${answer.model}).`);
    } catch (error) {
      toast.error(problemOf(error).message);
    } finally {
      testing = null;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !connecting && onclose()} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal}>
  <div class="dialog glass glass--3" role="dialog" aria-modal="true" aria-labelledby="gr-settings-title" transition:scale={{ start: 0.96, duration: 180, easing: cubicOut }}>
    <header>
      <div>
        <h2 id="gr-settings-title">GitHub Releases settings</h2>
        <p>{current === "account" ? "The GitHub account releases are published with." : "Who writes commit messages and release notes."}</p>
      </div>
      <button class="icon-btn" aria-label="Close" onclick={onclose}><X size={16} /></button>
    </header>
    <nav class="seg" aria-label="Settings sections">
      <button class:active={current === "account"} aria-pressed={current === "account"} onclick={() => (current = "account")}>
        <GithubMark size={13} /> GitHub account
        {#if account}<i class="tick ok" aria-hidden="true"></i>{:else}<i class="tick" aria-hidden="true"></i>{/if}
      </button>
      <button class:active={current === "ai"} aria-pressed={current === "ai"} onclick={() => (current = "ai")}>
        <Sparkles size={13} /> AI
        {#if active}<i class="tick ok" aria-hidden="true"></i>{:else}<i class="tick" aria-hidden="true"></i>{/if}
      </button>
    </nav>

    <div class="body">
      {#if current === "account"}
        {#if account}
          <div class="who">
            {#if account.avatarUrl}<img src={account.avatarUrl} alt="" width="46" height="46" />{:else}<span class="who-mark"><GithubMark size={24} /></span>{/if}
            <div class="who-text">
              <strong>{account.name ?? account.login}</strong>
              <span>@{account.login}</span>
            </div>
            <span class="pill ok"><CircleCheck size={12} /> Connected</span>
            <button class="btn small ghost" onclick={() => void disconnect()}><LogOut size={13} /> Disconnect</button>
          </div>
          <dl class="facts">
            <div><dt>Signed in with</dt><dd>{account.source === "gh" ? "The GitHub CLI" : "A personal access token"}</dd></div>
            {#if account.scopes.length}
              <div><dt>Scopes</dt><dd class="scopes">{#each account.scopes as scope (scope)}<code class:need={scope === "workflow" || scope === "repo"}>{scope}</code>{/each}</dd></div>
            {/if}
            <div><dt>Token</dt><dd>Encrypted for your Windows account; git gets it only for a push or pull.</dd></div>
          </dl>
          {#if missingWorkflow}
            <p class="warn"><CircleAlert size={14} /> This token can't change workflow files (no <code>workflow</code> scope). Sign in again with the GitHub CLI to add it.</p>
          {/if}
        {:else}
          {@const installed = !!gh?.installed}
          {@const signedIn = !!gh?.signedIn}
          <ol class="steps">
            <li class:done={installed}>
              <span class="num">{#if installed}<CircleCheck size={15} />{:else}1{/if}</span>
              <div><strong>GitHub CLI</strong><span>{!gh ? "Checking…" : installed ? "Installed" : "GitHub's own tool; it signs you in through the browser."}</span></div>
              {#if !gh}
                <LoaderCircle size={15} class="spin" />
              {:else if !installed}
                <button class="btn small primary" onclick={() => void installTool("gh").then(() => setTimeout(loadGh, 15_000))}><Download size={13} /> Install</button>
                <button class="btn small ghost" onclick={() => void loadGh()}>Check again</button>
              {/if}
            </li>
            <li class:done={signedIn} class:off={!installed}>
              <span class="num">{#if signedIn}<CircleCheck size={15} />{:else}2{/if}</span>
              <div><strong>Sign in</strong><span>{signedIn ? "The GitHub CLI is signed in" : "A window shows a code and opens github.com."}</span></div>
              {#if installed && !signedIn}
                <button class="btn small primary" disabled={waitingForLogin} onclick={() => void ghLogin()}>
                  {#if waitingForLogin}<LoaderCircle size={13} class="spin" /> Waiting…{:else}Sign in{/if}
                </button>
              {/if}
            </li>
            <li class:off={!signedIn}>
              <span class="num">3</span>
              <div><strong>Connect</strong><span>Use that account here.</span></div>
              <button class="btn small" class:primary={signedIn} disabled={!signedIn || connecting} onclick={() => void connectGh()}>
                {#if connecting}<LoaderCircle size={13} class="spin" />{/if} Connect
              </button>
            </li>
          </ol>
          <details class="token">
            <summary><ChevronDown size={13} class="chev" /> Or paste a personal access token</summary>
            <div class="token-body">
              <p>Classic token with <code>repo</code> and <code>workflow</code>, or a fine-grained one with Contents, Actions (read) and Workflows access.</p>
              <div class="row">
                <input class="input" type="password" autocomplete="off" bind:value={token} placeholder="ghp_… or github_pat_…" onkeydown={(e) => e.key === "Enter" && void connectToken()} />
                <button class="btn small" class:primary={!!token.trim()} disabled={connecting || !token.trim()} onclick={() => void connectToken()}>Connect</button>
              </div>
              <button class="link" onclick={() => void openUrl("https://github.com/settings/tokens/new?scopes=repo,workflow&description=MYLE%20GitHub%20Releases")}>
                Create a token on GitHub <ExternalLink size={11} />
              </button>
            </div>
          </details>
          <p class="note"><ShieldCheck size={13} /> The token is kept encrypted for your Windows account, never on a command line or in .git/config.</p>
        {/if}
      {:else if ai}
        <p class="lead">Used from the top. If one fails or hits its limit, MYLE offers the next one; it never switches by itself. Drag <GripVertical size={12} /> to reorder.</p>
        <ol class="providers" class:dragging={!!dragging}>
          {#each ordered as p, i (p.id)}
            {@const st = statusOf(p)}
            {@const open = expanded === p.id}
            <li data-provider={p.id} class:open class:is-dragging={dragging === p.id}>
              <div class="p-row">
                <button
                  class="grip"
                  aria-label="Move {p.name} (arrow keys)"
                  title="Drag to reorder, or use the arrow keys"
                  onpointerdown={(e) => startDrag(e, p.id)}
                  onpointermove={dragMove}
                  onpointerup={() => void endDrag()}
                  onpointercancel={() => void endDrag()}
                  onkeydown={(e) => handleKey(e, p.id)}
                >
                  <GripVertical size={14} />
                </button>
                <span class="rank">{i + 1}</span>
                <button class="p-main" aria-expanded={open} onclick={() => toggle(p.id)}>
                  <span class="p-name">
                    <strong>{p.name}</strong>
                    {#if active === p.id}<em class="used">Used first</em>{/if}
                  </span>
                  <small>{p.free}</small>
                </button>
                <span class="pill {st.tone}">
                  {#if st.tone === "ok"}<CircleCheck size={12} />{:else if st.tone === "warn"}<Clock size={12} />{/if}
                  {st.text}
                </span>
                <button class="icon-btn chev-btn" aria-label={open ? "Close" : "Edit"} onclick={() => toggle(p.id)}><ChevronDown size={15} /></button>
              </div>

              {#if open}
                <div class="p-edit">
                  {#if p.needsKey}
                    <div class="row">
                      <input
                        class="input"
                        type="password"
                        autocomplete="off"
                        bind:value={keys[p.id]}
                        placeholder={p.ready ? "Paste a new key to replace it" : `Paste your ${p.name} API key`}
                        onkeydown={(e) => e.key === "Enter" && void saveKey(p)}
                      />
                      <button
                        class="btn small save"
                        class:primary={!!keys[p.id]?.trim()}
                        disabled={saving === p.id || !keys[p.id]?.trim()}
                        title={keys[p.id]?.trim() ? "Check the key with the provider, then save it" : "Paste a key first"}
                        onclick={() => void saveKey(p)}
                      >
                        {#if saving === p.id}<LoaderCircle size={13} class="spin" />{:else}<KeyRound size={13} />{/if} Check & save
                      </button>
                    </div>
                    <div class="row small">
                      <button class="link" onclick={() => void openUrl(p.keyUrl)}>Get a {p.id === "deepseek" ? "" : "free "}key <ExternalLink size={11} /></button>
                      {#if p.ready}<button class="link danger" onclick={() => void removeKey(p)}>Remove the key</button>{/if}
                    </div>
                  {:else}
                    <label class="row switch-row">
                      <input type="checkbox" class="switch" checked={ai.ollamaEnabled} onchange={(e) => void setOllama((e.currentTarget as HTMLInputElement).checked)} />
                      <span>Use Ollama on this PC</span>
                    </label>
                    {#if ai.ollamaEnabled}
                      <div class="row">
                        <input class="input mono" bind:value={ollamaUrl} placeholder="http://localhost:11434" onblur={() => void saveOllamaUrl()} />
                        <button class="btn small ghost" onclick={() => void listOllama()}>Models</button>
                      </div>
                      {#if ollamaModels}
                        <div class="models">
                          {#each ollamaModels as m (m)}
                            <button class="chip" class:active={(models.ollama || p.defaultModel) === m} onclick={() => ((models.ollama = m), void saveModel(p))}>{m}</button>
                          {:else}
                            <span class="hint">No models yet: run <code>ollama pull llama3.2</code>.</span>
                          {/each}
                        </div>
                      {/if}
                    {:else}
                      <button class="link" onclick={() => void openUrl(p.keyUrl)}>Get Ollama <ExternalLink size={11} /></button>
                    {/if}
                  {/if}

                  {#if p.ready}
                    <div class="row model">
                      <span>Model</span>
                      <input class="input mono" bind:value={models[p.id]} placeholder={p.defaultModel} onblur={() => void saveModel(p)} spellcheck="false" />
                      <button class="btn small ghost" disabled={testing === p.id} onclick={() => void test(p)}>
                        {#if testing === p.id}<LoaderCircle size={13} class="spin" />{/if} Test
                      </button>
                    </div>
                  {/if}
                  <p class="privacy"><Info size={12} /> {p.privacy}</p>
                </div>
              {/if}
            </li>
          {/each}
        </ol>
        <p class="note"><ShieldCheck size={13} /> Keys are checked with the provider, then kept encrypted for your Windows account; never shown again or synced. Your diffs go only to the provider that writes.</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 91;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--scrim);
  }

  .dialog {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: 14px;
    width: min(600px, 100%);
    max-height: min(720px, 100%);
    padding: 20px;
    border-radius: var(--radius-xl);
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  h2 {
    margin: 0 0 3px;
    font-size: 17px;
  }

  header p {
    margin: 0;
    color: var(--text-3);
    font-size: 12px;
  }

  .seg {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 3px;
    padding: 3px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 11px;
    background: rgb(0 0 0 / 0.18);
  }

  .seg button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    height: 32px;
    border-radius: 8px;
    color: var(--text-2);
    font-size: 12.5px;
  }

  .seg button:hover {
    color: var(--text-1);
  }

  .seg button.active {
    background: rgb(var(--accent-rgb) / 0.18);
    color: var(--text-1);
    box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.35);
  }

  .tick {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: rgb(255 255 255 / 0.2);
  }

  .tick.ok {
    background: #3ecf8e;
  }

  .body {
    display: grid;
    align-content: start;
    gap: 12px;
    min-height: 0;
    padding-right: 4px;
    overflow: auto;
  }

  .lead {
    margin: 0;
    color: var(--text-2);
    font-size: 12.3px;
    line-height: 1.5;
  }

  .lead :global(svg) {
    vertical-align: -2px;
  }

  .note {
    display: flex;
    gap: 7px;
    margin: 0;
    color: var(--text-3);
    font-size: 11.3px;
    line-height: 1.45;
  }

  .note :global(svg) {
    flex: none;
    margin-top: 1px;
  }

  .warn {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 9px 11px;
    border-radius: 9px;
    background: rgb(255 180 84 / 0.08);
    color: #ffd08a;
    font-size: 12px;
  }

  /* Account */
  .who {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px;
    border: 1px solid rgb(62 207 142 / 0.2);
    border-radius: 12px;
    background: linear-gradient(135deg, rgb(62 207 142 / 0.07), rgb(255 255 255 / 0.02));
  }

  .who img {
    border-radius: 50%;
  }

  .who-mark {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 50%;
    background: rgb(255 255 255 / 0.06);
  }

  .who-text {
    display: grid;
    flex: 1;
    gap: 1px;
    min-width: 0;
  }

  .who-text strong {
    font-size: 14px;
  }

  .who-text span {
    color: var(--text-3);
    font-size: 12px;
  }

  .facts {
    display: grid;
    margin: 0;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 12px;
    background: rgb(0 0 0 / 0.12);
  }

  .facts > div {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    gap: 12px;
    padding: 9px 13px;
    font-size: 12.2px;
  }

  .facts > div + div {
    border-top: 1px solid rgb(255 255 255 / 0.05);
  }

  dt {
    color: var(--text-3);
  }

  dd {
    margin: 0;
    color: var(--text-1);
  }

  .scopes {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .scopes code {
    padding: 0 6px;
    border-radius: 5px;
    background: rgb(255 255 255 / 0.06);
    color: var(--text-2);
    line-height: 18px;
  }

  .scopes code.need {
    background: rgb(var(--accent-rgb) / 0.16);
    color: #c9cffb;
  }

  .steps {
    display: grid;
    margin: 0;
    padding: 0;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 12px;
    background: rgb(0 0 0 / 0.12);
    list-style: none;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 11px 13px;
  }

  .steps li + li {
    border-top: 1px solid rgb(255 255 255 / 0.05);
  }

  .steps li.off {
    opacity: 0.55;
  }

  .num {
    display: grid;
    flex: none;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 1px solid rgb(var(--accent-rgb) / 0.35);
    border-radius: 50%;
    color: #c9cffb;
    font-size: 12px;
    font-weight: 600;
  }

  .done .num {
    border-color: rgb(62 207 142 / 0.4);
    color: #6fdba5;
  }

  .steps li > div {
    display: grid;
    flex: 1;
    gap: 1px;
    min-width: 0;
  }

  .steps strong {
    font-size: 12.8px;
  }

  .steps li > div span {
    color: var(--text-3);
    font-size: 11.6px;
  }

  .token {
    color: var(--text-2);
    font-size: 12.3px;
  }

  .token summary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    list-style: none;
  }

  .token summary::-webkit-details-marker {
    display: none;
  }

  .token:not([open]) :global(.chev) {
    transform: rotate(-90deg);
  }

  .token-body {
    display: grid;
    gap: 8px;
    margin-top: 8px;
    padding: 11px 12px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 11px;
  }

  .token-body p {
    margin: 0;
    color: var(--text-3);
    font-size: 11.6px;
  }

  /* AI */
  .providers {
    display: grid;
    margin: 0;
    padding: 0;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 12px;
    background: rgb(0 0 0 / 0.12);
    list-style: none;
  }

  .providers li + li {
    border-top: 1px solid rgb(255 255 255 / 0.05);
  }

  .providers li.open {
    background: rgb(255 255 255 / 0.025);
  }

  .providers li.is-dragging {
    background: rgb(var(--accent-rgb) / 0.1);
  }

  .providers.dragging {
    cursor: grabbing;
    user-select: none;
  }

  .p-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 52px;
    padding: 6px 8px 6px 4px;
  }

  .grip {
    display: grid;
    flex: none;
    place-items: center;
    width: 22px;
    height: 32px;
    border-radius: 6px;
    color: var(--text-3);
    cursor: grab;
    touch-action: none;
  }

  .grip:hover,
  .grip:focus-visible {
    background: rgb(255 255 255 / 0.05);
    color: var(--text-1);
  }

  .rank {
    flex: none;
    width: 16px;
    color: var(--text-3);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  .p-main {
    display: grid;
    flex: 1;
    gap: 2px;
    min-width: 0;
    padding: 4px 2px;
    text-align: left;
  }

  .p-name {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .p-name strong {
    font-size: 13px;
  }

  .used {
    padding: 0 6px;
    border-radius: 5px;
    background: rgb(var(--accent-rgb) / 0.2);
    color: #c9cffb;
    font-size: 10px;
    font-style: normal;
    line-height: 16px;
  }

  .p-main small {
    overflow: hidden;
    color: var(--text-3);
    font-size: 11.3px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pill {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 5px;
    padding: 0 9px;
    border-radius: 999px;
    font-size: 11px;
    line-height: 22px;
    white-space: nowrap;
  }

  .pill.ok {
    background: rgb(62 207 142 / 0.12);
    color: #6fdba5;
  }

  .pill.warn {
    background: rgb(255 180 84 / 0.12);
    color: #ffd08a;
  }

  .pill.off {
    background: rgb(255 255 255 / 0.05);
    color: var(--text-3);
  }

  .chev-btn {
    width: 28px;
    height: 28px;
  }

  .chev-btn :global(svg) {
    transition: transform var(--dur-fast);
  }

  li.open .chev-btn :global(svg) {
    transform: rotate(180deg);
  }

  .p-edit {
    display: grid;
    gap: 9px;
    padding: 2px 14px 13px 52px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .row .input {
    flex: 1;
    min-width: 0;
  }

  .row.small {
    gap: 14px;
  }

  .save:disabled {
    opacity: 0.55;
  }

  .mono {
    font-family: var(--font-mono);
    font-size: 11.8px;
  }

  .model span {
    color: var(--text-3);
    font-size: 11.8px;
  }

  .privacy {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--text-3);
    font-size: 11px;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    padding: 0;
    color: #b7befa;
    font-size: 12px;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .link.danger {
    color: #ff9d9d;
  }

  .switch-row {
    color: var(--text-2);
    font-size: 12.3px;
  }

  .models {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .hint {
    color: var(--text-3);
    font-size: 11.8px;
  }

  code {
    font-family: var(--font-mono);
    font-size: 11px;
  }
</style>
