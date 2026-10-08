<script lang="ts">
  // Building a project: the detected commands (or a typed one), a live log,
  // and a Problems panel whose items open in VS Code.
  import { onMount, untrack } from "svelte";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Copy from "@lucide/svelte/icons/copy";
  import Download from "@lucide/svelte/icons/download";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Package from "@lucide/svelte/icons/package";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import Select from "../../../lib/components/Select.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import {
    githubReleasesApi as api,
    messageOf,
    type Artifact,
    type BuildInfo,
    type EntryStatus,
  } from "./api";
  import BuildLog from "./BuildLog.svelte";
  import { duration, formatBytes, formatRelative, githubReleases as gr, type ListItem } from "./state.svelte";

  let { item, entry }: { item: ListItem; entry: EntryStatus } = $props();

  const repoId = $derived(item.repo.id);
  // The panel is made again for another project, so the id stays.
  const session = gr.build(untrack(() => entry.id));
  const busy = $derived(gr.running(repoId));

  let info = $state<BuildInfo | null>(null);
  let error = $state<string | null>(null);
  let customOpen = $state(false);
  let custom = $state("");
  let problemsOpen = $state(false);
  let copied = $state<string | null>(null);

  onMount(() => {
    void load();
  });

  async function load() {
    try {
      info = await api.buildInfo(entry.id);
      error = null;
      custom = info.customCommand ?? "";
      if (info.customCommand) customOpen = true;
    } catch (e) {
      error = messageOf(e);
    }
  }

  const options = $derived((info?.plan.options ?? []).map((o) => ({ value: o.id, label: o.label })));
  const choice = $derived(info?.choice ?? info?.plan.options[0]?.id ?? "");
  const command = $derived(info?.customCommand ?? info?.plan.options.find((o) => o.id === choice)?.command ?? info?.plan.options[0]?.command ?? null);

  async function choose(id: string) {
    if (!info) return;
    info.choice = id;
    info.customCommand = null;
    custom = "";
    try {
      await api.setEntry(entry.id, { buildChoice: id, buildCommand: null });
    } catch (e) {
      toast.error(messageOf(e));
    }
  }

  async function saveCustom() {
    if (!info) return;
    const value = custom.trim();
    try {
      await api.setEntry(entry.id, { buildCommand: value || null });
      info.customCommand = value || null;
      toast.success(value ? "Custom build command saved." : "Using the detected command again.");
    } catch (e) {
      toast.error(messageOf(e));
    }
  }

  async function build(install: boolean) {
    const cmd = install ? info?.plan.installCommand : command;
    if (!cmd) return;
    const s = session;
    s.reset(cmd, install);
    problemsOpen = !install;
    try {
      const result = await api.build(entry.id, cmd, install, (event) => s.apply(event));
      s.finish(result.outcome);
      if (!install) {
        s.artifacts = result.artifacts;
        s.selected = result.selected;
        s.startedAt = result.startedAt;
      }
      if (result.outcome.ok) toast.success(install ? "Dependencies installed." : `Build finished in ${duration(result.outcome.durationMs)}.`);
      else if (!result.outcome.cancelled) problemsOpen = true;
    } catch (e) {
      s.running = false;
      s.outcome = null;
      toast.error(messageOf(e));
    }
  }

  async function cancel() {
    await api.cancel(repoId);
  }


  async function reveal(path: string) {
    try {
      await api.reveal(entry.id, path);
    } catch (e) {
      toast.error(messageOf(e));
    }
  }

  async function checksum(artifact: Artifact) {
    try {
      const sha = await api.checksum(entry.id, artifact.path);
      await navigator.clipboard.writeText(sha);
      copied = artifact.path;
      setTimeout(() => (copied === artifact.path ? (copied = null) : null), 2000);
    } catch (e) {
      toast.error(messageOf(e));
    }
  }

  const kindLabels: Record<string, string> = { installer: "Installer", program: "Program", package: "Package", archive: "Archive", update: "Update file", signature: "Signature" };
</script>

<div class="build">
  {#if error}
    <p class="error">{error}</p>
  {:else if !info}
    <div class="quiet"><LoaderCircle size={16} class="spin" /></div>
  {:else}
    <div class="bar">
      {#if customOpen}
        <input class="input" bind:value={custom} placeholder="The command that builds this project" spellcheck="false" onkeydown={(e) => e.key === "Enter" && void saveCustom()} />
        <button class="btn small" onclick={() => void saveCustom()}>Save</button>
        <button class="icon-btn" title="Use a detected command" aria-label="Use a detected command" onclick={() => (customOpen = false)}><Pencil size={14} /></button>
      {:else if options.length}
        <Select value={choice} {options} fullWidth ariaLabel="Build command" onchange={choose} />
        <button class="icon-btn" title="Type a custom command" aria-label="Type a custom command" onclick={() => (customOpen = true)}><Pencil size={14} /></button>
      {:else}
        <span class="none">No build command was found for this project.</span>
        <button class="btn small" onclick={() => (customOpen = true)}><Pencil size={13} /> Type one</button>
      {/if}

      {#if info.plan.needsInstall && info.plan.installCommand && !session.running}
        <button class="btn small" disabled={!!busy} title={info.plan.installCommand} onclick={() => void build(true)}>
          <Download size={13} /> Install dependencies
        </button>
      {/if}
      {#if session.running}
        <button class="btn danger" onclick={() => void cancel()}><Square size={13} /> Cancel</button>
      {:else}
        <button class="btn primary" disabled={!!busy || !(customOpen ? custom.trim() : command)} onclick={() => (customOpen && custom.trim() !== (info?.customCommand ?? "") ? saveCustom().then(() => build(false)) : build(false))}>
          <Play size={14} /> Build
        </button>
      {/if}
    </div>

    {#if command && !customOpen}
      <code class="cmd selectable">{command}</code>
    {/if}
    {#if info.lastBuild && !session.running && !session.outcome}
      <span class="last">Last build {formatRelative(info.lastBuild.at)} · {duration(info.lastBuild.durationMs)} · {info.lastBuild.lines.toLocaleString()} lines</span>
    {/if}

    {#if session.running || session.outcome || session.lines.length}
      <BuildLog {session} entryId={entry.id} bind:problemsOpen />
    {:else}
      <div class="quiet"><Play size={20} /> Build the project to see its output here.</div>
    {/if}

    {#if session.artifacts.length}
      <div class="artifacts">
        <div class="art-head"><Package size={14} /> Files the build made <span>{session.artifacts.length}</span></div>
        {#each session.artifacts as artifact (artifact.path)}
          <div class="artifact">
            <div class="art-text">
              <strong>{artifact.name}</strong>
              <small>{kindLabels[artifact.kind] ?? "File"} · {formatBytes(artifact.size)} · {artifact.rel}</small>
            </div>
            <button class="icon-btn" title={copied === artifact.path ? "Copied" : "Copy the SHA-256"} aria-label="Copy the SHA-256" onclick={() => void checksum(artifact)}>
              {#if copied === artifact.path}<CircleCheck size={14} />{:else}<Copy size={14} />{/if}
            </button>
            <button class="icon-btn" title="Show in Explorer" aria-label="Show in Explorer" onclick={() => void reveal(artifact.path)}><FolderOpen size={14} /></button>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .build {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 9px;
    min-height: 0;
    padding: 14px 16px 16px;
    overflow-y: auto;
  }

  .bar,
  .cmd,
  .last,
  .artifacts {
    flex: none;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .bar :global(.select) {
    flex: 1;
    min-width: 0;
  }

  .bar .input {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .none {
    flex: 1;
    color: var(--text-3);
    font-size: 12.5px;
  }

  .cmd {
    overflow: hidden;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .last {
    color: var(--text-3);
    font-size: 11.5px;
  }

  .quiet {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 8px;
    flex: 1;
    min-height: 160px;
    color: var(--text-3);
    font-size: 12.5px;
  }

  .artifacts {
    display: grid;
    gap: 3px;
    padding: 8px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 10px;
    background: rgb(255 255 255 / 0.03);
  }

  .art-head {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 2px 6px 6px;
    color: var(--text-2);
    font-size: 12px;
  }

  .art-head span {
    color: var(--text-3);
  }

  .artifact {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 6px;
    border-radius: 7px;
  }

  .artifact:hover {
    background: var(--hover);
  }

  .art-text {
    display: grid;
    flex: 1;
    min-width: 0;
    gap: 1px;
  }

  .art-text strong {
    overflow: hidden;
    font-size: 12.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .art-text small {
    overflow: hidden;
    color: var(--text-3);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artifact .icon-btn {
    width: 28px;
    height: 28px;
  }

  .error {
    color: #ff9d9d;
    font-size: 12.5px;
  }
</style>
