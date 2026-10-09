import type { Attachment } from "svelte/attachments";

/**
 * Moves a modal's root element to `<body>`.
 *
 * Pages render inside the content panel's scroller, whose `contain: strict`
 * makes it the containing block of any `position: fixed` child. A modal left
 * there covers only that panel; from `<body>` it covers the whole window.
 *
 * Use on the single root element of an `{#if}` block or component.
 */
export const portal: Attachment<HTMLElement> = (node) => {
  document.body.append(node);
  return () => node.remove();
};
