<script lang="ts">
  // The preview of one changed file in a comparison: a text diff (side by
  // side or unified), old and new images, or sizes and SHA-256 of binaries.
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Code from "@lucide/svelte/icons/code";
  import Image from "@lucide/svelte/icons/image";
  import type { Change, DiffRow, FileDiff, SideInfo } from "./api";
  import { formatBytes } from "./state.svelte";

  let {
    change,
    diff,
    mode,
  }: { change: Change; diff: FileDiff; mode: "split" | "unified" } = $props();

  /** Rows drawn at first; more on request (a diff can have 20,000). */
  const STEP = 1500;
  let limit = $state(STEP);
  let svgSource = $state(false);

  const rows = $derived<DiffRow[]>(diff.kind === "text" ? diff.rows : diff.kind === "image" ? (diff.rows ?? []) : []);
  /** A file only on one side is shown whole, in one column. */
  const single = $derived(change.status !== "modified");
  const showRows = $derived(diff.kind === "text" || (diff.kind === "image" && svgSource));

  interface Line {
    sign: " " | "-" | "+" | "…";
    no: number | null;
    oldNo: number | null;
    text: string;
    kind: "same" | "removed" | "added" | "gap";
  }

  /** Unified lines: a changed row becomes a removed and an added line. */
  const unified = $derived.by(() => {
    const out: Line[] = [];
    for (const row of rows) {
      if (row.kind === "gap") out.push({ sign: "…", no: null, oldNo: null, text: "", kind: "gap" });
      else if (row.kind === "same")
        out.push({ sign: " ", no: row.newLine, oldNo: row.oldLine, text: row.newText ?? row.oldText ?? "", kind: "same" });
      else {
        if (row.oldText !== null && row.kind !== "added")
          out.push({ sign: "-", no: null, oldNo: row.oldLine, text: row.oldText, kind: "removed" });
        if (row.newText !== null && row.kind !== "removed")
          out.push({ sign: "+", no: row.newLine, oldNo: null, text: row.newText, kind: "added" });
      }
    }
    return out;
  });

  const total = $derived(mode === "unified" || single ? unified.length : rows.length);
  /** Small images (icons) are scaled up with sharp pixels so a change can be seen. */
  function enlarge(event: Event) {
    const image = event.currentTarget as HTMLImageElement;
    const longest = Math.max(image.naturalWidth, image.naturalHeight);
    if (!longest || longest >= 128) return;
    const scale = Math.max(1, Math.floor(128 / longest));
    image.style.width = `${image.naturalWidth * scale}px`;
    image.style.imageRendering = "pixelated";
  }

  const size = (info: SideInfo | null) => (info ? formatBytes(info.size) : "—");
  const sameHash = $derived(
    diff.kind === "binary" && diff.old?.sha256 && diff.new?.sha256 ? diff.old.sha256 === diff.new.sha256 : null,
  );
</script>

{#if diff.kind === "text"}
  {#if diff.encoding || diff.truncated || diff.lineEndingsDiffer}
    <p class="notes">
      {#if diff.encoding}<span>Encoding: {diff.encoding}</span>{/if}
      {#if diff.lineEndingsDiffer}<span>Line endings differ (CRLF / LF)</span>{/if}
      {#if diff.truncated}<span class="warn"><CircleAlert size={12} /> Only the first {diff.rows.length.toLocaleString()} rows are shown</span>{/if}
    </p>
  {/if}
  {#if diff.identical}
    <p class="placeholder">
      {diff.lineEndingsDiffer ? "Same text: only the line endings (CRLF / LF) differ." : "Same text; only the encoding or the bytes differ."}
    </p>
  {/if}
{:else if diff.kind === "image"}
  <div class="image-bar">
    {#if diff.rows}
      <button class="chip" class:active={!svgSource} onclick={() => (svgSource = false)}><Image size={12} /> Image</button>
      <button class="chip" class:active={svgSource} onclick={() => (svgSource = true)}><Code size={12} /> Source</button>
    {/if}
  </div>
  {#if !svgSource}
    <div class="images" class:single>
      {#each [{ side: "Before", url: diff.old, info: diff.oldInfo }, { side: "After", url: diff.new, info: diff.newInfo }] as item (item.side)}
        {#if !single || item.info}
          <figure>
            <figcaption>{single ? (change.status === "added" ? "Added" : "Deleted") : item.side} · {size(item.info)}</figcaption>
            <div class="canvas">
              {#if item.url}
                <img src={item.url} alt={`${item.side}: ${change.path}`} onload={enlarge} />
              {:else if item.info}
                <span>Too large to show ({formatBytes(item.info.size)})</span>
              {:else}
                <span>Not in this side</span>
              {/if}
            </div>
          </figure>
        {/if}
      {/each}
    </div>
  {/if}
{:else}
  <div class="binary">
    {#if diff.tooLarge}<p class="notes"><span>Too large to show line by line (over 2 MB): compared by size and SHA-256.</span></p>{/if}
    <table>
      <thead><tr><th></th><th>Size</th><th>SHA-256</th></tr></thead>
      <tbody>
        <tr><th>Before</th><td>{size(diff.old)}</td><td class="hash selectable">{diff.old ? (diff.old.sha256 ?? "too large to hash") : "—"}</td></tr>
        <tr><th>After</th><td>{size(diff.new)}</td><td class="hash selectable">{diff.new ? (diff.new.sha256 ?? "too large to hash") : "—"}</td></tr>
      </tbody>
    </table>
    {#if sameHash !== null}
      <p class="placeholder">{sameHash ? "Same content (the SHA-256 matches)." : "Different content."}</p>
    {/if}
  </div>
{/if}

{#if showRows && !(diff.kind === "text" && diff.identical)}
  {#if mode === "unified" || single}
    <div class="table" role="table" aria-label={`Lines of ${change.path}`}>
      {#each unified.slice(0, limit) as line, index (index)}
        {#if line.kind === "gap"}
          <div class="gap" role="row"><span role="cell">⋯ unchanged lines</span></div>
        {:else}
          <div class="uline {line.kind}" class:single role="row">
            {#if !single}<span class="no" role="cell">{line.oldNo ?? ""}</span>{/if}
            <span class="no" role="cell">{single ? (line.no ?? line.oldNo ?? "") : (line.no ?? "")}</span>
            {#if !single}<span class="sign" role="cell">{line.sign}</span>{/if}
            <span class="text" role="cell">{line.text}</span>
          </div>
        {/if}
      {/each}
    </div>
  {:else}
    <div class="table" role="table" aria-label={`Changes in ${change.path}`}>
      {#each rows.slice(0, limit) as row, index (index)}
        {#if row.kind === "gap"}
          <div class="gap" role="row"><span role="cell">⋯ unchanged lines</span></div>
        {:else}
          <div class="line {row.kind}" role="row">
            <span class="no" role="cell">{row.oldLine ?? ""}</span>
            <span class="text old" role="cell">{row.oldText ?? ""}</span>
            <span class="no" role="cell">{row.newLine ?? ""}</span>
            <span class="text new" role="cell">{row.newText ?? ""}</span>
          </div>
        {/if}
      {/each}
    </div>
  {/if}
  {#if total > limit}
    <button class="btn small more" onclick={() => (limit += STEP * 2)}>
      Show more ({(total - limit).toLocaleString()} lines left)
    </button>
  {/if}
{/if}

<style>
  .placeholder { display: flex; align-items: center; gap: 6px; padding: 12px 14px; color: var(--text-3); font-size: 12px; }
  .notes { display: flex; flex-wrap: wrap; gap: 6px; padding: 6px 10px 0; }
  .notes span { display: inline-flex; align-items: center; gap: 4px; padding: 1px 6px; border-radius: 5px; background: rgb(255 255 255 / 0.05); color: var(--text-3); font-size: 10.5px; }
  .notes .warn { background: rgb(245 176 65 / 0.1); color: rgb(245 188 95 / 0.9); }
  .table { flex: 1; min-height: 0; overflow: auto; font-family: var(--font-mono); font-size: 11px; line-height: 1.55; }
  .line { display: grid; grid-template-columns: 44px minmax(0, 1fr) 44px minmax(0, 1fr); min-width: 0; }
  .uline { display: grid; grid-template-columns: 44px 44px 16px minmax(0, 1fr); min-width: 0; }
  .uline.single { grid-template-columns: 50px minmax(0, 1fr); }
  .no { padding: 0 6px; color: var(--text-3); text-align: right; user-select: none; opacity: 0.7; }
  .sign { color: var(--text-3); user-select: none; text-align: center; }
  .text { padding: 0 8px; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--text-2); user-select: text; }
  .text.old { border-right: 1px solid rgb(255 255 255 / 0.05); }
  .line.removed .old, .line.changed .old, .uline.removed { background: rgb(229 72 77 / 0.12); }
  .line.removed .old, .line.changed .old, .uline.removed .text { color: rgb(255 190 190); }
  .line.added .new, .line.changed .new, .uline.added { background: rgb(62 207 142 / 0.11); }
  .line.added .new, .line.changed .new, .uline.added .text { color: rgb(190 240 215); }
  .uline.single.removed, .uline.single.added { background: none; }
  .uline.single .text { color: var(--text-2); }
  .gap { padding: 1px 10px; background: rgb(var(--accent-rgb) / 0.05); color: var(--text-3); font-size: 10.5px; }
  .more { align-self: center; margin: 8px; }
  .image-bar { display: flex; gap: 6px; padding: 8px 10px 0; }
  .image-bar:empty { display: none; }
  .images { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; flex: 1; min-height: 0; padding: 10px; overflow: auto; }
  .images.single { grid-template-columns: minmax(0, 1fr); }
  figure { display: flex; flex-direction: column; gap: 6px; min-width: 0; margin: 0; }
  figcaption { color: var(--text-3); font-size: 11px; }
  .canvas {
    display: grid; place-items: center; flex: 1; min-height: 160px; padding: 10px; border-radius: 8px;
    border: 1px solid rgb(255 255 255 / 0.06); color: var(--text-3); font-size: 11.5px;
    background: repeating-conic-gradient(rgb(255 255 255 / 0.05) 0% 25%, transparent 0% 50%) 50% / 16px 16px;
  }
  .canvas img { max-width: 100%; max-height: 52vh; object-fit: contain; image-rendering: auto; }
  .binary { display: grid; gap: 4px; padding: 6px 0; }
  table { margin: 4px 10px; border-collapse: collapse; font-size: 11.5px; }
  th, td { padding: 4px 8px; text-align: left; vertical-align: top; }
  thead th { color: var(--text-3); font-size: 10.5px; font-weight: 500; }
  tbody th { color: var(--text-2); font-weight: 600; }
  td { color: var(--text-2); }
  .hash { font-family: var(--font-mono); font-size: 10.5px; overflow-wrap: anywhere; }
</style>
