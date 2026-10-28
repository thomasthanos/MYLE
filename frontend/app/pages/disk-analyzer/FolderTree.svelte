<script lang="ts">
  // The folder tree. Only the rows on screen (and a few more) are drawn, so a folder with 100,000 entries scrolls like one with ten.
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Cloud from "@lucide/svelte/icons/cloud";
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import Link2 from "@lucide/svelte/icons/link-2";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Lock from "@lucide/svelte/icons/lock";
  import ChevronsDown from "@lucide/svelte/icons/chevrons-down";
  import { contextActions } from "../../../lib/context-menu";
  import { formatSize, sizeOf, type NodeView, type SortKey } from "./api";
  import { itemActions } from "./actions";
  import { share, visibleRange, type Row } from "./rows";
  import { diskState as d } from "./state.svelte";

  const ROW = 30;
  const INDENT = 16;

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(400);

  const rows = $derived(d.rows);
  const range = $derived(visibleRange(scrollTop, height, ROW, rows.length));
  const visible = $derived(rows.slice(range.first, range.last));
  const focusedIndex = $derived(rows.findIndex((row) => row.type === "node" && row.node.id === d.focused));
  const date = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

  const columns: { key: SortKey; label: string }[] = [
    { key: "name", label: "Name" },
    { key: "size", label: "Size" },
    { key: "files", label: "Files" },
    { key: "modified", label: "Modified" },
  ];

  // Brings a row asked for (from the treemap, say) into view once it is listed, then lets the request go, so later changes don't scroll back.
  $effect(() => {
    const wanted = d.scrollTo;
    if (!wanted || !viewport) return;
    const index = rows.findIndex((row) => row.type === "node" && row.node.id === wanted.id);
    if (index < 0) return;
    bringIntoView(index);
    d.scrollTo = null;
  });

  function bringIntoView(index: number) {
    if (!viewport) return;
    const top = index * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + height) viewport.scrollTop = top + ROW - height;
  }

  function nodeAt(index: number): NodeView | null {
    const row = rows[index];
    return row?.type === "node" ? row.node : null;
  }

  function moveTo(index: number, extend: boolean) {
    const node = nodeAt(index);
    if (!node) return;
    d.select(node.id, extend ? "range" : "only");
    bringIntoView(index);
  }

  /** The next node row from `from` in `step`'s direction. */
  function nextNode(from: number, step: number): number {
    for (let i = from + step; i >= 0 && i < rows.length; i += step) if (rows[i].type === "node") return i;
    return -1;
  }

  function onKeydown(e: KeyboardEvent) {
    const at = focusedIndex;
    const node = at >= 0 ? nodeAt(at) : null;
    const row = at >= 0 ? (rows[at] as Extract<Row, { type: "node" }>) : null;
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const next = at < 0 ? nextNode(-1, 1) : nextNode(at, e.key === "ArrowDown" ? 1 : -1);
        if (next >= 0) moveTo(next, e.shiftKey);
        return;
      }
      case "Home":
      case "End":
        e.preventDefault();
        moveTo(e.key === "Home" ? nextNode(-1, 1) : nextNode(rows.length, -1), e.shiftKey);
        return;
      case "ArrowRight":
        if (!node || node.kind !== "dir") return;
        e.preventDefault();
        if (!row?.expanded) d.toggle(node.id, true);
        else if (nextNode(at, 1) >= 0 && rows[nextNode(at, 1)].depth > row.depth) moveTo(nextNode(at, 1), false);
        return;
      case "ArrowLeft": {
        if (!row) return;
        e.preventDefault();
        if (row.expanded) d.toggle(row.node.id, false);
        else {
          const parent = rows.findIndex((r) => r.type === "node" && r.node.id === row.parent);
          if (parent >= 0) moveTo(parent, false);
        }
        return;
      }
      case "Enter":
        if (node?.kind === "dir") {
          e.preventDefault();
          void d.zoomTo(node.id);
        }
        return;
      case " ":
        if (node) {
          e.preventDefault();
          d.select(node.id, "toggle");
        }
        return;
      case "Delete":
        e.preventDefault();
        void d.recycle();
        return;
    }
  }

  function onRowClick(e: MouseEvent, node: NodeView) {
    d.select(node.id, e.shiftKey ? "range" : e.ctrlKey || e.metaKey ? "toggle" : "only");
    viewport?.focus({ preventScroll: true });
  }

  function rowActions(target: HTMLElement) {
    const element = target.closest<HTMLElement>("[data-node]");
    const id = element ? Number(element.dataset.node) : d.focused;
    const node = rows.find((row) => row.type === "node" && row.node.id === id);
    if (id === null || !node || node.type !== "node") return [];
    return itemActions(id, { kind: node.node.kind, locked: node.node.protected }, "tree");
  }

  const kindIcon = { dir: Folder, file: File, link: Link2, cloud: Cloud };

  function tip(node: NodeView): string {
    const lines = [node.name];
    lines.push(`Size: ${formatSize(node.size)} (${node.size.toLocaleString()} bytes)`);
    lines.push(`On disk: ${formatSize(node.alloc)} (${node.alloc.toLocaleString()} bytes)`);
    if (node.kind === "link") lines.push("A link to somewhere else: not followed, nothing counted.");
    if (node.kind === "cloud") lines.push("A cloud file: counted, never downloaded.");
    if (node.inaccessible) lines.push("This folder could not be read.");
    if (node.hardLink) lines.push("Another name of a file counted where it is found first (a hard link): it takes no more space.");
    if (node.protected && node.reason) lines.push(`Stays where it is: ${node.reason}`);
    if (node.caution) lines.push(`Think twice: ${node.caution}`);
    return lines.join("\n");
  }
</script>

<div class="tree" role="presentation">
  <div class="head" role="presentation">
    {#each columns as column (column.key)}
      <button
        type="button"
        class="col {column.key}"
        class:active={d.sort === column.key}
        aria-pressed={d.sort === column.key}
        title="Sort by {column.label.toLowerCase()}"
        onclick={() => d.setSort(column.key)}>{column.label}</button>
      {#if column.key === "name"}<span class="col share" aria-hidden="true"></span>{/if}
    {/each}
  </div>
  <div
    class="viewport"
    bind:this={viewport}
    bind:clientHeight={height}
    onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
    onkeydown={onKeydown}
    tabindex="0"
    role="tree"
    aria-label="Folders, largest first"
    aria-multiselectable="true"
    aria-activedescendant={focusedIndex >= 0 ? `da-row-${rows[focusedIndex].key}` : undefined}
    {@attach contextActions(rowActions)}
  >
    <div class="spacer" style:height="{rows.length * ROW}px">
      {#each visible as row, i (row.key)}
        {@const index = range.first + i}
        {#if row.type === "node"}
          {@const node = row.node}
          {@const Icon = kindIcon[node.kind]}
          {@const part = share(sizeOf(node, d.sizeMode), row.parentSize)}
          <!-- svelte-ignore a11y_click_events_have_key_events (the list handles the keys, aria-activedescendant points at the row) -->
          <div
            class="row"
            class:selected={d.selection.has(node.id)}
            class:focused={d.focused === node.id}
            class:dim={node.kind === "link"}
            style:transform="translateY({index * ROW}px)"
            id="da-row-{row.key}"
            role="treeitem"
            aria-level={row.depth + 1}
            aria-expanded={node.kind === "dir" && node.hasChildren ? row.expanded : undefined}
            aria-selected={d.selection.has(node.id)}
            data-node={node.id}
            tabindex="-1"
            onclick={(e) => onRowClick(e, node)}
            ondblclick={() => node.kind === "dir" && d.zoomTo(node.id)}
          >
            <span class="name" style:padding-left="{row.depth * INDENT}px" title={tip(node)}>
              {#if node.kind === "dir" && node.hasChildren}
                <button
                  type="button"
                  class="twisty"
                  class:open={row.expanded}
                  tabindex="-1"
                  aria-label={row.expanded ? "Close" : "Open"}
                  onclick={(e) => {
                    e.stopPropagation();
                    d.toggle(node.id);
                  }}><ChevronRight size={14} /></button>
              {:else}
                <span class="twisty-space"></span>
              {/if}
              <span class="icon {node.kind}"><Icon size={15} /></span>
              <span class="label">{node.name}</span>
              {#if node.protected && node.kind !== "link" && row.depth > 0}<span class="lock" aria-label="Protected"><Lock size={12} /></span>{/if}
              {#if node.inaccessible}<span class="unread">not readable</span>{/if}
            </span>
            <span class="share" aria-hidden="true"><span class="bar" style:width="{part * 100}%"></span><em>{Math.round(part * 100)}%</em></span>
            <span class="size" title="Size: {formatSize(node.size)} · On disk: {formatSize(node.alloc)}">{formatSize(sizeOf(node, d.sizeMode))}</span>
            <span class="files">{node.kind === "dir" ? node.files.toLocaleString() : ""}</span>
            <span class="modified">{node.modified ? date.format(node.modified * 1000) : ""}</span>
          </div>
        {:else if row.type === "more"}
          <div class="row more" style:transform="translateY({index * ROW}px)" role="presentation">
            <span class="name" style:padding-left="{row.depth * INDENT}px">
              <span class="twisty-space"></span>
              <button type="button" class="btn ghost small" onclick={() => d.loadChildren(row.parent, true)}>
                <ChevronsDown size={14} />
                {row.count.toLocaleString()} smaller item{row.count === 1 ? "" : "s"} · {formatSize(row.size)} · show {Math.min(500, row.count).toLocaleString()} more
              </button>
            </span>
          </div>
        {:else}
          <div class="row loading" style:transform="translateY({index * ROW}px)" role="presentation">
            <span class="name" style:padding-left="{row.depth * INDENT}px"><span class="twisty-space"></span><LoaderCircle size={14} class="spin" /> Loading…</span>
          </div>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .tree { --row: 30px; --cols: minmax(0, 1fr) 92px 78px 64px 104px; display: flex; flex-direction: column; flex: 1; min-height: 0; container-type: inline-size; }
  .head, .row { display: grid; grid-template-columns: var(--cols); align-items: center; gap: 8px; padding: 0 10px; }
  .head { flex: none; height: 30px; border-bottom: 1px solid var(--glass-border); color: var(--text-3); font-size: var(--fluid-xs); }
  .head .col { justify-self: end; padding: 3px 4px; border-radius: 6px; color: inherit; font: inherit; font-weight: 600; }
  .head .col.name { justify-self: start; }
  .head .col:hover { color: var(--text-1); background: var(--hover); }
  .head .col.active { color: var(--text-1); }
  .viewport { position: relative; flex: 1; min-height: 0; overflow: auto; outline: none; border-radius: 0 0 var(--radius-md) var(--radius-md); }
  .viewport:focus-visible { box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.45); }
  .spacer { position: relative; }
  .row { position: absolute; inset: 0 0 auto; height: var(--row); color: var(--text-2); font-size: var(--fluid-xs); cursor: default; user-select: none; }
  .row:hover { background: var(--hover); }
  .row.selected { background: var(--selected); color: var(--text-1); }
  .viewport:focus-within .row.focused { box-shadow: inset 2px 0 0 var(--accent); }
  .row.dim { color: var(--text-3); }
  .name { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .label { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--text-1); }
  .row.dim .label { color: var(--text-3); }
  .twisty, .twisty-space { display: grid; place-items: center; flex: none; width: 18px; height: 18px; border-radius: 5px; color: var(--text-3); }
  .twisty :global(svg) { transition: transform var(--dur-fast) var(--ease-out); }
  .twisty.open :global(svg) { transform: rotate(90deg); }
  .twisty:hover { background: var(--hover); color: var(--text-1); }
  .icon { display: grid; flex: none; color: var(--text-3); }
  .icon.dir { color: rgb(var(--accent-soft-rgb)); }
  .icon.cloud { color: #7cc4e8; }
  .lock { display: grid; flex: none; color: var(--text-3); }
  .unread { flex: none; padding: 0 6px; border-radius: 999px; background: rgb(229 72 77 / 0.14); color: #ffb4b0; font-size: 10.5px; }
  .share { position: relative; display: flex; align-items: center; height: 14px; border-radius: 4px; background: rgb(255 255 255 / 0.05); overflow: hidden; }
  .share .bar { position: absolute; inset: 0 auto 0 0; border-radius: inherit; background: rgb(var(--accent-rgb) / 0.55); }
  .share em { position: relative; margin-left: auto; padding-right: 5px; color: var(--text-2); font-size: 10.5px; font-style: normal; font-variant-numeric: tabular-nums; }
  .size, .files, .modified { justify-self: end; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .size { color: var(--text-1); }
  .files, .modified { color: var(--text-3); }
  .row.more, .row.loading { color: var(--text-3); }
  .row.more .btn { color: var(--text-2); font-weight: 500; }
  .row.more:hover, .row.loading:hover { background: transparent; }
  /* The container can't style itself: the narrow columns go on the rows. */
  @container (max-width: 560px) {
    .head, .row { --cols: minmax(0, 1fr) 70px 74px; }
    .head .col.files, .head .col.modified, .row .files, .row .modified { display: none; }
  }
</style>
