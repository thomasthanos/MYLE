<script lang="ts">
  // Releasing a project: the version (synced across its files), the notes
  // (AI from the commits), checks, then build → commit → push → tag →
  // release and upload, or the tag's GitHub Actions workflow.
  import { onMount, untrack } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Circle from "@lucide/svelte/icons/circle";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleDashed from "@lucide/svelte/icons/circle-dashed";
  import CircleSlash from "@lucide/svelte/icons/circle-slash";
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
  import Popover from "../../../lib/components/Popover.svelte";
  import Select, { type SelectOption } from "../../../lib/components/Select.svelte";
  import {
    actionsSteps,
    failureKey,
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
  // The panel is made again for another project, so the id stays.
  const s = gr.release(untrack(() => entry.id));
  const busy = $derived(gr.running(repoId));
  let info = $state<ReleaseInfo | null>(null);
  let loadError = $state<string | null>(null);
  let checks = $state<Check[] | null>(null);
  let checking = $state(false);
  let writing = $state(false);
  let commitsOpen = $state(false);
  let found = $state<Artifact[] | null>(null);
  let finding = $state(false);
  let stopping = $state(false);
  let branches = $state<string[] | null>(null);
  const branchOptions = $derived<SelectOption<string>[]>([
    { value: "", label: "None" },
    ...(branches ?? []).map((b) => ({ value: b, label: b })),
  ]);
  const isProtectedBranch = $derived(
    ["main", "master", "old"].includes(s.mergeBranch.trim().toLowerCase())
  );
  onMount(() => {
    void load();
    void loadBranches();
  });
  async function loadBranches() {
    try {
      branches = await api.branches(repoId);
      if (s.mergeBranch && !branches.includes(s.mergeBranch)) s.mergeBranch = "";
    } catch {
      branches = [];
    }
  }
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
  const steps = $derived<StepId[]>([
    ...(s.mergeBranch ? (["merge"] as StepId[]) : []),
    ...(s.mode === "actions" ? actionsSteps : localSteps),
  ]);
  const versionFiles = $derived(info?.versions.files ?? []);
  /** The version files the release changes (not the ones skipped). */
  const activeFiles = $derived(versionFiles.filter((file) => !file.skipped).length);
  const mismatchedFiles = $derived(info?.versions.mismatched ?? []);
  const mismatched = $derived(mismatchedFiles.length > 0);
  const commits = $derived(info?.commits ?? []);
  const lastTag = $derived(info?.lastTag ?? null);
  const ownChanges = $derived(entry.changes);
  function pickVersion(version: string) {
    s.version = version;
    s.touched = true;
    if (info?.notesDir && s.notesFile !== null) s.notesFile = `${info.notesDir}/${version.trim()}.md`;
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
  // The checks run again as the form changes; only the newest answer counts
  // (an older one finishing later would show checks for another version).
  let checkSeq = 0;
  async function check() {
    if (!s.version.trim()) return;
    const seq = ++checkSeq;
    checking = true;
    try {
      const result = await api.preflight(entry.id, s.version.trim(), s.mode, s.includeChanges, s.mode === "local" && s.build);
      if (seq === checkSeq) checks = result;
    } catch (error) {
      if (seq === checkSeq) checks = [{ id: "check", state: "fail", message: messageOf(error), fix: null }];
    } finally {
      if (seq === checkSeq) checking = false;
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
  // The GitHub repository's name ("MYLE"), not the folder's ("Make_Your_Life_Easier.A.E").
  const repoName = $derived(item.status?.remote?.repo ?? item.repo.name);
  const defaultTitle = $derived(entry.monorepo ? `${entry.name} ${tag}` : `${repoName} ${tag}`);
  async function release() {
    if (s.running) return;
    const version = s.version.trim();
    const local = s.mode === "local";
    const parts = [
      s.mergeBranch ? `${s.mergeBranch} is merged into ${info?.branch ?? "the branch"}` : "",
      `The version files become ${version}`,
      local && s.build ? "the project is built" : "",
      `a commit and the tag ${tag} are pushed to ${info?.branch ?? "the branch"}`,
      local
        ? `a ${s.draft ? "draft " : ""}release with ${local && !s.build ? (s.assets?.length ?? 0) : "the built"} files is ${s.draft ? "made" : "published"}`
        : `GitHub Actions (${entry.releaseWorkflow ?? "the workflow"}) builds ${s.target === "windows" ? "the Windows .exe only" : "everything"} and publishes it`,
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
          mergeBranch: s.mergeBranch || null,
          deleteMergedBranch: s.mergeBranch ? s.deleteMergedBranch && !isProtectedBranch : false,
          target: s.mode === "actions" ? s.target : null,
        },
        (event) => s.apply(event),
      );
      finish(outcome);
    } catch (error) {
      s.outcome = null;
      gr.showProblem(problemOf(error), repoId);
    } finally {
      stopping = false;
      s.running = false;
      await gr.refresh(repoId);
      await gr.loadRemote(repoId);
      void load();
    }
  }
  function finish(outcome: ReleaseOutcome) {
    stopping = false;
    s.outcome = outcome;
    if (outcome.ok) {
      s.touched = false;
      s.notes = "";
      s.title = "";
      s.commitMessage = "";
      s.assets = null;
      found = null;
      void loadBranches();
      const url = outcome.release?.htmlUrl ?? outcome.run?.htmlUrl;
      toast.success(`${outcome.tag} is out.`, url ? { label: "Open", run: () => void openUrl(url) } : undefined);
    }
  }
  async function resume() {
    const resumeState = s.outcome?.resume;
    if (!resumeState || s.running) return;
    const before = s.outcome;
    s.start();
    stopping = false;
    try {
      const outcome = await api.resume(entry.id, resumeState, (event) => s.apply(event));
      finish({ ...outcome, version: outcome.version || before?.version || "", tag: outcome.tag || before?.tag || "" });
    } catch (error) {
      s.outcome = before;
      gr.showProblem(problemOf(error), repoId);
    } finally {
      stopping = false;
      s.running = false;
      void gr.loadRemote(repoId);
    }
  }
  async function watchAgain() {
    const commit = s.outcome?.commit;
    if (!commit || s.running) return;
    const before = s.outcome;
    s.running = true;
    stopping = false;
    try {
      const run = await api.watch(entry.id, commit, before?.tag ?? null, (event) => s.apply(event));
      if (before) s.outcome = { ...before, run, ok: runState(run) === "ok", problem: runState(run) === "ok" ? null : before.problem };
    } catch (error) {
      gr.showProblem(problemOf(error), repoId);
    } finally {
      stopping = false;
      s.running = false;
      void gr.loadRemote(repoId);
    }
  }
  /** Following the workflow on GitHub: the tag is out, so stopping means
   *  either no longer following, or cancelling the run there. */
  const following = $derived(s.mode === "actions" && s.steps.workflow?.state === "running");
  /** The failures' box: brought into view when a failure is told. */
  let failuresBox = $state<HTMLElement>();
  let failuresSeen = 0;
  $effect(() => {
    const count = s.failures.length;
    if (count > failuresSeen) failuresBox?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    failuresSeen = count;
  });
  async function stopFollowing() {
    if (stopping) return;
    stopping = true;
    try {
      await api.cancel(repoId);
    } catch (error) {
      toast.error(messageOf(error));
      stopping = false;
    }
  }
  async function cancelOnGithub() {
    const run = s.run;
    if (stopping || !run?.id) return;
    const ok = await confirm({
      title: "Cancel the run on GitHub?",
      message: `${run.name ?? "The workflow"} stops on GitHub, and ${tag} is not published by it. The tag stays pushed.`,
      confirmLabel: "Cancel the run",
      danger: true,
    });
    if (!ok) return;
    stopping = true;
    try {
      await api.cancelCi(repoId, run.id);
      await api.cancel(repoId);
    } catch (error) {
      toast.error(`The run goes on on GitHub: ${messageOf(error)}`, { label: "Open", run: () => void openUrl(run.htmlUrl) });
      stopping = false;
    }
  }
  async function cancel() {
    if (stopping) return;
    stopping = true;
    try {
      await api.cancel(repoId);
      const run = s.run;
      if (run?.id) {
        void api.cancelCi(repoId, run.id).catch((error) =>
          toast.error(`The run goes on on GitHub: ${messageOf(error)}`, {
            label: "Open",
            run: () => void openUrl(run.htmlUrl),
          }),
        );
      }
    } catch (error) {
      toast.error(messageOf(error));
      stopping = false;
    }
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
    <!-- Version: one row; the files and commits behind its summary -->
    <section class="line">
      <div class="row">
        <h3 class="label">Version</h3>
        {#each ["patch", "minor", "major"] as const as bump (bump)}
          {#if info.next[bump]}
            <button class="bump" class:active={s.version === info.next[bump]} onclick={() => pickVersion(info!.next[bump]!)} title={`${bump[0].toUpperCase() + bump.slice(1)} release`}>
              {info.next[bump]}
            </button>
          {/if}
        {/each}
        <input class="input version" value={s.version} oninput={(e) => pickVersion((e.currentTarget as HTMLInputElement).value)} aria-label="Version" spellcheck="false" />
        <span class="tag-preview">tag <code>{tag}</code></span>
        <span class="spacer"></span>
        <Popover align="end" bind:open={commitsOpen}>
          {#snippet trigger({ toggle, open })}
            <button class="summary" class:warn={!versionFiles.length || mismatched} aria-expanded={open} aria-haspopup="dialog" onclick={toggle}
              title="{activeFiles} version {activeFiles === 1 ? 'file' : 'files'} · {commits.length} {commits.length === 1 ? 'commit' : 'commits'} since {lastTag?.name ?? 'the start'}">
              {#if !versionFiles.length}<TriangleAlert size={12} /> No file
              {:else}{#if mismatched}<CircleAlert size={12} />{/if}{activeFiles} {activeFiles === 1 ? "file" : "files"}{/if}
              <span class="dim">· {commits.length}</span>
              <ChevronDown size={12} />
            </button>
          {/snippet}
          {#snippet children()}
            <div class="pop" role="dialog" aria-label="Version files and commits">
              <p class="sub">
                {#if lastTag}Last: <code>{lastTag.name}</code>, {formatRelative(lastTag.date)}{lastTag.legacy ? " (a shared tag from before this app had its own)" : ""}
                {:else}The first release of this project.{/if}
              </p>
              {#if versionFiles.length}
                <div class="files">
                  {#each versionFiles as file (file.path)}
                    <label class="file" class:off={file.skipped}>
                      <input type="checkbox" class="check" checked={!file.skipped} onchange={() => void toggleSkip(file.path)} />
                      <code>{file.path}</code>
                      <span class="from" class:bad={mismatchedFiles.includes(file.path)}>{file.version}</span>
                      {#if !file.skipped}<ArrowRight size={12} /><span class="to">{s.version || "?"}</span>{/if}
                    </label>
                  {/each}
                  <div class="file"><span class="lock">+ lock files (package-lock.json, Cargo.lock) follow</span></div>
                </div>
              {:else}
                <p class="sub warn"><TriangleAlert size={12} /> No version file: only the tag carries the version.</p>
              {/if}
              <div class="pop-label">{commits.length} {commits.length === 1 ? "commit" : "commits"} since {lastTag?.name ?? "the start"}</div>
              <ul class="commits">
                {#each commits.slice(0, 80) as c (c.sha)}<li><code>{c.sha.slice(0, 7)}</code> {c.subject}</li>{/each}
              </ul>
            </div>
          {/snippet}
        </Popover>
      </div>
    </section>
    <!-- How: the choices on one row, their options on one line under them -->
    <section class="line">
      <div class="row wrap how">
        <span class="seg" role="radiogroup" aria-label="How to release">
          <button role="radio" aria-checked={s.mode === "local"} class:active={s.mode === "local"} onclick={() => setMode("local")}
            title="MYLE builds, then makes the GitHub release and uploads the files">Build here</button>
          <button role="radio" aria-checked={s.mode === "actions"} class:active={s.mode === "actions"} disabled={!hasWorkflow} onclick={() => setMode("actions")}
            title={hasWorkflow ? `Pushing ${tag} starts ${entry.releaseWorkflow}; it builds and publishes` : "No workflow of this repository runs on a pushed tag"}>GitHub Actions</button>
        </span>
        {#if s.mode === "actions"}
          <span class="seg" role="radiogroup" aria-label="What the workflows build">
            <button role="radio" aria-checked={s.target === "windows"} class:active={s.target === "windows"} onclick={() => (s.target = "windows")} title="The tag says Build: windows; mobile.yml skips the phone apps">Windows .exe</button>
            <button role="radio" aria-checked={s.target === "full"} class:active={s.target === "full"} onclick={() => (s.target = "full")} title="Every workflow of the tag builds: Windows, phones and the rest">Full release</button>
          </span>
        {/if}
        <span class="field branch-field">
          <span>Merge</span>
          <Select bind:value={s.mergeBranch} options={branchOptions} disabled={branches === null} ariaLabel="Merge a branch into {info.branch ?? 'the branch'} first" size="sm" minWidth="100px" />
          <button class="icon-btn" title="Fetch the branches again" aria-label="Fetch the branches again" onclick={() => void loadBranches()}><RefreshCw size={12} /></button>
        </span>
      </div>
      <div class="row wrap opts">
        {#if s.mergeBranch}
          {#if isProtectedBranch}
            <span class="sub"><code>{s.mergeBranch}</code> is kept (a protected branch).</span>
          {:else}
            <label class="opt" title="origin/{s.mergeBranch} is merged into {info.branch ?? 'the branch'} first; a conflict stops the release and changes nothing">
              <input type="checkbox" class="switch" bind:checked={s.deleteMergedBranch} />
              <span>Delete <code>{s.mergeBranch}</code> once released</span>
            </label>
          {/if}
        {/if}
        {#if s.mode === "local"}
          <label class="opt" title={info.buildCommand ? info.buildSteps.join(" → ") : "No build command: set one in Build"}>
            <input type="checkbox" class="switch" bind:checked={s.build} onchange={() => (s.touched = true)} disabled={!info.buildCommand} />
            <span>Build first {#if info.buildCommand}<code>{info.buildCommand}</code>{:else}<em>(none set)</em>{/if}</span>
          </label>
          {#if !s.build}
            <Popover align="start">
              {#snippet trigger({ toggle, open })}
                <button class="summary" aria-expanded={open} onclick={toggle}>{s.assets?.length ?? 0} {s.assets?.length === 1 ? "file" : "files"} to attach <ChevronDown size={12} /></button>
              {/snippet}
              {#snippet children()}
                <div class="pop assets">
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
              {/snippet}
            </Popover>
          {/if}
        {:else if info.notesDir}
          <label class="opt" title="Saved to {info.notesDir}/{s.version.trim()}.md in the release commit; the workflow publishes them">
            <input type="checkbox" class="switch" checked={!!s.notesFile} onchange={(e) => (s.notesFile = (e.currentTarget as HTMLInputElement).checked ? `${info!.notesDir}/${s.version.trim()}.md` : null)} />
            <span>Notes file</span>
          </label>
        {/if}
        {#if ownChanges > 0}
          <label class="opt" title="Commit the {ownChanges} uncommitted {ownChanges === 1 ? 'change' : 'changes'} with the release">
            <input type="checkbox" class="switch" bind:checked={s.includeChanges} />
            <span>+{ownChanges} uncommitted</span>
          </label>
        {/if}
      </div>
      {#if s.mode === "local" && s.build && info.fullBuild}
        <p class="sub warn"><TriangleAlert size={12} /> Only part of the project's build; the full one is <code>{info.fullBuild}</code>. <button class="link" onclick={() => gr.setTab("build")}>Change it in Build</button></p>
      {/if}
    </section>
    <!-- Notes: title and commit message on one row; the notes take the height left -->
    <section class="notes-section">
      <div class="row">
        <input class="input title" bind:value={s.title} placeholder={defaultTitle} aria-label="Release title" oninput={() => (s.touched = true)} />
        <input class="input commit" bind:value={s.commitMessage} placeholder={entry.monorepo ? `${entry.name} ${s.version.trim()}` : `Release ${s.version.trim()}`} aria-label="Commit message" />
        <button class="btn small ghost" disabled={writing} onclick={() => (gr.aiReady ? writeNotes(false) : (gr.settingsOpen = "ai"))} title="Write the notes from the commits since the last release">
          {#if writing}<LoaderCircle size={13} class="spin" />{:else}<Sparkles size={13} />{/if} AI notes
        </button>
        {#if s.notes.trim()}
          <button class="icon-btn" disabled={writing} onclick={() => (gr.aiReady ? writeNotes(true) : (gr.settingsOpen = "ai"))} title="Polish what you wrote" aria-label="Polish the notes">
            <WandSparkles size={13} />
          </button>
        {/if}
      </div>
      <textarea class="input notes" bind:value={s.notes}
        placeholder={s.mode === "actions" ? "Release notes (Markdown). Left empty, GitHub lists the changes since the last release." : "Release notes: what changed (Markdown)"}
        aria-label="Release notes" oninput={() => (s.touched = true)}></textarea>
    </section>
    <!-- A failing check shows here, one line each; all of them behind the chip below -->
    {#if checks?.some((c) => c.state !== "ok")}
      <ul class="checks inline-checks" aria-label="Checks to look at">
        {#each checks.filter((c) => c.state !== "ok") as c (c.id)}
          <li class="c-{c.state}">
            {#if c.state === "warn"}<CircleAlert size={14} />{:else}<CircleX size={14} />{/if}
            <span>{c.message}</span>
            {#if c.fix}<button class="btn small" onclick={() => fix(c)}>{fixLabels[c.fix] ?? "Fix"}</button>{/if}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="go">
      <Popover placement="top" align="start">
        {#snippet trigger({ toggle, open })}
          <button class="summary checks-chip" class:bad={failed.length > 0} class:ok={!!checks && !failed.length} aria-expanded={open} aria-haspopup="dialog" onclick={toggle}>
            {#if checking || !checks}<LoaderCircle size={12} class="spin" /> Checking…
            {:else if failed.length}<CircleX size={12} /> {failed.length} {failed.length === 1 ? "check fails" : "checks fail"}
            {:else}<CircleCheck size={12} /> {checks.length} checks OK{/if}
          </button>
        {/snippet}
        {#snippet children()}
          <div class="pop" role="dialog" aria-label="Checks">
            <div class="assets-head">
              <span>Checks</span>
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
          </div>
        {/snippet}
      </Popover>
      {#if s.mode === "local"}
        <label class="opt inline"><input type="checkbox" class="check" bind:checked={s.draft} /> Draft</label>
      {/if}
      <label class="opt inline"><input type="checkbox" class="check" bind:checked={s.prerelease} /> Pre-release</label>
      <label class="opt inline"><input type="checkbox" class="check" bind:checked={s.makeLatest} disabled={s.prerelease} /> Latest</label>
      <span class="spacer"></span>
      <button class="btn primary big" disabled={!!busy || checking || !checks || failed.length > 0 || !s.version.trim()} onclick={() => void release()}>
        <Rocket size={16} /> Release {tag}
      </button>
    </div>
  {:else}
    <!-- The run -->
    <section class="run">
      <div class="notes-head run-head">
        <div class="run-title">
          {#if s.running && stopping}
            <LoaderCircle size={18} class="spin stopping-icon" />
            <h3>Stopping release {tag}…</h3>
            <span class="run-badge warn">Rolling back</span>
          {:else if s.running}
            <LoaderCircle size={18} class="spin running-icon" />
            <h3>Releasing {tag}</h3>
            <span class="run-badge info">In progress</span>
          {:else if s.outcome?.ok}
            <CircleCheck size={18} class="ok-icon" />
            <h3>Released {s.outcome?.tag ?? tag}</h3>
            <span class="run-badge success">Published</span>
          {:else if s.outcome?.cancelled}
            <CircleSlash size={18} class="cancel-icon" />
            <h3>Cancelled {s.outcome?.tag ?? tag}</h3>
            <span class="run-badge neutral">Cancelled</span>
          {:else}
            <CircleX size={18} class="fail-icon" />
            <h3>Stopped {s.outcome?.tag ?? tag}</h3>
            <span class="run-badge danger">Failed</span>
          {/if}
        </div>
        {#if s.running && following}
          <!-- The tag is pushed: the release goes on on GitHub whatever MYLE does. -->
          <div class="row">
            <button class="btn small" disabled={stopping} onclick={() => void stopFollowing()} title="MYLE stops following the run; it goes on on GitHub">
              <Square size={12} /> Stop following
            </button>
            <button class="btn small danger" disabled={stopping || !s.run?.id} onclick={() => void cancelOnGithub()} title="Cancel the workflow run on GitHub">
              {#if stopping}<LoaderCircle size={12} class="spin" />{:else}<CircleSlash size={12} />{/if} Cancel run on GitHub
            </button>
          </div>
        {:else if s.running}
          <button class="btn small danger" disabled={stopping} onclick={() => void cancel()}>
            {#if stopping}
              <LoaderCircle size={12} class="spin" /> Stopping…
            {:else}
              <Square size={12} /> Stop release
            {/if}
          </button>
        {/if}
      </div>
      {#if s.failures.length}
        <div class="failures" role="alert" bind:this={failuresBox}>
          {#each s.failures as f (failureKey(f))}
            <div class="failure">
              <p class="f-head">
                <CircleX size={14} />
                <strong>{f.job}</strong>{#if f.step}<span>failed at <b>{f.step}</b></span>{/if}
                {#if f.url}<button class="link" onclick={() => void openUrl(f.url!)}>Open the job <ExternalLink size={11} /></button>{/if}
              </p>
              {#each f.errors.slice(0, 5) as e (e)}<p class="f-error selectable">{e}</p>{/each}
              {#if f.logTail}<pre class="f-log selectable">{f.logTail}</pre>{/if}
            </div>
          {/each}
        </div>
      {/if}
      <ol class="steps">
        {#each steps as id (id)}
          {@const state = stepIcon(id)}
          <li class="step s-{state ?? 'waiting'}" class:active={state === "running"}>
            <div class="step-icon">
              {#if state === "running"}<LoaderCircle size={15} class="spin" />{:else if state === "done"}<CircleCheck size={15} />{:else if state === "failed"}<CircleX size={15} />{:else if state === "skipped"}<CircleDashed size={15} />{:else}<Circle size={15} />{/if}
            </div>
            <div class="step-content">
              <span class="label">{stepLabels[id]}</span>
              {#if s.steps[id]?.message}<span class="msg">{s.steps[id]?.message}</span>{/if}
            </div>
            {#if state}
              <span class="step-tag tag-{state}">{state === "running" ? "running" : state === "done" ? "done" : state === "failed" ? "failed" : "skipped"}</span>
            {/if}
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
            <CircleCheck size={20} />
            <div class="result-body">
              <strong>{s.outcome.tag} is published</strong>
              <span>{s.outcome.release?.draft ? "Draft saved on GitHub." : "Release is live and available on GitHub."}</span>
            </div>
            <div class="result-actions">
              {#if s.outcome.release}<button class="btn small" onclick={() => void openUrl(s.outcome!.release!.htmlUrl)}><ExternalLink size={13} /> Open the release</button>{/if}
              {#if s.outcome.run}<button class="btn small" onclick={() => void openUrl(s.outcome!.run!.htmlUrl)}><ExternalLink size={13} /> Open the run</button>{/if}
            </div>
          </div>
        {:else if s.outcome.cancelled}
          <div class="result cancelled">
            <CircleSlash size={20} />
            <div class="result-body">
              <strong>Release cancelled</strong>
              <span>
                {s.outcome.problem?.message ?? "The release was cancelled."}
                {#if s.outcome.rolledBack}<em>Nothing was pushed: the version files and commit were put back.</em>{/if}
              </span>
            </div>
            <div class="result-actions">
              {#if s.outcome.run}<button class="btn small" onclick={() => void openUrl(s.outcome!.run!.htmlUrl)}><ExternalLink size={13} /> Open the run</button>{/if}
            </div>
          </div>
        {:else}
          <div class="result bad">
            <CircleX size={20} />
            <div class="result-body">
              <strong>Release stopped</strong>
              <span>
                {s.outcome.problem?.message ?? "The release stopped."}
                {#if s.outcome.rolledBack}<em>Nothing was pushed: the version files and commit were put back.</em>{/if}
              </span>
            </div>
            <div class="result-actions">
              {#if s.outcome.resume}
                <button class="btn small primary" disabled={!!busy} onclick={() => void resume()}><RotateCcw size={13} /> Resume</button>
              {/if}
              {#if s.outcome.commit && s.mode === "actions" && !s.outcome.resume}
                <button class="btn small" disabled={!!busy} onclick={() => void watchAgain()}><RefreshCw size={13} /> Follow the run</button>
              {/if}
              {#if s.outcome.run}<button class="btn small" onclick={() => void openUrl(s.outcome!.run!.htmlUrl)}><ExternalLink size={13} /> Open the run</button>{/if}
            </div>
          </div>
        {/if}
        <button class="btn small ghost new-btn" onclick={() => (s.outcome = null)}>
          <ArrowLeft size={13} /> {s.outcome.ok ? "Start the next release" : "Back to the form"}
        </button>
      {/if}
    </section>
  {/if}
</div>
<style>
  .release { display: flex; flex: 1; flex-direction: column; gap: 6px; min-height: 0; padding: 8px 10px 10px; overflow: auto; }
  section { display: grid; gap: 6px; padding: 8px 11px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 12px; background: rgb(255 255 255 / 0.025); }
  h3 { display: flex; align-items: center; gap: 7px; margin: 0; color: var(--text-1); font-size: 13px; font-weight: 650; }
  .row { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .wrap { flex-wrap: wrap; }
  .version { width: 78px; height: 28px; font-family: var(--font-mono); font-size: 12.5px; }
  .tag-preview { flex: none; color: var(--text-3); font-size: 11.5px; white-space: nowrap; }
  code { font-family: var(--font-mono); font-size: 11.3px; }
  .tag-preview code { color: #c3c9f7; }
  .sub { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin: 0; color: var(--text-3); font-size: 11.8px; }
  .sub.warn { color: #ffd08a; }
  .link { display: inline-flex; align-items: center; gap: 4px; padding: 0; color: #b7befa; font-size: inherit; text-decoration: underline; text-underline-offset: 2px; }
  .commits { display: grid; gap: 2px; max-height: 180px; margin: 0; padding: 8px 10px; overflow: auto; border-radius: 8px; background: rgb(0 0 0 / 0.2); color: var(--text-2); font-size: 11.8px; list-style: none; }
  .commits code { color: var(--text-3); }
  .files { display: grid; gap: 2px; }
  .file { display: flex; align-items: center; gap: 8px; padding: 4px 6px; border-radius: 7px; font-size: 12px; }
  .file:hover { background: var(--hover); }
  .file.off { opacity: 0.55; }
  .file code { flex: 1; min-width: 0; overflow: hidden; color: var(--text-2); text-overflow: ellipsis; }
  .from { color: var(--text-3); font-family: var(--font-mono); font-size: 11.3px; }
  .from.bad { color: #ffb84d; }
  .to { color: #8fe6bf; font-family: var(--font-mono); font-size: 11.3px; }
  .lock { padding-left: 26px; color: var(--text-3); font-size: 11px; }
  .file .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file small { color: var(--text-3); font-size: 11px; }
  .modes { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .mode { display: grid; gap: 3px; padding: 10px 12px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 10px; text-align: left; }
  .mode:hover:not(:disabled) { background: var(--hover); }
  .mode.active { border-color: rgb(var(--accent-rgb) / 0.5); background: rgb(var(--accent-rgb) / 0.12); }
  .mode:disabled { opacity: 0.5; }
  .mode strong { font-size: 12.8px; }
  .mode span { color: var(--text-3); font-size: 11.5px; line-height: 1.4; }
  .opt { display: flex; align-items: center; gap: 9px; color: var(--text-2); font-size: 12.3px; }
  .opt code { margin-left: 4px; color: var(--text-3); }
  .opt em { color: var(--text-3); font-style: normal; }
  .opt.inline { gap: 6px; }
  .assets { display: grid; gap: 3px; padding: 8px; border-radius: 9px; background: rgb(0 0 0 / 0.16); }
  .assets-head { display: flex; align-items: center; justify-content: space-between; padding: 0 4px 4px; color: var(--text-2); font-size: 12px; }
  .notes-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .notes { flex: 1; min-height: 30px; resize: none; font-family: var(--font-mono); font-size: 12px; line-height: 1.5; }
  .commit { flex: 1; min-width: 140px; }
  .title { flex: 1.3; min-width: 150px; }
  /* One-row sections, and the notes taking what height is left. */
  .line { gap: 5px; padding: 6px 10px; }
  .label { flex: none; color: var(--text-2); font-size: 12px; font-weight: 600; }
  .bump { flex: none; height: 26px; padding: 0 8px; border: 1px solid var(--btn-border); border-radius: 7px; background: var(--btn-fill); color: var(--text-2); font-family: var(--font-mono); font-size: 11.8px; }
  .bump:hover { background: var(--hover); color: var(--text-1); }
  .bump.active { border-color: rgb(var(--accent-rgb) / 0.55); background: rgb(var(--accent-rgb) / 0.18); color: var(--text-1); }
  .line .input, .notes-section .row .input { height: 28px; }
  .how { gap: 6px 10px; }
  .spacer { flex: 1; }
  .opts { gap: 6px 14px; }
  .opts:empty { display: none; }
  .notes-section { display: flex; flex: 1 1 0; flex-direction: column; gap: 6px; min-height: 78px; padding: 7px 10px; }
  .summary { display: inline-flex; align-items: center; gap: 5px; max-width: 100%; height: 26px; padding: 0 9px; overflow: hidden; border: 1px solid var(--btn-border); border-radius: 8px; background: var(--btn-fill); color: var(--text-2); font-size: 11.8px; white-space: nowrap; }
  .summary:hover { background: var(--hover); color: var(--text-1); }
  .summary.warn { border-color: rgb(255 184 77 / 0.4); color: #ffd08a; }
  .summary .dim { color: var(--text-3); }
  .checks-chip.ok { border-color: rgb(62 207 142 / 0.35); color: #8fe6bf; }
  .checks-chip.bad { border-color: rgb(255 123 123 / 0.45); color: #ffb3b3; }
  .pop { display: grid; gap: 6px; width: min(460px, 80vw); max-height: 320px; padding: 10px; overflow: auto; border: 1px solid var(--glass-border); border-radius: 12px; background: var(--float-bg); box-shadow: var(--elev-3); }
  .pop-label { margin-top: 4px; color: var(--text-3); font-size: 11px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
  .pop .commits { max-height: 150px; }
  .checks.inline-checks { flex: none; gap: 0; max-height: 66px; padding: 0 4px; overflow: auto; }
  .checks.inline-checks li { min-height: 22px; padding: 0 2px; font-size: 12px; }
  .checks.inline-checks .btn { height: 20px; }
  .line .row { gap: 6px; }
  .checks { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .checks li { display: flex; align-items: center; gap: 8px; padding: 4px 2px; font-size: 12.3px; }
  .checks li span { flex: 1; color: var(--text-2); }
  .c-ok { color: #3ecf8e; }
  .c-warn { color: #ffb84d; }
  .c-fail { color: #ff7b7b; }
  .go { display: flex; flex: none; align-items: center; gap: 12px; }
  .big { height: 36px; padding: 0 18px; font-size: 13.5px; }
  .run { display: flex; flex-direction: column; gap: 10px; padding: 12px 14px; background: rgb(0 0 0 / 0.14); border: 1px solid rgb(255 255 255 / 0.06); border-radius: 12px; }
  .run-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-bottom: 8px; border-bottom: 1px solid rgb(255 255 255 / 0.07); }
  .run-title { display: flex; align-items: center; gap: 10px; }
  .run-title h3 { margin: 0; font-size: 16px; font-weight: 600; }
  .run-badge { display: inline-flex; align-items: center; padding: 2px 8px; border-radius: 6px; font-size: 11px; font-weight: 600; letter-spacing: 0.02em; text-transform: uppercase; }
  .run-badge.info { background: rgb(159 211 255 / 0.14); color: #9fd3ff; }
  .run-badge.warn { background: rgb(255 184 77 / 0.14); color: #ffb84d; }
  .run-badge.success { background: rgb(62 207 142 / 0.14); color: #3ecf8e; }
  .run-badge.neutral { background: rgb(255 255 255 / 0.08); color: var(--text-2); }
  .run-badge.danger { background: rgb(255 123 123 / 0.14); color: #ff7b7b; }
  .stopping-icon { color: #ffb84d; }
  .running-icon { color: #9fd3ff; }
  .ok-icon { color: #3ecf8e; }
  .cancel-icon { color: #ffb84d; }
  .fail-icon { color: #ff7b7b; }
  .steps { display: flex; flex-direction: column; gap: 3px; margin: 0; padding: 0; list-style: none; }
  .step { display: flex; align-items: center; gap: 10px; padding: 5px 10px; border-radius: 8px; background: rgb(255 255 255 / 0.02); border: 1px solid transparent; color: var(--text-3); font-size: 12.8px; transition: all 0.2s ease; }
  .step.active { background: rgb(var(--accent-rgb) / 0.09); border-color: rgb(var(--accent-rgb) / 0.3); color: var(--text-1); box-shadow: 0 2px 8px rgb(0 0 0 / 0.15); }
  .step-icon { display: grid; place-items: center; flex: none; width: 20px; }
  .step-content { display: flex; align-items: baseline; gap: 10px; flex: 1; min-width: 0; }
  .step .label { min-width: 120px; font-weight: 550; color: var(--text-2); }
  .step.active .label { color: var(--text-1); }
  .step .msg { flex: 1; min-width: 0; overflow: hidden; color: var(--text-3); font-size: 11.8px; text-overflow: ellipsis; white-space: nowrap; }
  .step-tag { flex: none; padding: 1px 7px; border-radius: 4px; font-size: 10.5px; font-weight: 600; text-transform: capitalize; }
  .tag-running { background: rgb(159 211 255 / 0.14); color: #9fd3ff; }
  .tag-done { background: rgb(62 207 142 / 0.12); color: #3ecf8e; }
  .tag-failed { background: rgb(255 123 123 / 0.12); color: #ff7b7b; }
  .tag-skipped { background: rgb(255 255 255 / 0.05); color: var(--text-3); }
  .s-running { color: #9fd3ff !important; }
  .s-done { color: #3ecf8e !important; }
  .s-failed { color: #ff7b7b !important; }
  li.upload,
  li.jobs { display: grid !important; gap: 5px; padding: 4px 12px 8px 42px !important; background: rgb(255 255 255 / 0.015); border-radius: 8px; }
  .upload .progress { height: 5px; overflow: hidden; border-radius: 999px; background: rgb(255 255 255 / 0.08); }
  .upload .progress div { height: 100%; background: var(--accent-grad); transition: width 0.25s; }
  .upload em { font-style: normal; font-size: 11px; }
  .job { display: flex; align-items: center; gap: 7px; font-size: 12px; }
  .job strong { color: var(--text-2); font-weight: 550; }
  .job span { color: var(--text-3); font-size: 11.3px; }
  .j-ok { color: #3ecf8e; }
  .j-failed { color: #ff7b7b; }
  .j-running { color: #9fd3ff; }
  .build-log { display: flex; flex-direction: column; gap: 7px; }
  details.log summary { color: var(--text-3); font-size: 11.8px; cursor: pointer; }
  details.log pre { max-height: 160px; margin: 6px 0 0; padding: 8px 10px; overflow: auto; border-radius: 8px; background: rgb(0 0 0 / 0.25); color: var(--text-2); font-family: var(--font-mono); font-size: 11px; white-space: pre-wrap; }
  .result { display: flex; align-items: flex-start; gap: 14px; padding: 14px 16px; border-radius: 10px; font-size: 13px; }
  .result-body { display: flex; flex-direction: column; gap: 3px; flex: 1; min-width: 0; }
  .result-body strong { font-size: 13.5px; font-weight: 600; color: var(--text-1); }
  .result-body span { color: var(--text-2); font-size: 12px; line-height: 1.4; }
  .result-body em { display: block; margin-top: 2px; color: var(--text-3); font-style: normal; }
  .result-actions { display: flex; align-items: center; gap: 8px; flex: none; }
  .result.ok { border: 1px solid rgb(62 207 142 / 0.3); background: rgb(62 207 142 / 0.08); color: #3ecf8e; }
  .result.cancelled { border: 1px solid rgb(255 184 77 / 0.3); background: rgb(255 184 77 / 0.08); color: #ffb84d; }
  .result.bad { border: 1px solid rgb(229 72 77 / 0.3); background: rgb(229 72 77 / 0.08); color: #ff7b7b; }
  .new-btn { align-self: flex-start; margin-top: 4px; gap: 6px; }
  .quiet { display: grid; place-items: center; min-height: 160px; color: var(--text-3); }
  .error { color: #ff9d9d; font-size: 12.5px; }
  @container releases (max-width: 900px) {
    .modes {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  /* Merge, target, failures */
  .pick { gap: 14px; }
  .field { display: inline-flex; align-items: center; gap: 6px; color: var(--text-2); font-size: 11.5px; }
  .branch-field { position: relative; }
  .merge-summary { display: flex; flex-direction: column; gap: 4px; margin-top: 2px; }
  .protected-branch-note { color: var(--text-3); font-size: 11.5px; }
  .select { height: 28px; min-width: 170px; padding: 0 8px; font-size: 12px; }
  .seg { display: inline-flex; padding: 2px; border: 1px solid var(--btn-border); border-radius: 8px; }
  .seg button { padding: 3px 7px; border: 0; border-radius: 6px; background: transparent; color: var(--text-2); font-size: 11.5px; white-space: nowrap; cursor: pointer; }
  .seg button:disabled { opacity: 0.45; cursor: default; }
  .seg button.active { background: rgb(var(--accent-rgb) / 0.18); color: var(--text-1); }
  .failures { display: grid; flex: none; gap: 8px; max-height: 240px; overflow: auto; }
  .failure { display: grid; gap: 4px; padding: 8px 10px; border: 1px solid rgb(229 72 77 / 0.35); border-radius: 10px; background: rgb(229 72 77 / 0.07); }
  .f-head { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin: 0; color: #f1a7a9; font-size: 12px; }
  .f-head span { color: var(--text-2); }
  .f-error { margin: 0; color: var(--text-1); font-size: 11.5px; }
  .f-log { max-height: 180px; margin: 0; padding: 6px 8px; overflow: auto; border-radius: 6px; background: rgb(0 0 0 / 0.35); color: var(--text-2); font-size: 10.5px; line-height: 1.4; white-space: pre-wrap; }
</style>
