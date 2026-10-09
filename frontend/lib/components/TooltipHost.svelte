<script lang="ts">
  // Every `title` in the app, drawn by the app instead of by Windows: the
  // native tooltip is small, square and light on a dark glass window. On the
  // first hover an element's title moves to `data-tip` (so Windows shows
  // nothing), and stays readable to screen readers as its label or
  // description. Lines in a title ("\n") are kept, and long paths wrap.
  import { onMount, untrack } from "svelte";

  const SHOW_AFTER_MS = 450;
  /** Moving from one tooltip to the next shows the next at once. */
  const WARM_MS = 400;
  const GAP = 8;
  const EDGE = 8;

  let text = $state("");
  let left = $state(0);
  let top = $state(0);
  let visible = $state(false);
  /** Measured and moved into place: until then it is drawn but not seen. */
  let placed = $state(false);
  let box = $state<HTMLDivElement>();

  let target: HTMLElement | null = null;
  /** Hovered, its tooltip still waiting for the delay. */
  let pending: HTMLElement | null = null;
  let pointerX = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let hiddenAt = 0;

  /** The element's tooltip text, taking its `title` over first. */
  function tipOf(element: HTMLElement): string {
    const title = element.getAttribute("title");
    if (title !== null) {
      element.removeAttribute("title");
      element.dataset.tip = title;
      // What the title told a screen reader, it keeps telling it.
      if (title && !element.hasAttribute("aria-label") && !element.textContent?.trim()) {
        element.setAttribute("aria-label", title);
      } else if (title && (!element.hasAttribute("aria-description") || element.dataset.tipDescribes)) {
        element.setAttribute("aria-description", title);
        element.dataset.tipDescribes = "1";
      }
    }
    return element.dataset.tip?.trim() ?? "";
  }

  function tipTarget(node: EventTarget | null): HTMLElement | null {
    if (!(node instanceof Element) || node.closest("[data-no-tooltip]")) return null;
    return node.closest<HTMLElement>("[title], [data-tip]");
  }

  /** Below the element, or above it without room; under the pointer, kept
   *  inside the window. */
  function place() {
    if (!target || !box) return;
    const anchor = target.getBoundingClientRect();
    const { width, height } = box.getBoundingClientRect();
    const x = pointerX || anchor.left + anchor.width / 2;
    left = Math.min(Math.max(x - width / 2, EDGE), window.innerWidth - width - EDGE);
    const below = anchor.bottom + GAP;
    top = below + height <= window.innerHeight - EDGE ? below : Math.max(anchor.top - GAP - height, EDGE);
    placed = true;
  }

  function show(element: HTMLElement) {
    const content = tipOf(element);
    if (!content) return;
    pending = null;
    target = element;
    text = content;
    placed = false;
    visible = true;
    requestAnimationFrame(place);
  }

  function hide() {
    clearTimeout(timer);
    if (visible) hiddenAt = Date.now();
    visible = false;
    target = null;
    pending = null;
  }

  function onOver(event: PointerEvent) {
    const element = tipTarget(event.target);
    if (element && (element === target || element === pending)) return;
    hide();
    if (!element || !tipOf(element)) return;
    pending = element;
    pointerX = event.clientX;
    const wait = Date.now() - hiddenAt < WARM_MS ? 0 : SHOW_AFTER_MS;
    timer = setTimeout(() => {
      if (element.isConnected) show(element);
    }, wait);
  }

  function onMove(event: PointerEvent) {
    if (!visible) pointerX = event.clientX;
  }

  function onOut(event: PointerEvent) {
    const element = tipTarget(event.target);
    if (!element) return;
    const to = event.relatedTarget;
    if (to instanceof Node && element.contains(to)) return;
    if (element === target || element === pending) hide();
  }

  function onFocus(event: FocusEvent) {
    const element = tipTarget(event.target);
    if (element && element === event.target && element.matches(":focus-visible")) {
      pointerX = 0;
      show(element);
    }
  }

  onMount(() => {
    const options = { capture: true, passive: true };
    const listeners: [EventTarget, string, EventListener][] = [
      [document, "pointerover", onOver as EventListener],
      [document, "pointermove", onMove as EventListener],
      [document, "pointerout", onOut as EventListener],
      [document, "pointerdown", hide],
      [document, "wheel", hide],
      [document, "scroll", hide],
      [document, "keydown", hide],
      [document, "focusin", onFocus as EventListener],
      [document, "focusout", hide],
      [window, "blur", hide],
      [window, "resize", hide],
    ];
    // Removing a focused element can dispatch focusout during a Svelte
    // render. Native event handlers must run outside that render's tracking.
    const removeListeners = listeners.map(([on, name, listener]) => {
      const handle: EventListener = (event) => untrack(() => listener(event));
      on.addEventListener(name, handle, options);
      return () => on.removeEventListener(name, handle, options);
    });
    return () => {
      clearTimeout(timer);
      for (const remove of removeListeners) remove();
    };
  });
</script>

{#if visible}
  <div
    class="tooltip"
    class:placed
    role="tooltip"
    bind:this={box}
    style:left="{left}px"
    style:top="{top}px">{text}</div>
{/if}

<style>
  .tooltip {
    position: fixed;
    z-index: 300;
    max-width: min(460px, calc(100vw - 16px));
    padding: 6px 10px;
    border: 1px solid rgb(255 255 255 / 0.1);
    border-radius: var(--radius-sm);
    background: var(--tooltip-bg);
    box-shadow: var(--elev-1);
    color: var(--text-1);
    font-size: 12px;
    line-height: 1.45;
    white-space: pre-line;
    overflow-wrap: anywhere;
    pointer-events: none;
    visibility: hidden;
  }

  .tooltip.placed {
    visibility: visible;
    animation: tip-in var(--dur-fast) var(--ease-out);
  }

  @keyframes tip-in {
    from {
      opacity: 0;
      transform: translateY(-3px);
    }
  }
</style>
