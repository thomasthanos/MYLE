// The main window loads hidden behind the splash and opens once it has drawn. What its first look doesn't need (winget, pictures of other pages, network checks) waits until it is open, so the open itself stays quick.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
/** Opens anyway if the hand-over never says so (a broken splash, a reload). */
const GIVE_UP_MS = 15_000;
let shown: Promise<void> | null = null;
/** Tells the backend the main page has drawn: the window can open. */
export function mainReady() {
  if (!isTauri()) return;
  requestAnimationFrame(() => requestAnimationFrame(() => void invoke("main_ready").catch(() => {})));
}
/** Resolves once the main window is open (or in the tray, started at sign-in). */
export function whenShown(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  shown ??= new Promise<void>((resolve) => {
    void listen("myle-shown", () => resolve());
    void getCurrentWindow().isVisible().then((visible) => visible && resolve(), () => {});
    setTimeout(resolve, GIVE_UP_MS);
  });
  return shown;
}
