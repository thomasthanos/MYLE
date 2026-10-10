import { readFlag, writeFlag } from "./storage";

const DARK_KEY = "myle.dark";
/** The old "Glass effects" switch: gone, so whatever it saved is dropped. */
const OLD_GLASS_KEY = "myle.glass";

/**
 * Visual preferences that apply to the whole document.
 *
 * Panels are always solid (`:root.solid`; the splash and the setup window
 * set it too). "Dark theme" uses matte layers and blurple actions
 * (`:root.dark`, tokens.css).
 */
class Settings {
  dark = $state(readFlag(DARK_KEY, false));

  constructor() {
    try {
      localStorage.removeItem(OLD_GLASS_KEY);
    } catch {
      // Storage blocked: nothing to clean up.
    }
    this.apply();
  }

  /** Re-reads the saved value (after account sync replaced it). */
  reload() {
    this.dark = readFlag(DARK_KEY, this.dark);
    this.apply();
  }

  setDark(value: boolean) {
    this.dark = value;
    writeFlag(DARK_KEY, value);
    this.apply();
  }

  private apply() {
    const root = document.documentElement;
    root.classList.add("solid");
    root.classList.toggle("dark", this.dark);
  }
}

export const settings = new Settings();
