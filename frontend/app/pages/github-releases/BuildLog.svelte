<script lang="ts">
  // A build's progress, its problems (each opens in the editor) and its log.
  import { tick } from "svelte";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { toast } from "../../../lib/toast.svelte";
  import { githubReleasesApi as api, messageOf, type Diagnostic } from "./api";
  import { duration, type BuildSession } from "./state.svelte";

  let {
    session,
    entryId,
    problemsOpen = $bindable(true),
    compact = false,
  }: { session: BuildSession; entryId: string; problemsOpen?: boolean; compact?: boolean } = $props();

  /** Lines drawn at once; the whole log is in its file. */
  const SHOWN = 3000;
  let showAll = $state(false);
  let logEl = $state<HTMLDivElement>();
  let follow = $state(true);

  const shown = $derived(showAll ? session.lines : session.lines.slice(-SHOWN));
  const hiddenAbove = $derived(session.lines.length - shown.length);
  const errors = $derived(session.diagnostics.filter((d) => d.severity === "error"));
  const warnings = $derived(session.diagnostics.filter((d) => d.severity === "warning"));

  /** The log follows the newest line unless the user scrolled away. */
  $effect(() => {
    const count = session.lines.length;
    if (!follow || !count) return;
    void tick().then(() => logEl && (logEl.scrollTop = logEl.scrollHeight));
  });

  function onLogScroll() {
    if (!logEl) return;
    follow = logEl.scrollTop + logEl.clientHeight >= logEl.scrollHeight - 40;
  }

  async function openProblem(d: Diagnostic) {
    if (!d.file) return;
    try {
      await api.openInEditor(entryId, d.file, d.line, d.column);
    } catch (e) {
      toast.error(messageOf(e));
    }
  }

  async function revealLog() {
    try {
      if (session.logPath) await api.reveal(entryId, session.logPath);
    } catch (e) {
      toast.error(messageOf(e));
    }
  }
</script>

<div class="progress" role="progressbar" aria-valuenow={Math.round(session.percent)} aria-valuemin="0" aria-valuemax="100">
  <div
    class="bar-fill"
    class:done={session.outcome?.ok}
    class:failed={session.outcome && !session.outcome.ok && !session.outcome.cancelled}
    style="width: {Math.max(session.percent, session.running ? 2 : 0)}%"
  ></div>
</div>
<div class="status-line">
  {#if session.running}
    <LoaderCircle size={13} class="spin" />
    <span>{session.stage ?? (session.install ? "Installing dependencies" : "Building")}…</span>
    <em>{session.percent.toFixed(0)}%{session.estimated ? " estimated" : ""}</em>
  {:else if session.outcome?.cancelled}
    <CircleX size={13} /> <span>Cancelled.</span>
  {:else if session.outcome?.ok}
    <CircleCheck size={13} class="ok" />
    <span>
      {session.install ? "Installed" : "Built"} in {duration(session.outcome.durationMs)}{session.outcome.warnings
        ? ` with ${session.outcome.warnings} ${session.outcome.warnings === 1 ? "warning" : "warnings"}`
        : ""}.
    </span>
  {:else if session.outcome}
    <CircleX size={13} class="bad" />
    <span>{session.outcome.failure ?? `The build failed${session.outcome.exitCode !== null ? ` (exit ${session.outcome.exitCode})` : ""}.`}</span>
  {/if}
  {#if session.diagnostics.length}
    <button class="problems-toggle" class:bad={errors.length > 0} onclick={() => (problemsOpen = !problemsOpen)} aria-expanded={problemsOpen}>
      {errors.length ? `${errors.length} ${errors.length === 1 ? "error" : "errors"}` : ""}{errors.length && warnings.length ? " · " : ""}{warnings.length
        ? `${warnings.length} ${warnings.length === 1 ? "warning" : "warnings"}`
        : ""}
    </button>
  {/if}
</div>

{#if problemsOpen && session.diagnostics.length}
  <div class="problems" aria-label="Problems">
    {#each session.diagnostics as d, i (i)}
      <button class="problem p-{d.severity}" disabled={!d.file} title={d.file ? "Open in the editor" : undefined} onclick={() => void openProblem(d)}>
        {#if d.severity === "error"}<CircleX size={13} />{:else}<TriangleAlert size={13} />{/if}
        <span class="msg" title={d.message}>{d.message}</span>
        {#if d.file}
          <span class="where">{d.file}{d.line ? `:${d.line}${d.column ? `:${d.column}` : ""}` : ""}</span>
          <ExternalLink size={11} />
        {/if}
        {#if d.code}<span class="code">{d.code}</span>{/if}
      </button>
    {/each}
  </div>
{/if}

<div class="log" class:compact bind:this={logEl} onscroll={onLogScroll} role="log" aria-label="Build output">
  {#if session.dropped || hiddenAbove}
    <div class="dropped">
      {#if hiddenAbove}
        <button class="link" onclick={() => (showAll = true)}>Show {hiddenAbove.toLocaleString()} earlier lines</button>
      {/if}
      {#if session.dropped}… {session.dropped.toLocaleString()} more are only in the log file{/if}
    </div>
  {/if}
  {#each shown as line (line.index)}
    <div class="line" class:err={line.err} class:error={line.severity === "error"} class:warning={line.severity === "warning"}>{line.text}</div>
  {/each}
</div>
<div class="log-foot">
  {#if session.logPath}
    <button class="btn small ghost" onclick={() => void revealLog()}><FolderOpen size={13} /> Log file</button>
  {/if}
  {#if !follow && session.running}
    <button class="btn small ghost" onclick={() => ((follow = true), logEl && (logEl.scrollTop = logEl.scrollHeight))}>Follow the output</button>
  {/if}
</div>

<style>
  .progress {
    height: 6px;
    overflow: hidden;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.07);
  }

  .bar-fill {
    height: 100%;
    border-radius: inherit;
    background: var(--accent-grad);
    transition: width 0.4s var(--ease-out);
  }

  .bar-fill.done {
    background: #3ecf8e;
  }

  .bar-fill.failed {
    background: #e5484d;
  }

  .status-line {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--text-2);
    font-size: 12.3px;
  }

  .status-line em {
    color: var(--text-3);
    font-style: normal;
  }

  .status-line span {
    min-width: 0;
  }

  .problems-toggle {
    margin-left: auto;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgb(255 196 92 / 0.14);
    color: #ffd08a;
    font-size: 11.5px;
  }

  .problems-toggle.bad {
    background: rgb(229 72 77 / 0.16);
    color: #ff9d9d;
  }

  .problems {
    display: grid;
    gap: 3px;
    max-height: 220px;
    padding: 6px;
    overflow: auto;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 10px;
    background: rgb(0 0 0 / 0.18);
    scrollbar-width: thin;
  }

  .problem {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 7px;
    text-align: left;
    font-size: 12px;
  }

  .problem:hover:not(:disabled) {
    background: var(--hover);
  }

  .problem:disabled {
    cursor: default;
  }

  .p-error {
    color: #ff9d9d;
  }

  .p-warning {
    color: #ffd08a;
  }

  .msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-1);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .where,
  .code {
    flex: none;
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 10.5px;
  }

  .log {
    flex: 1;
    min-height: 160px;
    padding: 10px 12px;
    overflow: auto;
    border-radius: 10px;
    background: rgb(0 0 0 / 0.35);
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.55;
    scrollbar-width: thin;
  }

  .line {
    color: var(--text-2);
    white-space: pre-wrap;
    word-break: break-all;
  }

  .line.err {
    color: #d8c8c8;
  }

  .line.error {
    color: #ff9d9d;
  }

  .line.warning {
    color: #ffd08a;
  }

  .dropped {
    margin-bottom: 6px;
    color: var(--text-3);
    font-style: italic;
  }

  .log-foot {
    display: flex;
    gap: 8px;
  }

  .log.compact {
    flex: none;
    height: 240px;
  }

  .link {
    padding: 0;
    color: #b7befa;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .status-line :global(.ok) {
    color: #3ecf8e;
  }

  .status-line :global(.bad) {
    color: #ff7b7b;
  }
</style>
