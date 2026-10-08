<script lang="ts">
  // Releasing a project: the version (synced across its files), the notes
  // (AI from the commits), checks, then build → commit → push → tag →
  // release and upload, or the tag's GitHub Actions workflow.
  import { onMount } from "svelte";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Circle from "@lucide/svelte/icons/circle";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleDashed from "@lucide/svelte/icons/circle-dashed";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Rocket from "@lucide/svelte/icons/rocket";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Square from "@lucide/svelte/icons/square";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { confirm } from "../../../lib/confirm.svelte";
  import { toast } from "../../../lib/toast.svelte";
  import {
    githubReleasesApi as api,
    messageOf,
    problemOf,
    type Artifact,
    type Check,
    type EntryStatus,
    type Mode,
    type ReleaseInfo,
    type ReleaseOutcome,
    type StepId,
  } from "./api";
  import BuildLog from "./BuildLog.svelte";
  import {
    actionsSteps,
    formatBytes,
    formatRelative,
    githubReleases as gr,
    localSteps,
    runState,
    stepLabels,
    type ListItem,
  } from "./state.svelte";

  let { item, entry }: { item: ListItem; entry: EntryStatus } = $props();

  const repoId = $derived(item.repo.id);
  const s = $derived(gr.release(entry.id));
  const busy = $derived(gr.running(repoId));

  let info = $state<ReleaseInfo | null>(null);
  let loadError = $state<string | null>(null);
  let checks = $state<Check[] | null>(null);
  let checking = $state(false);
  let writing = $state(false);
  let commitsOpen = $state(false);
  let found = $state<Artifact[] | null>(null);
  let finding = $state(false);

  onMount(() => {
    void load();
  });

  async function load() {
    try {
      info = await api.releaseInfo(entry.id);
      loadError = null;
      if (!s.touched && !s.running) {
        s.version = info.next.patch ?? "1.0.0";
        s.mode = info.mode;
        s.build = !!info.buildCommand;
        s.notesFile = info.notesDir ? `${info.notesDir}/${s.version}.md` : null;
      }
    } catch (error) {
      loadError = messageOf(error);
    }
  }

  const tag = $derived(`${info?.tagPrefix ?? entry.tagPrefix}${s.version.trim()}`);
  const hasWorkflow = $derived(!!entry.releaseWorkflow);
  const steps = $derived<StepId[]>(s.mode === "actions" ? actionsSteps : localSteps);
  const versionFiles = $derived(info?.versions.files ?? []);
  const ownChanges = $derived(entry.changes);

  function pickVersion(version: string) {
    s.version = version;
    s.touched = true;
    if (info?.notesDir) s.notesFile = `${info.notesDir}/${version}.md`;
  }

  function setMode(mode: Mode) {
    s.mode = mode;
    s.touched = true;
    // The automatic choice is saved as none, so a new workflow is noticed.
    const auto: Mode = entry.releaseWorkflow ? "actions" : "local";
    void api.setEntry(entry.id, { releaseMode: mode === auto ? "" : mode }).catch(() => {});
  }

  async function toggleSkip(path: string) {
    const skip = new Set(entry.skipVersionFiles);
    if (skip.has(path)) skip.delete(path);
    else skip.add(path);
    try {
      await api.setEntry(entry.id, { skipVersionFiles: [...skip] });
      await gr.refresh(repoId);
      await load();
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  // The checks follow the form (a moment after the last change).
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const key = [s.version, s.mode, s.includeChanges, s.build, entry.changes, item.status?.branch.behind, item.status?.branch.head, gr.page?.account?.login].join("|");
    if (s.running || !info) return;
    clearTimeout(timer);
    void key;
    timer = setTimeout(() => void check(), 450);
    return () => clearTimeout(timer);
  });

  async function check() {
    if (!s.version.trim()) return;
    checking = true;
    try {
      checks = await api.preflight(entry.id, s.version.trim(), s.mode, s.includeChanges, s.mode === "local" && s.build);
    } catch (error) {
      checks = [{ id: "check", state: "fail", message: messageOf(error), fix: null }];
    } finally {
      checking = false;
    }
  }

  const failed = $derived(checks?.filter((c) => c.state === "fail") ?? []);

  function fix(check: Check) {
    switch (check.fix) {
      case "pull":
        void gr.pull(repoId);
        break;
      case "commit":
        gr.setTab("changes");
        break;
      case "connect":
        gr.settingsOpen = "account";
        break;
      case "build":
        gr.setTab("build");
        break;
    }
  }
  const fixLabels: Record<string, string> = { pull: "Pull", commit: "Open Changes", connect: "Connect", build: "Open Build" };

  function writeNotes(polish: boolean) {
    void gr.ai({
      run: (provider) => api.aiNotes(entry.id, s.version.trim(), provider, polish ? s.notes : null),
      apply: (notes) => {
        s.notes = notes.notes;
        if (notes.title && !s.title.trim()) s.title = notes.title;
        s.touched = true;
      },
      setBusy: (value) => (writing = value),
    });
  }

  async function findFiles() {
    finding = true;
    try {
      found = await api.artifacts(entry.id, 0);
      s.assets = found.filter((a) => a.kind === "installer" || a.kind === "update" || a.kind === "signature").map((a) => a.path);
    } catch (error) {
      toast.error(messageOf(error));
    } finally {
      finding = false;
    }
  }

  // Files from a build made here, offered when the release doesn't build.
  const lastBuild = $derived(gr.builds.get(entry.id));
  $effect(() => {
    if (!found && lastBuild?.artifacts.length && !lastBuild.running) {
      found = lastBuild.artifacts;
      if (s.assets === null) s.assets = lastBuild.selected.slice();
    }
  });

  function toggleAsset(path: string) {
    const list = new Set(s.assets ?? []);
    if (list.has(path)) list.delete(path);
    else list.add(path);
    s.assets = [...list];
  }

  const defaultTitle = $derived(entry.monorepo ? `${entry.name} v${s.version.trim()}` : `${item.repo.name} v${s.version.trim()}`);

  async function release() {
    if (s.running) return;
    const version = s.version.trim();
    const local = s.mode === "local";
    const parts = [
      `The version files become ${version}`,
      local && s.build ? "the project is built" : "",
      `a commit and the tag ${tag} are pushed to ${info?.branch ?? "the branch"}`,
      local
        ? `a ${s.draft ? "draft " : ""}release with ${local && !s.build ? (s.assets?.length ?? 0) : "the built"} files is ${s.draft ? "made" : "published"}`
        : `GitHub Actions (${entry.releaseWorkflow ?? "the workflow"}) builds and publishes it`,
    ].filter(Boolean);
    const ok = await confirm({
      title: `Release ${tag}?`,
      message: `${parts.join(", ")}. Before the push, a failure puts everything back.`,
      confirmLabel: "Release",
    });
    if (!ok) return;
    s.start();
    try {
      const outcome = await api.release(
        {
          entryId: entry.id,
          version,
          mode: s.mode,
          build: local && s.build,
          command: null,
          includeChanges: s.includeChanges,
          commitMessage: s.commitMessage.trim() || null,
          title: s.title.trim() || defaultTitle,
          notes: s.notes,
          assets: local && !s.build ? (s.assets ?? []) : null,
          draft: s.draft,
          prerelease: s.prerelease,
          makeLatest: s.makeLatest && !s.prerelease,
          notesFile: s.mode === "actions" && s.notesFile ? s.notesFile : null,
        },
        (event) => s.apply(event),
      );
      finish(outcome);
    } catch (error) {
      s.outcome = null;
      gr.showProblem(problemOf(error), repoId);
    } finally {
      s.running = false;
      await gr.refresh(repoId);
      void gr.loadRemote(repoId);
      void load();
    }
  }

  function finish(outcome: ReleaseOutcome) {
    s.outcome = outcome;
    if (outcome.ok) {
      s.touched = false;
      s.notes = "";
      s.title = "";
      s.commitMessage = "";
      s.assets = null;
      found = null;
      const url = outcome.release?.htmlUrl ?? outcome.run?.htmlUrl;
      toast.success(`${outcome.tag} is out.`, url ? { label: "Open", run: () => void openUrl(url) } : undefined);
    }
  }

  async function resume() {
    const resumeState = s.outcome?.resume;
    if (!resumeState || s.running) return;
    const before = s.outcome;
    s.start();
    try {
      const outcome = await api.resume(entry.id, resumeState, (event) => s.apply(event));
      finish({ ...outcome, version: outcome.version || before?.version || "", tag: outcome.tag || before?.tag || "" });
    } catch (error) {
      s.outcome = before;
      gr.showProblem(problemOf(error), repoId);
    } finally {
      s.running = false;
      void gr.loadRemote(repoId);
    }
  }

  async function watchAgain() {
    const commit = s.outcome?.commit;
    if (!commit || s.running) return;
    const before = s.outcome;
    s.running = true;
    try {
      const run = await api.watch(entry.id, commit, before?.tag ?? null, (event) => s.apply(event));
      if (before) s.outcome = { ...before, run, ok: runState(run) === "ok", problem: runState(run) === "ok" ? null : before.problem };
    } catch (error) {
      gr.showProblem(problemOf(error), repoId);
    } finally {
      s.running = false;
      void gr.loadRemote(repoId);
    }
  }

  async function cancel() {
    await api.cancel(repoId);
  }

  function stepIcon(id: StepId) {
    return s.steps[id]?.state ?? null;
  }

  const showForm = $derived(!s.running && !s.outcome);
  const uploadPercent = $derived(s.upload && s.upload.total ? Math.round((s.upload.sent / s.upload.total) * 100) : 0);
</script>

<div class="release">
  {#if loadError}
    <p class="error">{loadError}</p>
  {:else if !info}
    <div class="quiet"><LoaderCircle size={16} class="spin" /></div>
  {:else if showForm}
    <!-- Version -->
    <section>
      <h3>Version</h3>
      <div class="row wrap">
        {#each ["patch", "minor", "major"] as const as bump (bump)}
          {#if info.next[bump]}
            <button class="chip" class:active={s.version === info.next[bump]} onclick={() => pickVersion(info!.next[bump]!)}>
              {bump[0].toUpperCase() + bump.slice(1)} <span class="count">{info.next[bump]}</span>
            </button>
          {/if}
        {/each}
        <input class="input version" value={s.version} oninput={(e) => pickVersion((e.currentTarget as HTMLInputElement).value)} aria-label="Version" spellcheck="false" />
        <span class="tag-preview">tag <code>{tag}</code></span>
      </div>
      {#if info.lastTag}
        <p class="sub">
          Last: <code>{info.lastTag.name}</code>, {formatRelative(info.lastTag.date)}{info.lastTag.legacy ? " (a shared tag from before this app had its own)" : ""} ·
          <button class="link" onclick={() => (commitsOpen = !commitsOpen)}>{info.commits.length} {info.commits.length === 1 ? "commit" : "commits"} since</button>
        </p>
      {:else}
        <p class="sub">The first release of this project. <button class="link" onclick={() => (commitsOpen = !commitsOpen)}>{info.commits.length} commits</button></p>
      {/if}
      {#if commitsOpen}
        <ul class="commits">
          {#each info.commits.slice(0, 80) as c (c.sha)}<li><code>{c.sha.slice(0, 7)}</code> {c.subject}</li>{/each}
        </ul>
      {/if}
      {#if versionFiles.length}
        <div class="files">
          {#each versionFiles as file (file.path)}
            <label class="file" class:off={file.skipped}>
              <input type="checkbox" class="check" checked={!file.skipped} onchange={() => void toggleSkip(file.path)} />
              <code>{file.path}</code>
              <span class="from" class:bad={info.versions.mismatched.includes(file.path)}>{file.version}</span>
              {#if !file.skipped}<ArrowRight size={12} /><span class="to">{s.version || "?"}</span>{/if}
            </label>
          {/each}
          <div class="file"><span class="lock">+ lock files (package-lock.json, Cargo.lock) follow</span></div>
        </div>
      {:else}
        <p class="sub warn"><TriangleAlert size={12} /> No version file: only the tag carries the version.</p>
      {/if}
    </section>

    <!-- How -->
    <section>
      <h3>How</h3>
      <div class="modes">
        <button class="mode" class:active={s.mode === "local"} onclick={() => setMode("local")}>
          <strong>Build here, upload</strong>
          <span>MYLE builds, then makes the GitHub release and uploads the files.</span>
        </button>
        <button class="mode" class:active={s.mode === "actions"} disabled={!hasWorkflow} onclick={() => setMode("actions")} title={hasWorkflow ? undefined : "No workflow of this repository runs on a pushed tag"}>
          <strong>GitHub Actions</strong>
          <span>{hasWorkflow ? `Pushing ${tag} starts ${entry.releaseWorkflow}; it builds and publishes.` : "No tag-triggered workflow found."}</span>
        </button>
      </div>
      {#if s.mode === "local"}
        <label class="opt">
          <input type="checkbox" class="switch" bind:checked={s.build} onchange={() => (s.touched = true)} disabled={!info.buildCommand} />
          <span>Build first {#if info.buildCommand}<code>{info.buildCommand}</code>{:else}<em>(no build command: set one in Build)</em>{/if}</span>
        </label>
        {#if !s.build}
          <div class="assets">
            <div class="assets-head">
              <span>Files to attach</span>
              <button class="btn small ghost" disabled={finding} onclick={() => void findFiles()}>
                {#if finding}<LoaderCircle size={12} class="spin" />{:else}<RefreshCw size={12} />{/if} Find built files
              </button>
            </div>
            {#if found?.length}
              {#each found as a (a.path)}
                <label class="file">
                  <input type="checkbox" class="check" checked={s.assets?.includes(a.path) ?? false} onchange={() => toggleAsset(a.path)} />
                  <span class="name">{a.name}</span>
                  <small>{formatBytes(a.size)} · {formatRelative(a.modified)}</small>
                </label>
              {/each}
            {:else if found}
              <p class="sub">No built files were found. Build the project, or release without files.</p>
            {:else}
              <p class="sub">None yet: the release has no files unless you pick some.</p>
            {/if}
          </div>
        {:else}
          <p class="sub">The installers and update files the build makes are attached.</p>
        {/if}
      {:else if info.notesDir}
        <label class="opt">
          <input type="checkbox" class="switch" checked={!!s.notesFile} onchange={(e) => (s.notesFile = (e.currentTarget as HTMLInputElement).checked ? `${info!.notesDir}/${s.version.trim()}.md` : null)} />
          <span>Save the notes to <code>{info.notesDir}/{s.version.trim()}.md</code> (the workflow reads them)</span>
        </label>
      {/if}
      {#if ownChanges > 0}
        <label class="opt">
          <input type="checkbox" class="switch" bind:checked={s.includeChanges} />
          <span>Include the {ownChanges} uncommitted {ownChanges === 1 ? "change" : "changes"} in the release commit</span>
        </label>
      {/if}
    </section>

    <!-- Notes -->
    <section>
      <div class="notes-head">
        <h3>Release notes</h3>
        <div class="row">
          <button class="btn small ghost" disabled={writing} onclick={() => (gr.aiReady ? writeNotes(false) : (gr.settingsOpen = "ai"))} title="Write them from the commits since the last release">
            {#if writing}<LoaderCircle size={13} class="spin" />{:else}<Sparkles size={13} />{/if} Write with AI
          </button>
          {#if s.notes.trim()}
            <button class="btn small ghost" disabled={writing} onclick={() => (gr.aiReady ? writeNotes(true) : (gr.settingsOpen = "ai"))} title="Tidy what you wrote">
              <WandSparkles size={13} /> Polish
            </button>
          {/if}
        </div>
      </div>
      <input class="input" bind:value={s.title} placeholder={defaultTitle} aria-label="Release title" oninput={() => (s.touched = true)} />
      <textarea class="input notes" bind:value={s.notes} rows="9" placeholder="What changed (Markdown)" oninput={() => (s.touched = true)}></textarea>
      <div class="row wrap">
        <input class="input commit" bind:value={s.commitMessage} placeholder={entry.monorepo ? `${entry.name} ${s.version.trim()}` : `Release ${s.version.trim()}`} aria-label="Commit message" />
        {#if s.mode === "local"}
          <label class="opt inline"><input type="checkbox" class="check" bind:checked={s.draft} /> Draft</label>
        {/if}
        <label class="opt inline"><input type="checkbox" class="check" bind:checked={s.prerelease} /> Pre-release</label>
        <label class="opt inline"><input type="checkbox" class="check" bind:checked={s.makeLatest} disabled={s.prerelease} /> Latest</label>
      </div>
    </section>

    <!-- Checks -->
    <section>
      <div class="notes-head">
        <h3>Checks {#if checking}<LoaderCircle size={12} class="spin" />{/if}</h3>
        <button class="icon-btn" aria-label="Check again" title="Check again" onclick={() => void check()}><RefreshCw size={13} /></button>
      </div>
      {#if checks}
        <ul class="checks">
          {#each checks as c (c.id)}
            <li class="c-{c.state}">
              {#if c.state === "ok"}<CircleCheck size={14} />{:else if c.state === "warn"}<CircleAlert size={14} />{:else}<CircleX size={14} />{/if}
              <span>{c.message}</span>
              {#if c.fix && c.state !== "ok"}<button class="btn small" onclick={() => fix(c)}>{fixLabels[c.fix] ?? "Fix"}</button>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <div class="go">
      <button class="btn primary big" disabled={!!busy || checking || !checks || failed.length > 0 || !s.version.trim()} onclick={() => void release()}>
        <Rocket size={16} /> Release {tag}
      </button>
      {#if failed.length}<span class="sub">Fix the checks above first.</span>{/if}
    </div>
  {:else}
    <!-- The run -->
    <section class="run">
      <div class="notes-head">
        <h3>{s.running ? "Releasing" : s.outcome?.ok ? "Released" : s.outcome?.cancelled ? "Cancelled" : "Stopped"} {s.outcome?.tag ?? tag}</h3>
        {#if s.running}
          <button class="btn small danger" onclick={() => void cancel()}><Square size={12} /> Cancel</button>
        {/if}
      </div>
      <ol class="steps">
        {#each steps as id (id)}
          {@const state = stepIcon(id)}
          <li class="s-{state ?? 'waiting'}">
            {#if state === "running"}<LoaderCircle size={15} class="spin" />{:else if state === "done"}<CircleCheck size={15} />{:else if state === "failed"}<CircleX size={15} />{:else if state === "skipped"}<CircleDashed size={15} />{:else}<Circle size={15} />{/if}
            <span class="label">{stepLabels[id]}</span>
            {#if s.steps[id]?.message}<span class="msg">{s.steps[id]?.message}</span>{/if}
          </li>
          {#if id === "upload" && s.upload && state === "running"}
            <li class="upload">
              <span>{s.upload.name} ({s.upload.index + 1}/{s.upload.count})</span>
              <div class="progress"><div style="width: {uploadPercent}%"></div></div>
              <em>{formatBytes(s.upload.sent)} / {formatBytes(s.upload.total)}</em>
            </li>
          {/if}
          {#if id === "workflow" && (s.run || s.jobs.length)}
            <li class="jobs">
              {#if s.run}
                <button class="link" onclick={() => void openUrl(s.run!.htmlUrl)}>{s.run.name ?? "Workflow run"} <ExternalLink size={11} /></button>
              {/if}
              {#each s.jobs as job (job.id)}
                {@const js = runState(job)}
                <div class="job j-{js}">
                  {#if js === "running"}<LoaderCircle size={13} class="spin" />{:else if js === "ok"}<CircleCheck size={13} />{:else if js === "failed"}<CircleX size={13} />{:else}<CircleDashed size={13} />{/if}
                  <strong>{job.name}</strong>
                  {#if js === "running"}
                    {@const current = job.steps.find((step) => step.status === "in_progress")}
                    {#if current}<span>{current.name}</span>{/if}
                  {/if}
                  {#if js === "failed"}
                    {@const bad = job.steps.find((step) => step.conclusion === "failure")}
                    {#if bad}<span>{bad.name}</span>{/if}
                  {/if}
                </div>
              {/each}
            </li>
          {/if}
        {/each}
      </ol>

      {#if s.steps.build && s.mode === "local" && (s.steps.build.state === "running" || s.steps.build.state === "failed")}
        <div class="build-log"><BuildLog session={s.buildLog} entryId={entry.id} compact /></div>
      {/if}

      {#if s.log.length}
        <details class="log">
          <summary>Details</summary>
          <pre class="selectable">{s.log.join("\n")}</pre>
        </details>
      {/if}

      {#if s.outcome && !s.running}
        {#if s.outcome.ok}
          <div class="result ok">
            <CircleCheck size={18} />
            <span><strong>{s.outcome.tag}</strong> is {s.outcome.release?.draft ? "a draft on GitHub" : "published"}.</span>
            {#if s.outcome.release}<button class="btn small" onclick={() => void openUrl(s.outcome!.release!.htmlUrl)}><ExternalLink size={13} /> Open the release</button>{/if}
            {#if s.outcome.run}<button class="btn small" onclick={() => void openUrl(s.outcome!.run!.htmlUrl)}><ExternalLink size={13} /> Open the run</button>{/if}
          </div>
        {:else}
          <div class="result bad">
            <CircleX size={18} />
            <span>
              {s.outcome.problem?.message ?? "The release stopped."}
              {#if s.outcome.rolledBack}<em>Nothing was pushed: the version files and commit were put back.</em>{/if}
            </span>
            {#if s.outcome.resume}
              <button class="btn small primary" disabled={!!busy} onclick={() => void resume()}><RotateCcw size={13} /> Resume</button>
            {/if}
            {#if s.outcome.commit && s.mode === "actions" && !s.outcome.resume}
              <button class="btn small" disabled={!!busy} onclick={() => void watchAgain()}><RefreshCw size={13} /> Follow the run</button>
            {/if}
            {#if s.outcome.run}<button class="btn small" onclick={() => void openUrl(s.outcome!.run!.htmlUrl)}><ExternalLink size={13} /> Open the run</button>{/if}
          </div>
        {/if}
        <button class="btn small ghost new" onclick={() => (s.outcome = null)}><ChevronDown size={13} /> {s.outcome.ok ? "Start the next release" : "Back to the form"}</button>
      {/if}
    </section>
  {/if}
</div>

<style>
  .release {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
    padding: 14px 16px 18px;
    overflow: auto;
    scrollbar-width: thin;
  }

  section {
    display: grid;
    gap: 9px;
    padding: 12px 14px;
    border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 12px;
    background: rgb(255 255 255 / 0.025);
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    color: var(--text-1);
    font-size: 13px;
    font-weight: 650;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .wrap {
    flex-wrap: wrap;
  }

  .version {
    width: 130px;
    height: 32px;
    font-family: var(--font-mono);
    font-size: 12.5px;
  }

  .tag-preview {
    color: var(--text-3);
    font-size: 11.5px;
  }

  code {
    font-family: var(--font-mono);
    font-size: 11.3px;
  }

  .tag-preview code {
    color: #c3c9f7;
  }

  .sub {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    margin: 0;
    color: var(--text-3);
    font-size: 11.8px;
  }

  .sub.warn {
    color: #ffd08a;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    color: #b7befa;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .commits {
    display: grid;
    gap: 2px;
    max-height: 180px;
    margin: 0;
    padding: 8px 10px;
    overflow: auto;
    border-radius: 8px;
    background: rgb(0 0 0 / 0.2);
    color: var(--text-2);
    font-size: 11.8px;
    list-style: none;
  }

  .commits code {
    color: var(--text-3);
  }

  .files {
    display: grid;
    gap: 2px;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 7px;
    font-size: 12px;
  }

  .file:hover {
    background: var(--hover);
  }

  .file.off {
    opacity: 0.55;
  }

  .file code {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-2);
    text-overflow: ellipsis;
  }

  .from {
    color: var(--text-3);
    font-family: var(--font-mono);
    font-size: 11.3px;
  }

  .from.bad {
    color: #ffb84d;
  }

  .to {
    color: #8fe6bf;
    font-family: var(--font-mono);
    font-size: 11.3px;
  }

  .lock {
    padding-left: 26px;
    color: var(--text-3);
    font-size: 11px;
  }

  .file .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file small {
    color: var(--text-3);
    font-size: 11px;
  }

  .modes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .mode {
    display: grid;
    gap: 3px;
    padding: 10px 12px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 10px;
    text-align: left;
  }

  .mode:hover:not(:disabled) {
    background: var(--hover);
  }

  .mode.active {
    border-color: rgb(var(--accent-rgb) / 0.5);
    background: rgb(var(--accent-rgb) / 0.12);
  }

  .mode:disabled {
    opacity: 0.5;
  }

  .mode strong {
    font-size: 12.8px;
  }

  .mode span {
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.4;
  }

  .opt {
    display: flex;
    align-items: center;
    gap: 9px;
    color: var(--text-2);
    font-size: 12.3px;
  }

  .opt code {
    margin-left: 4px;
    color: var(--text-3);
  }

  .opt em {
    color: var(--text-3);
    font-style: normal;
  }

  .opt.inline {
    gap: 6px;
  }

  .assets {
    display: grid;
    gap: 3px;
    padding: 8px;
    border-radius: 9px;
    background: rgb(0 0 0 / 0.16);
  }

  .assets-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px 4px;
    color: var(--text-2);
    font-size: 12px;
  }

  .notes-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .notes {
    min-height: 150px;
    resize: vertical;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
  }

  .commit {
    flex: 1;
    min-width: 200px;
  }

  .checks {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .checks li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 2px;
    font-size: 12.3px;
  }

  .checks li span {
    flex: 1;
    color: var(--text-2);
  }

  .c-ok {
    color: #3ecf8e;
  }

  .c-warn {
    color: #ffb84d;
  }

  .c-fail {
    color: #ff7b7b;
  }

  .go {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .big {
    height: 42px;
    padding: 0 20px;
    font-size: 13.5px;
  }

  .steps {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 2px;
    color: var(--text-3);
    font-size: 12.5px;
  }

  .steps .label {
    min-width: 110px;
    color: var(--text-2);
  }

  .steps .msg {
    min-width: 0;
    overflow: hidden;
    color: var(--text-3);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .s-running {
    color: #9fd3ff !important;
  }

  .s-running .label {
    color: var(--text-1) !important;
  }

  .s-done {
    color: #3ecf8e !important;
  }

  .s-failed {
    color: #ff7b7b !important;
  }

  li.upload,
  li.jobs {
    display: grid !important;
    gap: 5px;
    padding: 2px 0 6px 25px !important;
  }

  .upload .progress {
    height: 5px;
    overflow: hidden;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.08);
  }

  .upload .progress div {
    height: 100%;
    background: var(--accent-grad);
    transition: width 0.25s;
  }

  .upload em {
    font-style: normal;
    font-size: 11px;
  }

  .job {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
  }

  .job strong {
    color: var(--text-2);
    font-weight: 550;
  }

  .job span {
    color: var(--text-3);
    font-size: 11.3px;
  }

  .j-ok {
    color: #3ecf8e;
  }

  .j-failed {
    color: #ff7b7b;
  }

  .j-running {
    color: #9fd3ff;
  }

  .build-log {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  details.log summary {
    color: var(--text-3);
    font-size: 11.8px;
    cursor: pointer;
  }

  details.log pre {
    max-height: 160px;
    margin: 6px 0 0;
    padding: 8px 10px;
    overflow: auto;
    border-radius: 8px;
    background: rgb(0 0 0 / 0.25);
    color: var(--text-2);
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: pre-wrap;
  }

  .result {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 11px 13px;
    border-radius: 10px;
    font-size: 12.5px;
  }

  .result span {
    flex: 1;
    color: var(--text-1);
  }

  .result em {
    display: block;
    color: var(--text-3);
    font-style: normal;
  }

  .result.ok {
    border: 1px solid rgb(62 207 142 / 0.3);
    background: rgb(62 207 142 / 0.08);
    color: #3ecf8e;
  }

  .result.bad {
    border: 1px solid rgb(229 72 77 / 0.3);
    background: rgb(229 72 77 / 0.08);
    color: #ff7b7b;
  }

  .new {
    justify-self: start;
  }

  .quiet {
    display: grid;
    place-items: center;
    min-height: 160px;
    color: var(--text-3);
  }

  .error {
    color: #ff9d9d;
    font-size: 12.5px;
  }

  @container releases (max-width: 900px) {
    .modes {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
