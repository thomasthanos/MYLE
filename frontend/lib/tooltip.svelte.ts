// A tooltip that follows the pointer, for things drawn on a canvas, where no
// element per item can carry a `title`. TooltipHost draws it with the same
// look as every other tooltip.

const SHOW_AFTER_MS = 350;

class PointerTip {
  text = $state("");
  x = $state(0);
  y = $state(0);
  visible = $state(false);
  #timer: ReturnType<typeof setTimeout> | undefined;

  /** Shows `text` near (x, y): after a short rest, then at once while it is up. */
  show(text: string, x: number, y: number) {
    this.text = text;
    this.x = x;
    this.y = y;
    if (this.visible) return;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => (this.visible = !!this.text), SHOW_AFTER_MS);
  }

  hide() {
    clearTimeout(this.#timer);
    this.visible = false;
    this.text = "";
  }
}

export const pointerTip = new PointerTip();
