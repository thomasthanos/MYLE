<script lang="ts">
  // The treemap, drawn on a canvas: thousands of rectangles are too many for the DOM. The map itself is drawn on one canvas, only when the data or the size changes; hover, selection and the kind in focus go on a second canvas above it, so moving the pointer never redraws the map. The tooltip and the right-click menu find the item under the pointer in the layout's flat arrays. The look follows the rest of the app: flat rounded tiles in one calm colour per kind of file, folders as quiet panels with their name on top, and the accent colour for hover and selection.
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Type from "@lucide/svelte/icons/type";
  import { contextActions } from "../../../lib/context-menu";
  import { settings } from "../../../lib/settings.svelte";
  import { pointerTip } from "../../../lib/tooltip.svelte";
  import { formatSize, sizeOf, SMALLER_ID, type TreemapPayload } from "./api";
  import { itemActions } from "./actions";
  import { CATEGORIES, categoryColor, categoryLabel, categoryOf, folderColor, type Category } from "./palette";
  import { diskState as d } from "./state.svelte";
  import { FOLDER, hitTest, layoutTreemap, SMALLER, type Layout } from "./treemap";

  const FILE = 1;
  const LINK = 2;
  const CLOUD = 3;

  let host = $state<HTMLDivElement>();
  let base = $state<HTMLCanvasElement>();
  let overlay = $state<HTMLCanvasElement>();
  let width = 0;
  let height = 0;
  let ratio = 1;
  let payload: TreemapPayload | null = null;
  let layout: Layout | null = null;
  /** Folders whose children were drawn (the rest are drawn as one block). */
  let opened = new Uint8Array(0);
  /** The kind of each of the payload's extensions. */
  let kinds: Category[] = [];
  let folderColors: string[] = [];
  let indexOf = new Map<number, number>();
  let hover = -1;
  let frame = 0;
  let wantMap = false;
  let wantMarks = false;
  let colors = {
    background: "#121212",
    accent: "107 133 255",
    text1: "#f3f3f3",
    text2: "#c8c8c8",
    text3: "#969696",
    font: "sans-serif",
    navy: false,
  };

  let showLabels = $state(true);
  /** The kind of file in focus from the legend: the rest is dimmed. */
  let focus = $state<Category | null>(null);

  const label = $derived.by(() => {
    const map = d.treemap;
    const where = d.trail.at(-1)?.name ?? "";
    if (!map) return `Treemap of ${where}`;
    const largest = map.ids
      .map((_, i) => i)
      .filter((i) => map.parent[i] === 0 && map.kind[i] !== SMALLER)
      .slice(0, 3)
      .map((i) => `${map.names[i]} (${formatSize(map.size[i])})`);
    return `Treemap of ${where}, ${formatSize(map.size[0] ?? 0)}. Largest: ${largest.join(", ") || "nothing"}. The folder list has the same items.`;
  });

  /** What each kind of file takes in the folder shown, largest first. */
  const legend = $derived.by(() => {
    const totals = new Map<Category, number>();
    for (const stat of d.extensions ?? []) {
      const kind = stat.other ? "other" : categoryOf(stat.ext);
      totals.set(kind, (totals.get(kind) ?? 0) + sizeOf(stat, d.sizeMode));
    }
    return CATEGORIES.filter((c) => (totals.get(c.id) ?? 0) > 0)
      .map((c) => ({ ...c, size: totals.get(c.id) ?? 0, color: categoryColor(c.id, settings.dark) }))
      .sort((a, b) => b.size - a.size);
  });

  function toggleFocus(kind: Category) {
    focus = focus === kind ? null : kind;
    schedule(false);
  }

  function schedule(map: boolean) {
    wantMap ||= map;
    wantMarks = true;
    if (!frame) frame = requestAnimationFrame(render);
  }

  function render() {
    frame = 0;
    if (wantMap) {
      relayout();
      drawMap();
      wantMap = false;
    }
    if (wantMarks) {
      drawMarks();
      wantMarks = false;
    }
  }

  function readColors() {
    if (!host) return;
    const style = getComputedStyle(host);
    const read = (name: string, fallback: string) => style.getPropertyValue(name).trim() || fallback;
    colors = {
      background: read("--input-fill", "#121212"),
      accent: read("--accent-rgb", "107 133 255"),
      text1: read("--text-1", "#f3f3f3"),
      text2: read("--text-2", "#c8c8c8"),
      text3: read("--text-3", "#969696"),
      font: style.fontFamily || "sans-serif",
      navy: document.documentElement.classList.contains("dark"),
    };
  }

  function measure() {
    if (!host || !base || !overlay) return;
    const box = host.getBoundingClientRect();
    width = Math.max(0, Math.floor(box.width));
    height = Math.max(0, Math.floor(box.height));
    ratio = window.devicePixelRatio || 1;
    for (const canvas of [base, overlay]) {
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
    }
    d.setArea(width, height);
    schedule(true);
  }

  function relayout() {
    layout = payload && width > 0 && height > 0 ? layoutTreemap(payload, width, height) : null;
    opened = new Uint8Array(payload?.ids.length ?? 0);
    kinds = payload?.exts.map(categoryOf) ?? [];
    folderColors = payload?.names.map((name, i) => payload?.kind[i] === FOLDER ? folderColor(name, colors.navy) : "") ?? [];
    indexOf = new Map();
    if (!layout || !payload) return;
    for (const i of layout.order) {
      opened[payload.parent[i]] = 1;
      indexOf.set(payload.ids[i], i);
    }
  }

  /** Rounder corners on bigger tiles; none on the smallest. */
  function radiusOf(w: number, h: number): number {
    const side = Math.min(w, h);
    return side >= 40 ? 6 : side >= 16 ? 4 : side >= 8 ? 2 : 0;
  }

  function tile(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, radius: number) {
    if (radius > 0) {
      ctx.beginPath();
      ctx.roundRect(x, y, w, h, radius);
      ctx.fill();
    } else {
      ctx.fillRect(x, y, w, h);
    }
  }

  function ring(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, radius: number, lineWidth: number) {
    const inset = lineWidth / 2;
    ctx.lineWidth = lineWidth;
    ctx.beginPath();
    ctx.roundRect(x + inset, y + inset, Math.max(0, w - lineWidth), Math.max(0, h - lineWidth), Math.max(0, radius - inset));
    ctx.stroke();
  }

  /** Text that fits in `room` pixels, shortened with "…" when it does not. */
  function fitted(ctx: CanvasRenderingContext2D, text: string, room: number): string | null {
    if (room < 14) return null;
    const full = ctx.measureText(text).width;
    if (full <= room) return text;
    for (let n = Math.floor((text.length * room) / full) - 1; n > 1; n--) {
      const short = `${text.slice(0, n)}…`;
      if (ctx.measureText(short).width <= room) return short;
    }
    return null;
  }

  /** Writes `text` at (x, y), shortened to `room`; returns the width written. */
  function write(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, room: number, color: string, weight = 700, size = 11.5): number {
    ctx.font = `${weight} ${size}px ${colors.font}`;
    const shown = fitted(ctx, text, room);
    if (!shown) return 0;
    ctx.fillStyle = color;
    ctx.fillText(shown, x, y);
    return ctx.measureText(shown).width;
  }

  function fillOf(i: number): string {
    if (!payload) return colors.text3;
    const kind = payload.kind[i];
    if (kind === FILE || kind === CLOUD) return categoryColor(kinds[payload.ext[i]] ?? "other", colors.navy);
    if (kind === FOLDER) return folderColors[i];
    return folderColors[payload.parent[i]] || categoryColor("other", colors.navy);
  }

  function drawMap() {
    const ctx = base?.getContext("2d", { alpha: false });
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.fillStyle = colors.background;
    ctx.fillRect(0, 0, width, height);
    if (!layout || !payload) return;
    const { rects, header, order } = layout;
    ctx.textBaseline = "middle";

    for (const i of order) {
      const x = rects[i * 4];
      const y = rects[i * 4 + 1];
      const w = rects[i * 4 + 2];
      const h = rects[i * 4 + 3];
      const kind = payload.kind[i];
      const radius = radiusOf(w, h);

      if (kind === FOLDER && opened[i]) {
        // Start from an opaque base so deeply nested tints do not accumulate into a muddy colour. A restrained accent separates each group.
        ctx.fillStyle = colors.background;
        tile(ctx, x, y, w, h, radius);
        ctx.fillStyle = `${folderColors[i]}29`;
        tile(ctx, x, y, w, h, radius);
        if (radius) {
          ctx.strokeStyle = `${folderColors[i]}80`;
          ring(ctx, x, y, w, h, radius, 1);
        }
        if (header[i]) {
          const mid = y + 3 + header[i] / 2;
          const room = w - 16;
          ctx.font = `600 10.5px ${colors.font}`;
          const size = formatSize(payload.size[i]);
          const sizeWidth = ctx.measureText(size).width;
          const showSize = room > sizeWidth + 48;
          write(ctx, payload.names[i], x + 8, mid, showSize ? room - sizeWidth - 10 : room, colors.text1, 600, 12);
          if (showSize) {
            ctx.textAlign = "right";
            write(ctx, size, x + w - 8, mid, sizeWidth + 1, colors.text3, 600, 10.5);
            ctx.textAlign = "left";
          }
        }
        continue;
      }

      // Small groups keep a visible fill and outline too. Folder accents distinguish groups; actual files keep their category's colour.
      const neutral = kind === FOLDER || kind === SMALLER || kind === LINK;
      if (neutral) {
        ctx.fillStyle = colors.background;
        tile(ctx, x, y, w, h, radius);
      }
      ctx.globalAlpha = kind === SMALLER || kind === LINK ? 0.24 : kind === FOLDER ? 0.34 : kind === CLOUD ? 0.65 : 0.9;
      ctx.fillStyle = fillOf(i);
      tile(ctx, x, y, w, h, radius);
      ctx.globalAlpha = 1;
      if (neutral && w >= 4 && h >= 4) {
        ctx.strokeStyle = `${fillOf(i)}80`;
        ring(ctx, x, y, w, h, radius, 1);
      }
      if (!neutral && w >= 12 && h >= 12) {
        // A faint light edge along the top.
        ctx.fillStyle = "rgb(255 255 255 / 0.12)";
        ctx.fillRect(x + radius, y, w - radius * 2, 1);
      }

      if (showLabels && w > 50 && h > 22 && kind !== SMALLER) {
        const twoLines = h > 38;
        const ink = neutral ? colors.text1 : "rgb(255 255 255 / 0.98)";
        write(ctx, payload.names[i], x + 7, twoLines ? y + 12 : y + h / 2, w - 14, ink, 700, 11.5);
        if (twoLines) write(ctx, formatSize(payload.size[i]), x + 7, y + 27, w - 14, neutral ? colors.text2 : "rgb(255 255 255 / 0.82)", 600, 10.5);
      }
    }
  }

  function drawMarks() {
    const ctx = overlay?.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    if (!layout || !payload) return;
    const { rects } = layout;
    const box = (i: number) => {
      const [x, y, w, h] = rects.subarray(i * 4, i * 4 + 4);
      return { x, y, w, h, radius: radiusOf(w, h) };
    };

    // A kind in focus: everything else steps back.
    if (focus && payload) {
      ctx.fillStyle = colors.background;
      ctx.globalAlpha = 0.72;
      for (const i of layout.order) {
        const kind = payload.kind[i];
        if (kind === FOLDER && opened[i]) continue;
        if ((kind === FILE || kind === CLOUD) && kinds[payload.ext[i]] === focus) continue;
        const b = box(i);
        tile(ctx, b.x, b.y, b.w, b.h, b.radius);
      }
      ctx.globalAlpha = 1;
    }

    for (const id of d.selection) {
      const i = indexOf.get(id);
      if (i === undefined) continue;
      const b = box(i);
      if (b.w <= 0) continue;
      ctx.fillStyle = `rgb(${colors.accent} / 0.2)`;
      tile(ctx, b.x, b.y, b.w, b.h, b.radius);
      ctx.strokeStyle = `rgb(${colors.accent})`;
      ring(ctx, b.x, b.y, b.w, b.h, b.radius, 2);
    }

    if (hover > 0) {
      const b = box(hover);
      if (b.w > 0) {
        const panel = payload.kind[hover] === FOLDER && opened[hover];
        if (!panel) {
          ctx.fillStyle = "rgb(255 255 255 / 0.1)";
          tile(ctx, b.x, b.y, b.w, b.h, b.radius);
        }
        ctx.strokeStyle = `rgb(${colors.accent} / ${panel ? 0.7 : 1})`;
        ring(ctx, b.x, b.y, b.w, b.h, b.radius, panel ? 1.5 : 2);
      }
    }
  }

  function itemAt(clientX: number, clientY: number): number {
    if (!layout || !overlay) return -1;
    const box = overlay.getBoundingClientRect();
    return hitTest(layout, clientX - box.left, clientY - box.top);
  }

  /** Where an item is: the treemap's folder, then the names down to it. */
  function pathOf(i: number): string {
    if (!payload) return "";
    const chain: string[] = [];
    for (let at = i; at > 0; at = payload.parent[at]) chain.push(payload.names[at]);
    const parts = [...d.trail.map((crumb) => crumb.name), ...chain.reverse()];
    return parts.reduce((path, part, n) => (n === 0 ? part : path.endsWith("\\") ? path + part : `${path}\\${part}`), "");
  }

  function tipOf(i: number): string {
    if (!payload) return "";
    const size = payload.size[i];
    const parent = payload.parent[i];
    const parentSize = parent >= 0 ? payload.size[parent] : size;
    const percent = parentSize ? (size / parentSize) * 100 : 100;
    const share = percent < 0.1 ? "<0.1%" : `${percent.toFixed(percent < 10 ? 1 : 0)}%`;
    const kind = payload.kind[i];
    const what =
      kind === FOLDER ? "Folder" : kind === SMALLER ? "Smaller items" : kind === LINK ? "Link" : categoryLabel(kinds[payload.ext[i]] ?? "other");
    const lines = [payload.names[i], `${what} · ${formatSize(size)} · ${share} of ${payload.names[parent] ?? "the folder"}`];
    if (payload.ids[i] !== SMALLER_ID) lines.push(pathOf(i));
    if (kind === FOLDER) lines.push("Double-click to open");
    return lines.join("\n");
  }

  function onMove(e: PointerEvent) {
    const i = itemAt(e.clientX, e.clientY);
    if (i !== hover) {
      hover = i;
      schedule(false);
    }
    if (i > 0) pointerTip.show(tipOf(i), e.clientX, e.clientY);
    else pointerTip.hide();
  }

  function onLeave() {
    hover = -1;
    schedule(false);
    pointerTip.hide();
  }

  function onClick(e: MouseEvent) {
    const i = itemAt(e.clientX, e.clientY);
    if (i <= 0 || !payload) return;
    const id = payload.ids[i];
    if (id === SMALLER_ID) return;
    d.select(id, e.ctrlKey || e.metaKey ? "toggle" : e.shiftKey ? "range" : "only");
    if (d.view === "folders") void d.showInTree(id);
  }

  function onDoubleClick(e: MouseEvent) {
    const i = itemAt(e.clientX, e.clientY);
    if (i > 0 && payload && payload.kind[i] === FOLDER) void d.zoomTo(payload.ids[i]);
  }

  function menuAt(_: HTMLElement, x: number, y: number) {
    pointerTip.hide();
    const i = itemAt(x, y);
    if (i <= 0 || !payload) return [];
    const kind = ["dir", "file", "link", "cloud"][payload.kind[i]];
    return itemActions(payload.ids[i], { kind, locked: kind === "link" }, "map");
  }

  function toggleLabels() {
    showLabels = !showLabels;
    schedule(true);
  }

  // A new payload: lay it out again.
  $effect(() => {
    payload = d.treemap;
    hover = -1;
    schedule(true);
  });

  // The legend counts the kinds of files in the folder shown.
  $effect(() => {
    if (d.treemap && d.extensions === null) void d.loadExtensions();
  });

  // The selection changed: only the marks are drawn again.
  $effect(() => {
    void [...d.selection];
    schedule(false);
  });

  onMount(() => {
    readColors();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const resized = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(measure, 60);
    });
    if (host) resized.observe(host);
    const themed = new MutationObserver(() => {
      readColors();
      schedule(true);
    });
    themed.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    measure();
    return () => {
      clearTimeout(timer);
      resized.disconnect();
      themed.disconnect();
      cancelAnimationFrame(frame);
      pointerTip.hide();
    };
  });
</script>

<div class="treemap-wrapper">
  <div class="map" bind:this={host} role="img" aria-label={label}>
    <canvas bind:this={base} aria-hidden="true"></canvas>
    <canvas
      class="marks"
      bind:this={overlay}
      aria-hidden="true"
      onpointermove={onMove}
      onpointerleave={onLeave}
      onpointerdown={() => pointerTip.hide()}
      onclick={onClick}
      ondblclick={onDoubleClick}
      {@attach contextActions(menuAt)}
    ></canvas>
    {#if !d.treemap}
      <p class="waiting"><LoaderCircle size={16} class="spin" /> Drawing the treemap…</p>
    {:else if d.treemap.ids.length <= 1}
      <p class="waiting">This folder is empty.</p>
    {/if}
  </div>

  <div class="legend">
    <div class="kinds" role="toolbar" aria-label="Kinds of files">
      {#each legend as kind (kind.id)}
        <button
          type="button"
          class="chip"
          class:active={focus === kind.id}
          aria-pressed={focus === kind.id}
          title="{kind.label}: {formatSize(kind.size)}. {focus === kind.id ? 'Click to show every kind again' : 'Click to bring them forward'}"
          onclick={() => toggleFocus(kind.id)}
        >
          <span class="dot" style:background={kind.color} aria-hidden="true"></span>
          {kind.label}
          <span class="count">{formatSize(kind.size)}</span>
        </button>
      {/each}
    </div>
    <button type="button" class="chip labels" class:active={showLabels} aria-pressed={showLabels} title="Names on the tiles" onclick={toggleLabels}>
      <Type size={13} />
      Labels
    </button>
  </div>
</div>

<style>
  .treemap-wrapper { display: flex; flex-direction: column; flex: 1; min-height: 360px; gap: 8px; }
  .map { position: relative; flex: 1; min-height: 220px; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--input-fill); }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; }
  .marks { cursor: pointer; }
  .waiting { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--text-3); font-size: var(--fluid-xs); pointer-events: none; }

  .treemap-wrapper { container-type: inline-size; }
  .legend { display: flex; align-items: flex-start; flex-shrink: 0; gap: 8px; padding: 2px 6px 6px; }
  .kinds { display: flex; flex: 1; flex-wrap: wrap; gap: 5px; min-width: 0; }
  .chip { height: 26px; padding: 0 9px; gap: 6px; font-size: 11.5px; }
  .dot { flex: none; width: 9px; height: 9px; border-radius: 3px; }
  .labels { flex: none; }
  .labels :global(svg) { color: rgb(var(--accent-soft-rgb)); }
  /* A narrow pane: the sizes move to each chip's tooltip. */
  @container (max-width: 620px) {
    .kinds .count { display: none; }
  }
</style>
