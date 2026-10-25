// File-type colours, shared by the treemap and the File types tab. The scan's
// 20 largest extensions get one each, by rank (the backend ranks them once
// per scan, so the colours stay put after a delete); the rest share one.
// The accent blue leads, then muted hues that sit well on the dark theme.

export const PALETTE = [
  "#6b85ff", "#4fbf95", "#d9a35b", "#b48ae6", "#55b3c9",
  "#d97a7a", "#93bf6a", "#d6c065", "#7e8dd6", "#4fa89f",
  "#e08f6a", "#b77ab8", "#7db88a", "#6aa6db", "#c49a74",
  "#d987a6", "#8fa1b3", "#a8c276", "#9b86cc", "#5fc2cf",
] as const;

/** Files outside the first 20 extensions. */
export const OTHER = "#5d6577";
/** A folder drawn without what is inside it. */
export const FOLDER_FILL = "#3a4254";
/** A folder's frame and header strip. */
export const FOLDER_FRAME = "#1f2430";
export const FOLDER_TEXT = "#c9d1e3";
/** The block of a folder's small entries. */
export const SMALLER_FILL = "#2f3542";
/** A link: listed, never followed, nothing counted. */
export const LINK_FILL = "#454b58";

export function colorOf(rank: number): string {
  return rank >= 0 && rank < PALETTE.length ? PALETTE[rank] : OTHER;
}

/** Dark or light text, whichever reads better on `hex`. */
export function textOn(hex: string): string {
  const n = Number.parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.55 ? "rgb(12 14 20 / 0.88)" : "rgb(255 255 255 / 0.92)";
}
