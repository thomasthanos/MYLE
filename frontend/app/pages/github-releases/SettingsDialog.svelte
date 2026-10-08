<script lang="ts">
  // The GitHub account (the GitHub CLI's sign-in, or a pasted token) and the
  // AI providers: their keys (checked, then kept encrypted on this PC),
  // order and models.
  import { onDestroy, onMount, untrack } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Download from "@lucide/svelte/icons/download";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import LogOut from "@lucide/svelte/icons/log-out";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import SquareTerminal from "@lucide/svelte/icons/square-terminal";
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

  const ordered = $derived.by<ProviderView[]>(() => {
    if (!ai) return [];
    return ai.order.map((id) => ai.providers.find((p) => p.id === id)).filter((p): p is ProviderView => !!p);
  });

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
      <h2 id="gr-settings-title">GitHub Releases settings</h2>
      <button class="icon-btn" aria-label="Close" onclick={onclose}><X size={16} /></button>
    </header>
    <nav class="tabs">
      <button class="chip" class:active={current === "account"} onclick={() => (current = "account")}><GithubMark size={13} /> GitHub account</button>
      <button class="chip" class:active={current === "ai"} onclick={() => (current = "ai")}><Sparkles size={13} /> AI</button>
    </nav>

    <div class="body">
      {#if current === "account"}
        {#if account}
          <div class="account">
            {#if account.avatarUrl}<img src={account.avatarUrl} alt="" width="44" height="44" />{:else}<GithubMark size={30} />{/if}
            <div>
              <strong>{account.name ?? account.login}</strong>
              <span>@{account.login} · {account.source === "gh" ? "from the GitHub CLI" : "with a personal access token"}</span>
              {#if account.scopes.length}<small>Scopes: {account.scopes.join(", ")}</small>{/if}
            </div>
            <button class="btn small ghost" onclick={() => void disconnect()}><LogOut size={13} /> Disconnect</button>
          </div>
          {#if missingWorkflow}
            <p class="warn">This token can't push changes to workflow files (it lacks the "workflow" scope). Sign in again with the GitHub CLI to add it.</p>
          {/if}
          <p class="note"><ShieldCheck size={13} /> The token is kept encrypted for your Windows account. Git gets it only for a push or pull, never on a command line or in .git/config.</p>
        {:else}
          <p class="lead">MYLE uses your GitHub CLI sign-in. Your token is kept encrypted on this PC.</p>
          {#if !gh}
            <div class="quiet"><LoaderCircle size={15} class="spin" /></div>
          {:else if !gh.installed}
            <div class="step">
              <SquareTerminal size={18} />
              <div>
                <strong>Install the GitHub CLI</strong>
                <span>GitHub's own command-line tool signs you in through the browser. It installs with winget in a window you can watch.</span>
              </div>
              <button class="btn small primary" onclick={() => void installTool("gh").then(() => setTimeout(loadGh, 15_000))}><Download size={13} /> Install</button>
            </div>
            <button class="btn small ghost" onclick={() => void loadGh()}>I installed it</button>
          {:else if !gh.signedIn}
            <div class="step">
              <GithubMark size={18} />
              <div>
                <strong>Sign in with the GitHub CLI</strong>
                <span>A window opens; it shows a code and opens github.com in your browser. Come back when it says you're logged in.</span>
              </div>
              <button class="btn small primary" disabled={waitingForLogin} onclick={() => void ghLogin()}>
                {#if waitingForLogin}<LoaderCircle size={13} class="spin" /> Waiting…{:else}Sign in{/if}
              </button>
            </div>
          {:else}
            <div class="step">
              <CircleCheck size={18} />
              <div>
                <strong>The GitHub CLI is signed in</strong>
                <span>Use that account here.</span>
              </div>
              <button class="btn small primary" disabled={connecting} onclick={() => void connectGh()}>
                {#if connecting}<LoaderCircle size={13} class="spin" />{/if} Connect
              </button>
            </div>
          {/if}
          <details class="token">
            <summary>Or paste a personal access token</summary>
            <p>Classic token with the <code>repo</code> and <code>workflow</code> scopes, or a fine-grained one with Contents, Actions (read) and Workflows access.</p>
            <div class="row">
              <input class="input" type="password" autocomplete="off" bind:value={token} placeholder="ghp_… or github_pat_…" onkeydown={(e) => e.key === "Enter" && void connectToken()} />
              <button class="btn small primary" disabled={connecting || !token.trim()} onclick={() => void connectToken()}>Connect</button>
            </div>
            <button class="link" onclick={() => void openUrl("https://github.com/settings/tokens/new?scopes=repo,workflow&description=MYLE%20GitHub%20Releases")}>
              Create a token on GitHub <ExternalLink size={11} />
            </button>
          </details>
        {/if}
      {:else if ai}
        <p class="lead">
          AI writes commit messages and release notes from your diffs and commits. They go to the provider you use, so pick one you trust
          with your code. If one fails or hits its limit, MYLE offers the next one (it never switches by itself).
        </p>
        <ol class="providers">
          {#each ordered as p, i (p.id)}
            <li class:ready={p.ready}>
              <div class="p-head">
                <span class="order">
                  <button class="icon-btn" aria-label="Move up" disabled={i === 0} onclick={() => void move(p.id, -1)}><ArrowUp size={12} /></button>
                  <button class="icon-btn" aria-label="Move down" disabled={i === ordered.length - 1} onclick={() => void move(p.id, 1)}><ArrowDown size={12} /></button>
                </span>
                <div class="p-title">
                  <strong>{p.name} {#if i === 0}<em>first</em>{/if}</strong>
                  <span>{p.free}</span>
                  <small>{p.privacy}</small>
                </div>
                {#if p.ready}
                  <span class="ok"><CircleCheck size={13} /> {p.keyHint ? `Key ${p.keyHint}` : "On"}</span>
                {/if}
              </div>

              {#if p.needsKey}
                <div class="row">
                  <input class="input" type="password" autocomplete="off" bind:value={keys[p.id]} placeholder={p.ready ? "Replace the key" : "Paste your API key"} onkeydown={(e) => e.key === "Enter" && void saveKey(p)} />
                  <button class="btn small primary" disabled={saving === p.id || !keys[p.id]?.trim()} onclick={() => void saveKey(p)}>
                    {#if saving === p.id}<LoaderCircle size={13} class="spin" />{:else}<KeyRound size={13} />{/if} Check & save
                  </button>
                  {#if p.ready}<button class="btn small ghost" onclick={() => void removeKey(p)}>Remove</button>{/if}
                </div>
                {#if !p.ready}
                  <button class="link" onclick={() => void openUrl(p.keyUrl)}>Get a {p.id === "deepseek" ? "" : "free "}key <ExternalLink size={11} /></button>
                {/if}
              {:else}
                <label class="row switch-row">
                  <input type="checkbox" class="switch" checked={ai.ollamaEnabled} onchange={(e) => void setOllama((e.currentTarget as HTMLInputElement).checked)} />
                  <span>Use Ollama</span>
                </label>
                {#if ai.ollamaEnabled}
                  <div class="row">
                    <input class="input" bind:value={ollamaUrl} placeholder="http://localhost:11434" onblur={() => void saveOllamaUrl()} />
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
                  <input class="input" bind:value={models[p.id]} placeholder={p.defaultModel} onblur={() => void saveModel(p)} spellcheck="false" />
                  <button class="btn small ghost" disabled={testing === p.id} onclick={() => void test(p)}>
                    {#if testing === p.id}<LoaderCircle size={13} class="spin" />{/if} Test
                  </button>
                </div>
              {/if}
            </li>
          {/each}
        </ol>
        <p class="note"><ShieldCheck size={13} /> Keys are checked with the provider, then kept encrypted for your Windows account. They are never shown again or synced.</p>
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
    background: rgb(4 6 12 / 0.58);
  }

  .dialog {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: 12px;
    width: min(640px, 100%);
    max-height: min(760px, 100%);
    padding: 20px;
    border-radius: var(--radius-xl);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    font-size: 17px;
  }

  .tabs {
    display: flex;
    gap: 6px;
  }

  .tabs .chip {
    gap: 6px;
  }

  .body {
    display: grid;
    align-content: start;
    gap: 12px;
    min-height: 0;
    padding-right: 4px;
    overflow: auto;
    scrollbar-width: thin;
  }

  .lead {
    color: var(--text-2);
    font-size: 12.5px;
    line-height: 1.55;
  }

  .note {
    display: flex;
    gap: 7px;
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.45;
  }

  .warn {
    padding: 9px 11px;
    border-radius: 9px;
    background: rgb(255 180 84 / 0.08);
    color: #ffd08a;
    font-size: 12px;
  }

  .account {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px;
    border-radius: 12px;
    background: rgb(255 255 255 / 0.04);
  }

  .account img {
    border-radius: 50%;
  }

  .account > div {
    display: grid;
    flex: 1;
    gap: 2px;
    min-width: 0;
  }

  .account span,
  .account small {
    color: var(--text-3);
    font-size: 11.8px;
  }

  .step {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px;
    border: 1px solid rgb(var(--accent-rgb) / 0.25);
    border-radius: 12px;
    background: rgb(var(--accent-rgb) / 0.07);
    color: #c9cffb;
  }

  .step > div {
    display: grid;
    flex: 1;
    gap: 3px;
  }

  .step strong {
    color: var(--text-1);
    font-size: 13px;
  }

  .step span {
    color: var(--text-2);
    font-size: 12px;
    line-height: 1.45;
  }

  .token {
    display: grid;
    gap: 8px;
    color: var(--text-2);
    font-size: 12.3px;
  }

  .token summary {
    cursor: pointer;
  }

  .token p {
    margin: 8px 0;
    color: var(--text-3);
    font-size: 11.8px;
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

  .link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    margin-top: 6px;
    padding: 0;
    color: #b7befa;
    font-size: 12px;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .providers {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .providers li {
    display: grid;
    gap: 8px;
    padding: 11px 12px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 12px;
    background: rgb(255 255 255 / 0.025);
  }

  .providers li.ready {
    border-color: rgb(62 207 142 / 0.25);
  }

  .p-head {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .order {
    display: grid;
    gap: 2px;
  }

  .order .icon-btn {
    width: 22px;
    height: 20px;
  }

  .p-title {
    display: grid;
    flex: 1;
    gap: 2px;
    min-width: 0;
  }

  .p-title strong {
    font-size: 13px;
  }

  .p-title em {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: 5px;
    background: rgb(var(--accent-rgb) / 0.18);
    color: #c9cffb;
    font-size: 10px;
    font-style: normal;
  }

  .p-title span {
    color: var(--text-2);
    font-size: 11.8px;
  }

  .p-title small {
    color: var(--text-3);
    font-size: 11.3px;
  }

  .ok {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 5px;
    color: #3ecf8e;
    font-size: 11.5px;
  }

  .model span {
    color: var(--text-3);
    font-size: 11.8px;
  }

  .model .input {
    font-family: var(--font-mono);
    font-size: 11.8px;
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

  .quiet {
    display: grid;
    place-items: center;
    min-height: 60px;
    color: var(--text-3);
  }
</style>
