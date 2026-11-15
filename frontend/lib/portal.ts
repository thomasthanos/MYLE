import type { Attachment } from "svelte/attachments";
/* Moves a modal's root element to `<body>`. Pages render inside the content panel's scroller, whose `contain: strict` makes it the containing block of any `position: fixed` child. A modal left there covers only that panel; from `<body>` it covers the whole window. Use on the single root element of an `{#if}` block or component.
   A modal's `.backdrop` covers the window's frame, not the transparent margin around it where the frame's shadow is drawn (the main window is transparent): it takes the frame's inset and corners, `--modal-inset` and `--modal-radius` (tokens.css; 0 while maximized). Anything else moved here (the context menu) keeps its own place. */
export const portal: Attachment<HTMLElement> = (node) => {
  if (node.classList.contains("backdrop")) {
    node.style.inset = "var(--modal-inset, 0px)";
    node.style.borderRadius = "var(--modal-radius, 0px)";
  }
  document.body.append(node);
  return () => node.remove();
};
