<script lang="ts">
  // The treemap, drawn on a canvas: thousands of rectangles are too many for
  // the DOM. The map itself is drawn on one canvas, only when the data or the
  // size changes; hover and selection go on a second canvas above it, so
  // moving the pointer never redraws the map. The tooltip and the right-click
  // menu find the item under the pointer in the layout's flat arrays.
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
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
  let colors = { background: "#121212", accent: "#6b85ff", font: "sans-serif" };

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
      background: style.getPropertyValue("--input-fill").trim() || "#121212",
      accent: style.getPropertyValue("--accent").trim() || "#6b85ff",
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
    if (room < 14) return null;
    const full = ctx.measureText(text).width;
    if (full <= room) return text;
    for (let n = Math.floor((text.length * room) / full) - 1; n > 0; n--) {
      const short = `${text.slice(0, n)}…`;
      if (ctx.measureText(short).width <= room) return short;
    }
    return null;
  }

  function write(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, room: number, color: string) {
    const shown = fitted(ctx, text, room);
    if (!shown) return;
    ctx.fillStyle = color;
    ctx.fillText(shown, x, y);
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
    ctx.font = `600 11px ${colors.font}`;
    for (const i of order) {
      const x = rects[i * 4];
      const y = rects[i * 4 + 1];
      const w = rects[i * 4 + 2];
      const h = rects[i * 4 + 3];
      const kind = payload.kind[i];
      if (kind === FOLDER && opened[i]) {
        // A folder: a darker frame, its header strip when it has one.
        ctx.fillStyle = FOLDER_FRAME;
        ctx.fillRect(x, y, w - 1, h - 1);
        if (header[i]) write(ctx, `${payload.names[i]}  ${formatSize(payload.size[i])}`, x + 5, y + header[i] / 2 + 1, w - 10, FOLDER_TEXT);
        continue;
      }
      const fill =
        kind === FOLDER ? FOLDER_FILL : kind === SMALLER ? SMALLER_FILL : kind === LINK ? LINK_FILL : colorOf(payload.ranks[payload.ext[i]] ?? 255);
      ctx.fillStyle = fill;
      ctx.fillRect(x, y, Math.max(1, w - 1), Math.max(1, h - 1));
      if (w > 46 && h > 16) {
        const ink = textOn(fill);
        const twoLines = h > 34;
        write(ctx, payload.names[i], x + 5, twoLines ? y + 11 : y + h / 2, w - 10, ink);
        if (twoLines) write(ctx, formatSize(payload.size[i]), x + 5, y + 25, w - 10, ink);
      }
    }
  }

  function outline(ctx: CanvasRenderingContext2D, i: number, stroke: string, lineWidth: number, tint: string | null) {
    if (!layout) return;
    const [x, y, w, h] = layout.rects.subarray(i * 4, i * 4 + 4);
    if (w <= 0) return;
    if (tint) {
      ctx.fillStyle = tint;
      ctx.fillRect(x, y, w - 1, h - 1);
    }
    ctx.strokeStyle = stroke;
    ctx.lineWidth = lineWidth;
    ctx.strokeRect(x + lineWidth / 2, y + lineWidth / 2, Math.max(0, w - 1 - lineWidth), Math.max(0, h - 1 - lineWidth));
  }

  function drawMarks() {
    const ctx = overlay?.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    for (const id of d.selection) {
      const i = indexOf.get(id);
      if (i !== undefined) outline(ctx, i, colors.accent, 2, "rgb(255 255 255 / 0.12)");
    }
    if (hover >= 0) outline(ctx, hover, "rgb(255 255 255 / 0.85)", 1.5, null);
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
    const shown = percent < 0.1 ? "less than 0.1" : percent.toFixed(percent < 10 ? 1 : 0);
    const lines = [payload.names[i], `${formatSize(size)} · ${shown}% of ${parent >= 0 ? payload.names[parent] : "this folder"}`];
    if (payload.ids[i] !== SMALLER_ID) lines.push(pathOf(i));
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
    // The theme changed (Settings): the same layout in the new colours.
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

<style>
  .map { position: relative; flex: 1; min-height: 160px; overflow: hidden; border-radius: var(--radius-sm); background: var(--input-fill); }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; }
  .marks { cursor: default; }
  .waiting { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--text-3); font-size: var(--fluid-xs); pointer-events: none; }
</style>
