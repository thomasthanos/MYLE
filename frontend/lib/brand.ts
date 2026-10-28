// The app icon's scene (backend/icons/app-icon.svg), in its 1024×1024 space: a hooded hacker operator in front of a glowing cyber terminal. Shared by the static mark (components/Logo.svelte) and the animated splash (splash/CoderScene).
/** The rounded tile everything sits on. */
export const TILE = { x: 64, y: 64, size: 896, radius: 220 };
/** The screen inside the terminal bezel. */
export const SCREEN = { x: 248, y: 212, width: 528, height: 354, radius: 22 };
/** Terminal `>` prompt chevron on the first line. */
export const PROMPT = "M280 246L302 262L280 278";
/** Peaked hacker hood silhouette with sculpted side cowl. */
export const HOOD =
  "M512 330C470 344 420 384 400 456C386 508 380 584 364 646C426 708 598 708 660 646C644 584 638 508 624 456C604 384 554 344 512 330Z";
/** The drape where the hood meets the back. */
export const HOOD_HEM = "M364 646C426 708 598 708 660 646";
/** Sculpted side folds along the hacker hood. */
export const HOOD_FOLDS =
  "M448 402C430 486 420 572 406 654M576 402C594 486 604 572 618 654";
/** Broad, structured hoodie shoulders and torso. */
export const TORSO =
  "M432 610C336 626 258 666 224 724C194 776 186 866 184 1000H840C838 866 830 776 800 724C766 666 688 626 592 610Z";
/** Angled shoulder seam accents on the hoodie. */
export const TORSO_SEAMS =
  "M372 634L256 748M652 634L768 748";
export const SPARKLE =
  "M732 234c2.6 17.6 10 25 27.6 27.6-17.6 2.6-25 10-27.6 27.6-2.6-17.6-10-25-27.6-27.6 17.6-2.6 25-10 27.6-27.6z";
/** One token of code: a round-capped line from `x` to `x + w` at height `y`. */
export interface CodeToken {
  x: number;
  y: number;
  w: number;
  color: string;
  alpha?: number;
}
/** Stroke width of a code token; its caps stick out by half of it. */
export const CODE_STROKE = 20;
/** The terminal commands & code on screen, in typing order. */
export const CODE: CodeToken[] = [
  { x: 326, y: 262, w: 92, color: "#34d399" },
  { x: 440, y: 262, w: 164, color: "#22d3ee" },
  { x: 334, y: 310, w: 76, color: "#c084fc" },
  { x: 432, y: 310, w: 196, color: "#e2e8f0", alpha: 0.78 },
  { x: 334, y: 358, w: 132, color: "#38bdf8" },
  { x: 488, y: 358, w: 108, color: "#fbbf24" },
  { x: 618, y: 358, w: 68, color: "#34d399", alpha: 0.65 },
  { x: 374, y: 406, w: 142, color: "#818cf8" },
  { x: 538, y: 406, w: 84, color: "#22d3ee", alpha: 0.75 },
  { x: 334, y: 454, w: 88, color: "#c084fc" },
  { x: 298, y: 502, w: 64, color: "#34d399" },
];
/** Fewer, bolder lines for the mark at title-bar sizes. */
export const CODE_COMPACT: CodeToken[] = [
  { x: 336, y: 272, w: 104, color: "#34d399" },
  { x: 492, y: 272, w: 176, color: "#22d3ee" },
  { x: 356, y: 352, w: 110, color: "#c084fc" },
  { x: 518, y: 352, w: 142, color: "#fbbf24" },
];
export const CODE_COMPACT_STROKE = 44;
/** The glowing terminal block cursor after the last token. */
export const CARET = { x: 382, y: 488, width: 18, height: 28 };
