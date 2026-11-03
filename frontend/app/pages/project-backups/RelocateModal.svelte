<script lang="ts">
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import X from "@lucide/svelte/icons/x";
  import { portal } from "../../../lib/portal";
  import { dialogFocus } from "../game-saves/dialog-focus";
  import { projectBackupsApi as api, type CandidateFolder, type DriveInfo, type Project, type ProjectRelocation } from "./api";
  import { projectBackupsState as pb } from "./state.svelte";

  interface ProjectRowState {
    project: Project;
    selected: boolean;
    newPath: string;
    candidates: CandidateFolder[];
    isManual: boolean;
  }

  let drives = $state<DriveInfo[]>([]);
  let selectedRoot = $state("A:\\");
  let scanning = $state(false);
  let scanError = $state<string | null>(null);
  let allCandidates = $state<CandidateFolder[]>([]);

  let prefixFrom = $state("");
  let prefixTo = $state("A:\\");
  let filterMissingOnly = $state(true);

  let rows = $state<ProjectRowState[]>([]);

  function normalize(str: string): string {
    return str.toLowerCase().replace(/[^a-z0-9]/g, "");
  }

  function getFolderStem(path: string): string {
    return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
  }

  function detectCommonPrefix(projects: Project[]): string {
    const paths = projects.map((p) => p.sourcePath.trim()).filter((p) => p.length > 2);
    if (!paths.length) return "";
    let prefix = paths[0];
    for (const p of paths.slice(1)) {
      while (prefix.length && !p.toLowerCase().startsWith(prefix.toLowerCase())) {
        const lastSlash = Math.max(prefix.lastIndexOf("\\"), prefix.lastIndexOf("/"));
        if (lastSlash < 0) {
          prefix = "";
          break;
        }
        prefix = prefix.slice(0, lastSlash);
      }
    }
    if (prefix && !prefix.endsWith("\\") && !prefix.endsWith("/")) {
      prefix += "\\";
    }
    return prefix;
  }

  function matchScore(project: Project, candidate: CandidateFolder): number {
    const cName = candidate.name.toLowerCase();
    const cNorm = normalize(candidate.name);
    const pNorm = normalize(project.name);
    const appNorm = normalize(project.appName);
    const oldFolder = getFolderStem(project.sourcePath).toLowerCase();
    const oldFolderNorm = normalize(oldFolder);

    // 1. Exact folder name
    if (oldFolder && cName === oldFolder) return 100;

    // 2. Git remote URL matches
    if (candidate.gitRemote) {
      const gitStem = candidate.gitRemote.replace(/\.git$/i, "").split(/[\\/]/).pop()?.toLowerCase() ?? "";
      const gitNorm = normalize(gitStem);
      if (gitNorm === pNorm || gitNorm === appNorm || gitNorm === oldFolderNorm) return 98;
      if (gitStem === "myle" && pNorm.includes("makeyourlifeeasier")) return 98;
    }

    // 3. Package name matches
    if (candidate.packageName) {
      const pkgNorm = normalize(candidate.packageName);
      if (pkgNorm === pNorm || pkgNorm === appNorm || pkgNorm === oldFolderNorm) return 96;
      if (pkgNorm.includes("makeyourlifeeasier") && pNorm.includes("makeyourlifeeasier")) return 96;
    }

    // 4. Normalized name matches
    if (cNorm === pNorm || cNorm === appNorm || cNorm === oldFolderNorm) return 92;
    if (cNorm === "myle" && pNorm.includes("makeyourlifeeasier")) return 95;

    // 5. Contains substring
    if (pNorm.length > 3 && cNorm.includes(pNorm)) return 75;
    if (cNorm.length > 3 && pNorm.includes(cNorm)) return 75;

    return 0;
  }

  function updateCandidateMatches() {
    const list = pb.projects;
    const newRows: ProjectRowState[] = [];

    for (const project of list) {
      const isMissing = pb.health(project) === "missing";
      if (filterMissingOnly && !isMissing) continue;

      const scored = allCandidates
        .map((candidate) => ({ candidate, score: matchScore(project, candidate) }))
        .filter((item) => item.score > 0)
        .sort((a, b) => b.score - a.score);

      const matchedCandidates = scored.map((s) => s.candidate);
      const best = matchedCandidates[0];

      // Keep previous manual path if user typed/browsed one
      const prev = rows.find((r) => r.project.id === project.id);
      let newPath = prev?.isManual ? prev.newPath : best ? best.path : "";
      let selected = prev?.isManual ? prev.selected : !!best;

      newRows.push({
        project,
        selected,
        newPath,
        candidates: matchedCandidates,
        isManual: prev?.isManual ?? false,
      });
    }

    rows = newRows;
  }

  async function scanRoot(root: string) {
    selectedRoot = root;
    scanning = true;
    scanError = null;
    try {
      allCandidates = await api.scanCandidates(root, 3);
      updateCandidateMatches();
    } catch (err) {
      scanError = err instanceof Error ? err.message : String(err);
      allCandidates = [];
    } finally {
      scanning = false;
    }
  }

  function applyPrefix() {
    const from = prefixFrom.trim().toLowerCase();
    const to = prefixTo.trim();
    if (!from || !to) return;

    for (const row of rows) {
      const old = row.project.sourcePath;
      if (old.toLowerCase().startsWith(from)) {
        const rest = old.slice(from.length).replace(/^[\\/]+/, "");
        const combined = to.replace(/[\\/]+$/, "") + "\\" + rest;
        row.newPath = combined;
        row.selected = true;
        row.isManual = true;
      }
    }
  }

  async function pickCustomRoot() {
    const folder = await pb.pickFolder("Choose drive or folder to scan for projects");
    if (folder) {
      void scanRoot(folder);
    }
  }

  async function pickProjectFolder(row: ProjectRowState) {
    const folder = await pb.pickFolder(`Choose folder for ${row.project.name}`);
    if (folder) {
      row.newPath = folder;
      row.selected = true;
      row.isManual = true;
    }
  }

  function toggleAll(checked: boolean) {
    for (const row of rows) {
      if (row.newPath.trim()) {
        row.selected = checked;
      }
    }
  }

  const selectedCount = $derived(rows.filter((r) => r.selected && r.newPath.trim()).length);

  async function applyReplacements() {
    const replacements: ProjectRelocation[] = rows
      .filter((r) => r.selected && r.newPath.trim())
      .map((r) => ({
        projectId: r.project.id,
        newSourcePath: r.newPath.trim(),
      }));

    if (!replacements.length) return;
    await pb.relocateProjects(replacements);
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      pb.closeRelocateModal();
    }
  }

  onMount(() => {
    const missing = pb.projects.filter((p) => pb.health(p) === "missing");
    prefixFrom = detectCommonPrefix(missing);

    void api.getDrives().then((foundDrives) => {
      drives = foundDrives;
      const targetDrive = foundDrives.find((d) => d.root.toUpperCase() === "A:\\") ?? foundDrives[0];
      const initialRoot = targetDrive ? targetDrive.root : "A:\\";
      selectedRoot = initialRoot;
      prefixTo = initialRoot;
      void scanRoot(initialRoot);
    });
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" transition:fade={{ duration: 140 }} role="presentation" {@attach portal}>
  <div class="dialog surface" {@attach dialogFocus} transition:scale={{ duration: 160, start: 0.96 }} role="dialog" aria-modal="true" aria-labelledby="relocate-title" tabindex="-1">
    <header>
      <div class="title-area">
        <span class="header-icon"><FolderSearch size={20} /></span>
        <div>
          <h2 id="relocate-title">Relocate Projects</h2>
          <p>Scan drives to reconnect missing project folders after a format or drive letter change.</p>
        </div>
      </div>
      <button class="icon-btn" title="Close" aria-label="Close dialog" onclick={() => pb.closeRelocateModal()}><X size={15} /></button>
    </header>

    <div class="modal-body">
      <!-- Drive selector and scanner -->
      <section class="section scan-section">
        <div class="section-title">
          <HardDrive size={14} />
          <strong>Scan Drive / Folder</strong>
          {#if scanning}<LoaderCircle size={13} class="spin" /> Scanning…{/if}
        </div>
        <div class="drives-row">
          {#each drives as drive (drive.letter)}
            {@const active = selectedRoot.toLowerCase() === drive.root.toLowerCase()}
            <button
              class="chip drive-chip"
              class:active
              disabled={scanning || !!pb.busy}
              onclick={() => scanRoot(drive.root)}
              title={`${drive.label || "Drive"} (${drive.root}) · ${Math.round(drive.free / 1024 / 1024 / 1024)} GB free`}
            >
              <HardDrive size={12} />
              <strong>{drive.letter}</strong>
              <small>{drive.label || drive.root}</small>
            </button>
          {/each}
          <button class="chip" disabled={scanning || !!pb.busy} onclick={pickCustomRoot}>
            <FolderOpen size={12} /> Browse folder…
          </button>
          <button class="icon-btn refresh-btn" title="Rescan current drive" disabled={scanning || !!pb.busy} onclick={() => scanRoot(selectedRoot)}>
            <RefreshCw size={13} class={scanning ? "spin" : ""} />
          </button>
        </div>
        {#if scanError}
          <p class="error-msg"><CircleAlert size={13} /> {scanError}</p>
        {:else if !scanning && allCandidates.length > 0}
          <p class="scan-status"><Sparkles size={12} /> Found {allCandidates.length} candidate project {allCandidates.length === 1 ? "folder" : "folders"} on {selectedRoot}</p>
        {/if}
      </section>

      <!-- Quick path prefix replacement -->
      <section class="section prefix-section">
        <div class="section-title">
          <ArrowRight size={14} />
          <strong>Quick Path Prefix Replace</strong>
          <small class="hint-text">Replace old drive letter or base folder</small>
        </div>
        <div class="prefix-inputs">
          <div class="prefix-col">
            <span class="label">Replace:</span>
            <input class="input mono small" bind:value={prefixFrom} placeholder="H:\Projects\ThomasThanos\" />
          </div>
          <span class="arrow-icon"><ArrowRight size={14} /></span>
          <div class="prefix-col">
            <span class="label">With:</span>
            <input class="input mono small" bind:value={prefixTo} placeholder="A:\" />
          </div>
          <button class="btn small" disabled={!prefixFrom.trim() || !prefixTo.trim() || !!pb.busy} onclick={applyPrefix}>
            Apply Prefix
          </button>
        </div>
      </section>

      <!-- Projects list table -->
      <section class="section table-section">
        <div class="table-head-row">
          <label class="check-label">
            <input
              type="checkbox"
              checked={rows.length > 0 && rows.every((r) => r.selected || !r.newPath.trim())}
              onchange={(e) => toggleAll(e.currentTarget.checked)}
            />
            <strong>Projects to Relocate ({rows.length})</strong>
          </label>
          <label class="toggle-missing">
            <input type="checkbox" bind:checked={filterMissingOnly} onchange={updateCandidateMatches} />
            <span>Missing folders only</span>
          </label>
        </div>

        {#if rows.length === 0}
          <div class="empty-state">
            <Check size={20} />
            <p>No missing projects found! All project folders are currently accessible.</p>
          </div>
        {:else}
          <div class="projects-table-scroll">
            <table class="projects-table">
              <thead>
                <tr>
                  <th style="width: 36px"></th>
                  <th style="width: 26%">Project</th>
                  <th style="width: 34%">Current / Missing Path</th>
                  <th style="width: 40%">New Path (Replacement)</th>
                </tr>
              </thead>
              <tbody>
                {#each rows as row (row.project.id)}
                  {@const hasPath = !!row.newPath.trim()}
                  <tr class:selected={row.selected && hasPath}>
                    <td class="check-cell">
                      <input type="checkbox" bind:checked={row.selected} disabled={!hasPath} />
                    </td>
                    <td class="name-cell">
                      <strong class="proj-name">{row.project.name}</strong>
                      <span class="tag-small">{row.project.appName}</span>
                    </td>
                    <td class="old-cell">
                      <span class="path-text old" title={row.project.sourcePath}>
                        {row.project.sourcePath || "No folder"}
                      </span>
                    </td>
                    <td class="new-cell">
                      <div class="new-path-control">
                        {#if row.candidates.length > 1}
                          <select
                            class="input small mono path-select"
                            value={row.newPath}
                            onchange={(e) => {
                              row.newPath = e.currentTarget.value;
                              row.selected = true;
                              row.isManual = true;
                            }}
                          >
                            <option value="">Select candidate…</option>
                            {#each row.candidates as c (c.path)}
                              <option value={c.path}>{c.name} · {c.path}</option>
                            {/each}
                          </select>
                        {:else}
                          <input
                            class="input small mono path-input"
                            bind:value={row.newPath}
                            placeholder="Enter new folder path"
                            oninput={() => {
                              row.selected = !!row.newPath.trim();
                              row.isManual = true;
                            }}
                          />
                        {/if}
                        <button class="icon-btn small" title="Browse for this project" onclick={() => pickProjectFolder(row)}>
                          <FolderOpen size={13} />
                        </button>
                      </div>
                      {#if hasPath}
                        <span class="match-badge ok"><Check size={11} /> Ready to replace</span>
                      {:else}
                        <span class="match-badge none"><CircleAlert size={11} /> Not found yet</span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>
    </div>

    <footer>
      <div class="footer-left">
        {#if selectedCount > 0}
          <span class="ready-badge"><Check size={13} /> {selectedCount} {selectedCount === 1 ? "project" : "projects"} selected for replacement</span>
        {:else}
          <span class="hint-text">Select at least one project path to replace.</span>
        {/if}
      </div>
      <button type="button" class="btn" disabled={pb.busy === "relocate"} onclick={() => pb.closeRelocateModal()}>
        Cancel
      </button>
      <button class="btn primary" disabled={selectedCount === 0 || pb.busy === "relocate"} onclick={applyReplacements}>
        {#if pb.busy === "relocate"}<LoaderCircle size={14} class="spin" /> Replacing…{:else}Replace Paths ({selectedCount}){/if}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 95;
    display: grid;
    place-items: center;
    padding: 20px;
    background: var(--scrim);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(840px, 98vw);
    max-height: calc(100vh - 40px);
    border-radius: var(--radius-xl);
    overflow: hidden;
    background: var(--bg-surface);
    box-shadow: 0 16px 40px rgb(0 0 0 / 0.5);
    border: 1px solid var(--border-subtle);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-surface-elevated, var(--bg-surface));
  }
  .title-area {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .header-icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 9px;
    background: rgb(56 189 248 / 0.12);
    color: rgb(56 189 248);
  }
  h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
  }
  header p {
    margin: 2px 0 0 0;
    font-size: 12px;
    color: var(--text-3);
  }
  .modal-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 20px;
    overflow-y: auto;
    min-height: 0;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-md, 10px);
    background: rgb(255 255 255 / 0.02);
    border: 1px solid var(--border-subtle);
  }
  .section-title {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .hint-text {
    font-size: 11px;
    color: var(--text-3);
    margin-left: 6px;
  }
  .drives-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  .drive-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border-radius: 8px;
    cursor: pointer;
  }
  .drive-chip strong {
    color: var(--text-1);
    font-size: 12px;
  }
  .drive-chip small {
    color: var(--text-3);
    font-size: 10px;
  }
  .drive-chip.active {
    background: var(--accent-subtle, rgb(56 189 248 / 0.16));
    border-color: var(--accent, rgb(56 189 248));
    color: var(--text-1);
  }
  .refresh-btn {
    margin-left: auto;
  }
  .scan-status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: rgb(52 211 153);
    margin: 2px 0 0 0;
  }
  .error-msg {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: rgb(248 113 113);
    margin: 2px 0 0 0;
  }
  .prefix-inputs {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .prefix-col {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 180px;
  }
  .prefix-col .label {
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
  }
  .prefix-col input {
    flex: 1;
  }
  .arrow-icon {
    color: var(--text-3);
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .table-head-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 4px;
  }
  .check-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    cursor: pointer;
  }
  .toggle-missing {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-3);
    cursor: pointer;
  }
  .projects-table-scroll {
    max-height: 280px;
    overflow-y: auto;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
  }
  .projects-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  .projects-table th {
    position: sticky;
    top: 0;
    background: var(--bg-surface-elevated, var(--bg-surface));
    text-align: left;
    padding: 7px 10px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-3);
    border-bottom: 1px solid var(--border-subtle);
    z-index: 1;
  }
  .projects-table td {
    padding: 7px 10px;
    border-bottom: 1px solid rgb(255 255 255 / 0.04);
    vertical-align: middle;
  }
  .projects-table tr.selected {
    background: rgb(56 189 248 / 0.04);
  }
  .check-cell {
    text-align: center;
  }
  .name-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .proj-name {
    font-size: 12px;
    color: var(--text-1);
    word-break: break-all;
  }
  .tag-small {
    font-size: 10px;
    color: var(--text-3);
  }
  .old-cell {
    color: var(--text-3);
    font-size: 11px;
  }
  .path-text {
    display: block;
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  .path-text.old {
    text-decoration: line-through;
    opacity: 0.75;
  }
  .new-cell {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .new-path-control {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .path-select,
  .path-input {
    flex: 1;
    min-width: 0;
  }
  .match-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-weight: 500;
  }
  .match-badge.ok {
    color: rgb(52 211 153);
  }
  .match-badge.none {
    color: var(--text-3);
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 32px;
    color: var(--text-3);
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 20px;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-surface-elevated, var(--bg-surface));
  }
  .ready-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: rgb(52 211 153);
    font-weight: 500;
  }
</style>
