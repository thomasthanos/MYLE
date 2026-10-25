// File-type colours, shared by the treemap and the File types tab.
// Vibrant, high-contrast, modern Windows 11 Fluent dark-mode palette.

export const PALETTE = [
  "#3b82f6", // Vivid Blue
  "#10b981", // Emerald Green
  "#f59e0b", // Warm Amber
  "#8b5cf6", // Purple / Violet
  "#06b6d4", // Electric Cyan
  "#f43f5e", // Rose
  "#84cc16", // Lime
  "#eab308", // Goldenrod
  "#6366f1", // Indigo
  "#14b8a6", // Teal
  "#f97316", // Orange
  "#d946ef", // Fuchsia
  "#22c55e", // Green
  "#0ea5e9", // Sky Blue
  "#d97706", // Amber Dark
  "#ec4899", // Pink
  "#64748b", // Slate
  "#a3e635", // Bright Lime
  "#a855f7", // Deep Purple
  "#2dd4bf", // Mint Teal
] as const;

/** Files outside the first 20 extensions. */
export const OTHER = "#475569";
/** A folder drawn without what is inside it. */
export const FOLDER_FILL = "rgba(30, 41, 59, 0.75)";
/** A folder's frame and header strip. */
export const FOLDER_FRAME = "#0f172a";
export const FOLDER_TEXT = "#e2e8f0";
/** The block of a folder's small entries. */
export const SMALLER_FILL = "rgba(51, 65, 85, 0.7)";
/** A link: listed, never followed, nothing counted. */
export const LINK_FILL = "#475569";

export function colorOf(rank: number): string {
  return rank >= 0 && rank < PALETTE.length ? PALETTE[rank] : OTHER;
}

/** Dark or light text, whichever reads better on `hex`. */
export function textOn(hex: string): string {
  if (!hex || !hex.startsWith("#")) return "rgb(255 255 255 / 0.95)";
  const n = Number.parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.55 ? "rgb(12 14 20 / 0.95)" : "rgb(255 255 255 / 0.95)";
}
