<script lang="ts">
  import Eye from "@lucide/svelte/icons/eye";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Select from "../../../lib/components/Select.svelte";
  import { onMount } from "svelte";
  import { confirm } from "../../../lib/confirm.svelte";
  import { projectBackupsApi as api, type Preview } from "./api";
  import PreviewList from "./PreviewList.svelte";
  import { parseLines, projectBackupsState as pb } from "./state.svelte";

  const settings = pb.page.settings;
  let text = $state(settings.exclusions.join("\n"));
  let smartBuild = $state(settings.smartBuild);
  let followGitignore = $state(settings.followGitignore);
  let previewProject = $state(pb.projects.find((project) => !pb.isMissing(project))?.id ?? "");
  let preview = $state<Preview | null>(null);
  let previewError = $state<string | null>(null);
  let previewing = $state(false);

  const projectOptions = $derived(pb.projects.filter((project) => !pb.isMissing(project)).map((project) => ({ value: project.id, label: project.name })));
  const changed = $derived(
    parseLines(text).join("\n") !== pb.page.settings.exclusions.join("\n") ||
      smartBuild !== pb.page.settings.smartBuild ||
      followGitignore !== pb.page.settings.followGitignore,
  );

  /** Anything typed drops the preview: it showed the list before the edit. */
  function edited() {
    preview = null;
  }

  // Leaving the panel: a preview still running is stopped.
  onMount(() => () => { if (previewing) void api.cancelPreview() });

  async function resetToDefaults() {
    const ok = await confirm({ title: "Use the default exclusions?", message: "Your own patterns in this list are replaced by MYLE's defaults. Nothing changes until you press Save.", confirmLabel: "Use defaults" });
    if (!ok) return;
    text = pb.page.defaultExclusions.join("\n");
    edited();
  }

  async function runPreview() {
    if (!previewProject) return;
    previewing = true;
    previewError = null;
    try {
      preview = await api.preview({ projectId: previewProject, patterns: parseLines(text), smartBuild, followGitignore });
    } catch (error) {
      preview = null;
      const reason = error instanceof Error ? error.message : String(error);
      previewError = reason === "Cancelled." ? null : reason;
    } finally {
      previewing = false;
    }
  }
  async function save() {
    await pb.setExclusions(parseLines(text), smartBuild, followGitignore);
  }
</script>

<section id="project-backups-exclusions" class="panel surface" aria-labelledby="exclusions-heading">
  <div class="card-head">
    <span class="card-icon"><ListFilter size={18} /></span>
    <div>
      <h2 id="exclusions-heading">What backups leave out</h2>
      <p>For every project. A project can add its own and keep some back in (Edit project).</p>
    </div>
  </div>

  <div class="columns">
    <label class="field">
      <span>Patterns <small>one per line · <code>name/</code> a folder anywhere · <code>*.log</code> files · <code>path/inside/</code> from the project folder</small></span>
      <textarea class="input mono" rows="9" bind:value={text} oninput={edited} disabled={!!pb.busy}></textarea>
    </label>
    <div class="side">
      <label class="toggle surface">
        <span>
          <strong>Find build and cache folders</strong>
          <small>Leaves out folders such as <code>target</code>, <code>build</code>, <code>bin</code>/<code>obj</code>, <code>out</code>, <code>venv</code> or Unity's <code>Library</code> only when they are output: a marker says so (Cargo.toml, a .csproj, pyvenv.cfg, CMakeCache.txt, …), git tracks nothing in them, or <code>.gitignore</code> lists them. Folders git tracks, like an installer's <code>build</code>, are always kept.</small>
        </span>
        <input class="switch" type="checkbox" bind:checked={smartBuild} onchange={edited} disabled={!!pb.busy} />
      </label>
      <label class="toggle surface">
        <span>
          <strong>Follow .gitignore</strong>
          <small>Also leave out what the project's <code>.gitignore</code> files ignore. <code>.env</code> files are backed up either way.</small>
        </span>
        <input class="switch" type="checkbox" bind:checked={followGitignore} onchange={edited} disabled={!!pb.busy} />
      </label>
    </div>
  </div>

  <div class="actions">
    <button class="btn ghost" disabled={!!pb.busy || parseLines(text).join("\n") === pb.page.defaultExclusions.join("\n")} onclick={resetToDefaults}><RotateCcw size={14} /> Defaults</button>
    <span class="spacer"></span>
    {#if projectOptions.length}
      <Select bind:value={previewProject} options={projectOptions} ariaLabel="Project to preview" size="sm" onchange={edited} />
      <button class="btn" disabled={!previewProject || previewing} onclick={runPreview}>
        {#if previewing}<LoaderCircle size={14} class="spin" />{:else}<Eye size={14} />{/if} Preview
      </button>
    {/if}
    <button class="btn primary" disabled={!changed || !!pb.busy} onclick={save}>{pb.busy === "exclusions" ? "Saving…" : "Save"}</button>
  </div>
  {#if previewError}<p class="error">{previewError}</p>{/if}
  {#if preview}<PreviewList {preview} />{/if}
</section>

<style>
  .panel { display: grid; gap: 12px; margin-bottom: 12px; padding: 14px; }
  .card-head { display: flex; align-items: flex-start; gap: 10px; }
  .card-icon { display: grid; place-items: center; width: 34px; height: 34px; border-radius: 10px; background: rgb(var(--accent-rgb) / 0.08); color: var(--accent); }
  h2 { font-size: 13.5px; }
  .card-head p { margin-top: 2px; color: var(--text-3); font-size: 11.5px; }
  .columns { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr); gap: 12px; }
  .field { display: grid; gap: 6px; min-width: 0; }
  .field > span { color: var(--text-2); font-size: 12px; font-weight: 600; }
  .field small { color: var(--text-3); font-size: 10.5px; font-weight: 400; }
  code { font-family: var(--font-mono); font-size: 0.95em; }
  .mono { font-family: var(--font-mono); font-size: 11.5px; line-height: 1.5; }
  textarea { resize: vertical; min-height: 150px; }
  .side { display: grid; gap: 8px; align-content: start; }
  .toggle { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 10px 11px; }
  .toggle > span { display: grid; gap: 3px; }
  .toggle strong { font-size: 12px; }
  .toggle small { color: var(--text-3); font-size: 10.75px; line-height: 1.45; }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .spacer { flex: 1; }
  .error { color: rgb(255 145 145 / 0.85); font-size: 11.5px; }
  button:disabled { opacity: 0.45; pointer-events: none; }
  @container (max-width: 720px) { .columns { grid-template-columns: minmax(0, 1fr); } }
</style>
