<script lang="ts">
  // "What will be backed up": the project as a tree with sizes, what is left
  // out marked with the rule that leaves it out, and a count per rule.
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderX from "@lucide/svelte/icons/folder-x";
  import Search from "@lucide/svelte/icons/search";
  import type { Preview, PreviewNode, RuleKind } from "./api";
  import { formatBytes } from "./state.svelte";

  let { preview }: { preview: Preview } = $props();

  const kindNames: Record<RuleKind, string> = {
    pattern: "Pattern",
    detected: "Detected build / cache folder",
    gitignore: "From .gitignore",
    internal: "MYLE's own file",
  };

  type Show = "all" | "kept" | "left";
  let show = $state<Show>("all");
  let query = $state("");
  let rule = $state<string | null>(null);
  /** Folders folded away; small projects start open, big ones folded. */
  let collapsed = $state(new Set<string>());
  const STEP = 1500;
  let limit = $state(STEP);

  $effect.pre(() => {
    const tree = preview.tree;
    collapsed = new Set(tree.length > 80 ? tree.filter((node) => node.isDir && !node.rule).map((node) => node.path) : []);
    limit = STEP;
  });

  const leftOut = (node: PreviewNode) => node.rule !== null;
  const flat = $derived(!!query.trim() || rule !== null || show !== "all");

  const lines = $derived.by(() => {
    const nodes = preview.tree;
    if (flat) {
      const needle = query.trim().toLowerCase();
      return nodes.filter(
        (node) =>
          (show === "all" || (show === "left") === leftOut(node)) &&
          (rule === null || node.rule === rule) &&
          (!needle || node.path.toLowerCase().includes(needle)),
      );
    }
    const out: PreviewNode[] = [];
    let hideBelow = Infinity;
    for (const node of nodes) {
      if (node.depth > hideBelow) continue;
      hideBelow = Infinity;
      out.push(node);
      if (node.isDir && !node.rule && collapsed.has(node.path)) hideBelow = node.depth;
    }
    return out;
  });

  function toggle(path: string) {
    const next = new Set(collapsed);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    collapsed = next;
  }

  function setAll(open: boolean) {
    collapsed = new Set(open ? [] : preview.tree.filter((node) => node.isDir && !node.rule).map((node) => node.path));
  }

  function onRowKey(event: KeyboardEvent, node: PreviewNode) {
    if (!node.isDir || node.rule) return;
    const open = !collapsed.has(node.path);
    if ((event.key === "ArrowRight" && !open) || (event.key === "ArrowLeft" && open)) {
      event.preventDefault();
      toggle(node.path);
    }
  }

  const plural = (count: number, one: string, many: string) => `${count.toLocaleString()} ${count === 1 ? one : many}`;
</script>

<div class="preview surface">
  <p class="summary">
    <strong>{preview.fileCount.toLocaleString()}</strong> files · <strong>{formatBytes(preview.totalBytes)}</strong> backed up ·
    <strong>{preview.excludedTotal.toLocaleString()}</strong> left out
    {#if preview.envFiles.length}· includes <span class="env selectable">{preview.envFiles.join(", ")}</span>{/if}
  </p>
  {#if preview.skipped.length}
    <p class="skipped">
      Can't be zipped: {preview.skipped.map((item) => `${item.path} (${item.reason})`).join(", ")}
    </p>
  {/if}

  {#if preview.byRule.length}
    <div class="rules" aria-label="Left out, by rule">
      {#each preview.byRule as count (count.rule)}
        <button class="rule-chip {count.kind}" class:active={rule === count.rule} aria-pressed={rule === count.rule}
          title={`${kindNames[count.kind]}. Click to list what it leaves out.`}
          onclick={() => { rule = rule === count.rule ? null : count.rule; limit = STEP; }}>
          <span class="rule-name">{count.rule}</span>
          <span class="rule-count">
            {[count.folders ? plural(count.folders, "folder", "folders") : "", count.files ? plural(count.files, "file", "files") : ""]
              .filter(Boolean)
              .join(", ")}
          </span>
        </button>
      {/each}
    </div>
  {/if}

  {#if preview.tree.length}
    <div class="tree-bar">
      <span class="segmented" role="group" aria-label="Show">
        {#each [["all", "Everything"], ["kept", "Backed up"], ["left", "Left out"]] as const as [value, text] (value)}
          <button class:active={show === value} aria-pressed={show === value} onclick={() => { show = value; limit = STEP; }}>{text}</button>
        {/each}
      </span>
      <label class="search">
        <Search size={12} />
        <input class="input" type="search" placeholder="Find a file or folder" bind:value={query} aria-label="Find in the preview" />
      </label>
      {#if !flat}
        <button class="btn small ghost" onclick={() => setAll(true)}>Expand all</button>
        <button class="btn small ghost" onclick={() => setAll(false)}>Collapse all</button>
      {/if}
    </div>

    <ul class="tree" role="tree" aria-label="What will be backed up">
      {#each lines.slice(0, limit) as node (node.path)}
        {@const folder = node.isDir && !node.rule}
        {@const open = folder && !collapsed.has(node.path)}
        <li role="treeitem" aria-selected="false" aria-expanded={folder ? open : undefined} class:left={!!node.rule}>
          <div class="row" style:padding-left={flat ? "4px" : `${4 + node.depth * 14}px`}>
            {#if folder && !flat}
              <button class="twisty" class:open aria-label={open ? `Fold ${node.name}` : `Unfold ${node.name}`}
                onclick={() => toggle(node.path)} onkeydown={(event) => onRowKey(event, node)}>
                <ChevronRight size={12} />
              </button>
            {:else}
              <span class="twisty-space"></span>
            {/if}
            <span class="icon">
              {#if node.rule && node.isDir}<FolderX size={12} />{:else if node.isDir}<Folder size={12} />{:else}<File size={12} />{/if}
            </span>
            <span class="name selectable" title={node.path}>{flat ? node.path : node.name}{node.isDir ? "/" : ""}</span>
            {#if node.rule}
              <span class="tag {node.kind}" title={node.kind ? kindNames[node.kind] : undefined}>{node.rule}</span>
            {:else if node.size !== null}
              <span class="meta">
                {#if node.isDir}{plural(node.files, "file", "files")} · {/if}{formatBytes(node.size)}
                {#if node.hiddenFiles}<span class="more-files">· {node.hiddenFiles.toLocaleString()} not listed</span>{/if}
              </span>
            {/if}
          </div>
        </li>
      {:else}
        <li class="empty">Nothing matches.</li>
      {/each}
    </ul>
    {#if lines.length > limit}
      <button class="btn small" onclick={() => (limit += STEP * 2)}>Show more ({(lines.length - limit).toLocaleString()} left)</button>
    {/if}
    {#if preview.treeHiddenFiles && !flat}
      <p class="more">{plural(preview.treeHiddenFiles, "more file", "more files")} in the project folder are backed up but not listed (the list is kept short).</p>
    {/if}
  {/if}
</div>

<style>
  .preview { display: grid; gap: 7px; width: 100%; padding: 9px 10px; }
  .summary { color: var(--text-2); font-size: 11.5px; line-height: 1.5; overflow-wrap: anywhere; }
  .summary strong { color: var(--text-1); font-variant-numeric: tabular-nums; }
  .env { font-family: var(--font-mono); font-size: 10.5px; }
  .skipped { color: rgb(245 188 95 / 0.85); font-size: 11px; overflow-wrap: anywhere; }
  .rules { display: flex; flex-wrap: wrap; gap: 4px; max-height: 92px; overflow: auto; }
  .rule-chip {
    display: inline-flex; align-items: baseline; gap: 5px; padding: 2px 7px; border: 1px solid rgb(255 255 255 / 0.06);
    border-radius: 6px; background: rgb(255 255 255 / 0.03); color: var(--text-2); font-size: 10.5px;
  }
  .rule-chip:hover { border-color: rgb(var(--accent-rgb) / 0.3); }
  .rule-chip.active { border-color: rgb(var(--accent-rgb) / 0.45); background: var(--selected); }
  .rule-name { font-family: var(--font-mono); }
  .rule-chip.detected .rule-name { color: rgb(169 179 255 / 0.95); font-family: inherit; }
  .rule-chip.gitignore .rule-name { color: rgb(245 188 95 / 0.9); }
  .rule-count { color: var(--text-3); font-size: 10px; }
  .tree-bar { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
  .segmented { display: inline-flex; padding: 2px; border-radius: 7px; background: rgb(0 0 0 / 0.16); }
  .segmented button { padding: 2px 8px; border-radius: 5px; color: var(--text-3); font-size: 10.5px; }
  .segmented button.active { background: var(--selected); color: var(--text-1); }
  .search { display: flex; flex: 1; align-items: center; gap: 5px; min-width: 140px; color: var(--text-3); }
  .search input { width: 100%; height: 26px; font-size: 11px; }
  .tree { display: grid; max-height: 300px; margin: 0; padding: 2px 0; overflow: auto; list-style: none; border-radius: 7px; background: rgb(0 0 0 / 0.1); }
  .row { display: flex; align-items: center; gap: 4px; min-width: 0; min-height: 21px; padding-right: 6px; font-size: 11px; }
  .row:hover { background: rgb(255 255 255 / 0.03); }
  .twisty, .twisty-space { display: grid; flex: none; place-items: center; width: 16px; height: 16px; border-radius: 4px; color: var(--text-3); }
  .twisty :global(svg) { transition: transform 120ms; }
  .twisty.open :global(svg) { transform: rotate(90deg); }
  .twisty:hover { background: rgb(255 255 255 / 0.06); }
  .icon { display: grid; flex: none; place-items: center; color: var(--text-3); }
  .name { min-width: 0; overflow: hidden; color: var(--text-2); font-family: var(--font-mono); font-size: 10.5px; white-space: nowrap; text-overflow: ellipsis; }
  li.left .name { color: var(--text-3); text-decoration: line-through; text-decoration-color: rgb(255 255 255 / 0.18); }
  li.left .icon { color: rgb(255 145 145 / 0.6); }
  .meta { flex: none; margin-left: auto; color: var(--text-3); font-size: 10px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .more-files { opacity: 0.8; }
  .tag { flex: none; margin-left: auto; padding: 0 5px; border-radius: 4px; background: rgb(255 255 255 / 0.05); color: var(--text-3); font-family: var(--font-mono); font-size: 10px; white-space: nowrap; }
  .tag.detected { background: rgb(var(--accent-rgb) / 0.1); color: rgb(169 179 255 / 0.95); font-family: inherit; }
  .tag.gitignore { background: rgb(245 176 65 / 0.08); color: rgb(245 188 95 / 0.9); }
  .empty, .more { padding: 6px 8px; color: var(--text-3); font-size: 10.5px; }
</style>
