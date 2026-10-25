<script lang="ts">
  // The treemap, drawn on a canvas: thousands of rectangles are too many for
  // the DOM. The map itself is drawn on one canvas, only when the data or the
  // size changes; hover and selection go on a second canvas above it, so
  // moving the pointer never redraws the map. The tooltip and the right-click
  // menu find the item under the pointer in the layout's flat arrays.
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Type from "@lucide/svelte/icons/type";
  import { contextActions } from "../../../lib/context-menu";
  import { pointerTip } from "../../../lib/tooltip.svelte";
  import { formatSize, SMALLER_ID, type TreemapPayload } from "./api";
  import { itemActions } from "./actions";
  import { colorOf, FOLDER_FILL, FOLDER_FRAME, FOLDER_TEXT, LINK_FILL, SMALLER_FILL, textOn } from "./palette";
  import { diskState as d } from "./state.svelte";
  import { FOLDER, hitTest, layoutTreemap, SMALLER, type Layout } from "./treemap";

  const LINK = 2;

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
  let indexOf = new Map<number, number>();
  let hover = -1;
  let frame = 0;
  let wantMap = false;
  let wantMarks = false;
  let colors = { background: "#080d14", accent: "#3b82f6", font: "sans-serif" };

  // UI Visual Preferences
  let cushion3D = $state(true);
  let showLabels = $state(true);
  let highlightExt = $state<string | null>(null);

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

  // Top 7 extensions for the interactive legend bar
  const topExts = $derived.by(() => {
    if (!d.extensions) return [];
    return d.extensions.slice(0, 7).map((ext) => ({
      name: ext.other ? "other" : ext.ext || "none",
      size: formatSize(d.sizeMode === "alloc" ? ext.alloc : ext.size),
      color: colorOf(ext.rank),
    }));
  });

  function toggleExt(extName: string) {
    highlightExt = highlightExt === extName ? null : extName;
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
    colors = {
      background: style.getPropertyValue("--input-fill").trim() || "#080d14",
      accent: style.getPropertyValue("--accent").trim() || "#3b82f6",
      font: style.fontFamily || "sans-serif",
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
    indexOf = new Map();
    if (!layout || !payload) return;
    for (const i of layout.order) {
      opened[payload.parent[i]] = 1;
      indexOf.set(payload.ids[i], i);
    }
  }

  /** Text that fits in `room` pixels, shortened with "…" when it does not. */
  function fitted(ctx: CanvasRenderingContext2D, text: string, room: number): string | null {
    if (room < 12) return null;
    const full = ctx.measureText(text).width;
    if (full <= room) return text;
    for (let n = Math.floor((text.length * room) / full) - 1; n > 0; n--) {
      const short = `${text.slice(0, n)}…`;
      if (ctx.measureText(short).width <= room) return short;
    }
    return null;
  }

  function write(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, room: number, color: string, bold = false) {
    const shown = fitted(ctx, text, room);
    if (!shown) return;
    ctx.save();
    ctx.font = `${bold ? "600" : "500"} 11px ${colors.font}`;
    ctx.shadowColor = "rgba(0, 0, 0, 0.85)";
    ctx.shadowBlur = 3;
    ctx.shadowOffsetY = 1;
    ctx.fillStyle = color;
    ctx.fillText(shown, x, y);
    ctx.restore();
  }

  // Pre-calculated gradient cache to keep 60 FPS performance
  const shadeCache = new Map<string, { light: string; dark: string }>();
  function getShades(hexOrRgb: string) {
    let shades = shadeCache.get(hexOrRgb);
    if (!shades) {
      let r = 100, g = 100, b = 100;
      if (hexOrRgb.startsWith("#")) {
        const n = parseInt(hexOrRgb.slice(1), 16);
        r = (n >> 16) & 255;
        g = (n >> 8) & 255;
        b = n & 255;
      } else if (hexOrRgb.startsWith("rgb")) {
        const m = hexOrRgb.match(/\d+/g);
        if (m && m.length >= 3) {
          r = parseInt(m[0]);
          g = parseInt(m[1]);
          b = parseInt(m[2]);
        }
      }
      const lr = Math.min(255, Math.round(r + (255 - r) * 0.25));
      const lg = Math.min(255, Math.round(g + (255 - g) * 0.25));
      const lb = Math.min(255, Math.round(b + (255 - b) * 0.25));
      const dr = Math.max(0, Math.round(r * 0.72));
      const dg = Math.max(0, Math.round(g * 0.72));
      const db = Math.max(0, Math.round(b * 0.72));
      shades = {
        light: `rgb(${lr},${lg},${lb})`,
        dark: `rgb(${dr},${dg},${db})`,
      };
      shadeCache.set(hexOrRgb, shades);
    }
    return shades;
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

      if (kind === FOLDER && opened[i]) {
        // A folder: clean modern container frame with rounded borders
        const rw = Math.max(1, w - 1);
        const rh = Math.max(1, h - 1);
        ctx.fillStyle = FOLDER_FRAME;
        if (rw >= 14 && rh >= 14) {
          ctx.beginPath();
          ctx.roundRect(x, y, rw, rh, 4);
          ctx.fill();
          ctx.strokeStyle = "rgba(255, 255, 255, 0.08)";
          ctx.lineWidth = 1;
          ctx.stroke();
        } else {
          ctx.fillRect(x, y, rw, rh);
        }

        if (header[i]) {
          // Sleek folder header strip
          const hw = w - 8;
          const hh = Math.min(18, header[i]);
          ctx.fillStyle = "rgba(255, 255, 255, 0.05)";
          ctx.beginPath();
          ctx.roundRect(x + 3, y + 2, hw, hh, 3);
          ctx.fill();

          write(ctx, `${payload.names[i]}  ${formatSize(payload.size[i])}`, x + 8, y + hh / 2 + 2, hw - 12, FOLDER_TEXT, true);
        }
        continue;
      }

      const fill =
        kind === FOLDER ? FOLDER_FILL : kind === SMALLER ? SMALLER_FILL : kind === LINK ? LINK_FILL : colorOf(payload.ranks[payload.ext[i]] ?? 255);

      const rw = Math.max(1, w - 1);
      const rh = Math.max(1, h - 1);

      if (rw <= 3 || rh <= 3 || !cushion3D) {
        ctx.fillStyle = fill;
        ctx.fillRect(x, y, rw, rh);
      } else {
        const radius = rw >= 10 && rh >= 10 ? 3 : rw >= 6 && rh >= 6 ? 2 : 0;
        const shades = getShades(fill);
        const grad = ctx.createLinearGradient(x, y, x, y + rh);
        grad.addColorStop(0, shades.light);
        grad.addColorStop(0.5, fill);
        grad.addColorStop(1, shades.dark);

        ctx.fillStyle = grad;
        if (radius > 0) {
          ctx.beginPath();
          ctx.roundRect(x, y, rw, rh, radius);
          ctx.fill();

          // Delicate specular highlight
          ctx.strokeStyle = "rgba(255, 255, 255, 0.16)";
          ctx.lineWidth = 1;
          ctx.stroke();
        } else {
          ctx.fillRect(x, y, rw, rh);
        }
      }

      // Draw label when requested and room allows
      if (showLabels && rw > 46 && rh > 18) {
        const ink = textOn(fill);
        const twoLines = rh > 34;
        write(ctx, payload.names[i], x + 6, twoLines ? y + 11 : y + rh / 2, rw - 12, ink, false);
        if (twoLines) write(ctx, formatSize(payload.size[i]), x + 6, y + 25, rw - 12, "rgba(255, 255, 255, 0.85)", false);
      }
    }
  }

  function outline(ctx: CanvasRenderingContext2D, i: number, stroke: string, lineWidth: number, tint: string | null, glow = false) {
    if (!layout) return;
    const [x, y, w, h] = layout.rects.subarray(i * 4, i * 4 + 4);
    if (w <= 0) return;
    const rw = Math.max(0, w - 1);
    const rh = Math.max(0, h - 1);

    if (tint) {
      ctx.fillStyle = tint;
      ctx.fillRect(x, y, rw, rh);
    }

    ctx.save();
    if (glow) {
      ctx.shadowColor = stroke;
      ctx.shadowBlur = 8;
    }
    ctx.strokeStyle = stroke;
    ctx.lineWidth = lineWidth;
    ctx.strokeRect(x + lineWidth / 2, y + lineWidth / 2, Math.max(0, rw - lineWidth), Math.max(0, rh - lineWidth));
    ctx.restore();
  }

  function drawMarks() {
    const ctx = overlay?.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);

    // If an extension is highlighted from the legend
    if (highlightExt && payload && layout) {
      const matchExt = highlightExt.toLowerCase();
      for (const i of layout.order) {
        if (payload.kind[i] === 1) {
          const extName = (payload.exts[payload.ext[i]] || "").toLowerCase();
          if (extName === matchExt || (highlightExt === "other" && payload.ranks[payload.ext[i]] >= 20)) {
            outline(ctx, i, "#ffffff", 2, "rgba(255, 255, 255, 0.2)", true);
          }
        }
      }
    }

    for (const id of d.selection) {
      const i = indexOf.get(id);
      if (i !== undefined) outline(ctx, i, colors.accent, 2.5, "rgba(59, 130, 246, 0.25)", true);
    }

    if (hover >= 0) {
      outline(ctx, hover, "rgba(255, 255, 255, 0.95)", 2, "rgba(255, 255, 255, 0.12)", true);
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
    const shown = percent < 0.1 ? "<0.1%" : `${percent.toFixed(percent < 10 ? 1 : 0)}%`;
    const isFolder = payload.kind[i] === FOLDER;
    const kindLabel = isFolder ? "📁 Folder" : "📄 File";

    const lines = [
      `${kindLabel}: ${payload.names[i]}`,
      `📦 Size: ${formatSize(size)} (${shown} of parent)`,
    ];
    if (payload.ids[i] !== SMALLER_ID) {
      lines.push(`📂 Path: ${pathOf(i)}`);
    }
    if (isFolder) {
      lines.push("💡 Double-click to zoom in");
    }
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

  function toggleCushion() {
    cushion3D = !cushion3D;
    schedule(true);
  }

  // A new payload: lay it out again.
  $effect(() => {
    payload = d.treemap;
    hover = -1;
    schedule(true);
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

  <!-- Interactive Extension Legend & Treemap Controls -->
  <div class="map-controls-bar">
    <div class="legend-list" role="toolbar" aria-label="File type highlight">
      {#each topExts as ext}
        <button
          type="button"
          class="ext-pill"
          class:active={highlightExt === ext.name}
          onclick={() => toggleExt(ext.name)}
          title="Highlight {ext.name} files ({ext.size})"
        >
          <span class="dot" style:background={ext.color}></span>
          <span class="ext-name">.{ext.name}</span>
          <span class="ext-size">{ext.size}</span>
        </button>
      {/each}
    </div>
    <div class="display-tools">
      <button
        type="button"
        class="tool-btn"
        class:active={showLabels}
        onclick={toggleLabels}
        title="Toggle file labels"
      >
        <Type size={13} />
        <span>Labels</span>
      </button>
      <button
        type="button"
        class="tool-btn"
        class:active={cushion3D}
        onclick={toggleCushion}
        title="Toggle 3D cushion shading"
      >
        <Sparkles size={13} />
        <span>3D Cushion</span>
      </button>
    </div>
  </div>
</div>

<style>
  .treemap-wrapper { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: 8px; }
  .map { position: relative; flex: 1; min-height: 220px; overflow: hidden; border-radius: 12px; background: #080d14; border: 1px solid rgba(255, 255, 255, 0.08); box-shadow: inset 0 2px 8px rgba(0, 0, 0, 0.4); }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; }
  .marks { cursor: crosshair; }
  .waiting { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--text-3); font-size: var(--fluid-xs); pointer-events: none; }

  /* Sleek Legend & Tools Bar */
  .map-controls-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 6px 12px;
    padding: 6px 10px;
    background: rgba(15, 23, 42, 0.75);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    backdrop-filter: blur(8px);
  }
  .legend-list {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px 6px;
    min-width: 0;
  }
  .ext-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-2);
    font-size: 11px;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s, transform 0.15s;
  }
  .ext-pill:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-1);
    transform: translateY(-1px);
  }
  .ext-pill.active {
    border-color: #ffffff;
    background: rgba(255, 255, 255, 0.16);
    color: #ffffff;
    box-shadow: 0 0 8px rgba(255, 255, 255, 0.25);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    flex-shrink: 0;
  }
  .ext-name {
    font-weight: 600;
  }
  .ext-size {
    opacity: 0.75;
    font-variant-numeric: tabular-nums;
  }
  .display-tools {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
  }
  .tool-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 8px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-3);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .tool-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-1);
  }
  .tool-btn.active {
    background: var(--selected);
    border-color: rgba(59, 130, 246, 0.5);
    color: var(--text-1);
  }
</style>
