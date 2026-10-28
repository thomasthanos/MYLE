// Gets every page ready while the splash shows, and just after: what each one shows, read from this PC only, and its pictures decoded. A page then opens with its list, icons and covers in place instead of filling in. Pages still load as before when opened (this only gets there first), and nothing slow or with side effects runs here: no scans, no winget, no sync, no measuring the System Cleaner's folders. Two rounds: what matters for a page's first look (its data, and the icons of the page the app opens on) while the splash shows; the rest (other pages' pictures, covers, Spotify's update check online) once the window is open, so it never holds the opening up.
import { isTauri } from "@tauri-apps/api/core";
import { whenShown } from "../lib/startup";
import { creativeState } from "./pages/creative-hub/state.svelte";
import { gameSavesState } from "./pages/game-saves/state.svelte";
import { iconSources } from "./pages/install-apps/icons";
import { appsState } from "./pages/install-apps/state.svelte";
import { spotifyHubState } from "./pages/spotify-hub/state.svelte";
import { maintenanceState } from "./pages/system-maintenance/state.svelte";
import { debloat } from "./pages/windows-optimization/debloat/state.svelte";
import { windowsOptimizationState } from "./pages/windows-optimization/state.svelte";
/** Pictures decoded at the same time. */
const AT_ONCE = 6;
let started = false;
/** Lets the page in front draw first, then one step of the preload. */
function idle(): Promise<void> {
  return new Promise((resolve) =>
    "requestIdleCallback" in window ? requestIdleCallback(() => resolve(), { timeout: 400 }) : setTimeout(resolve, 60),
  );
}
/** Fetches and decodes a picture, so an `<img>` of it shows at once. */
function warm(src: string): Promise<void> {
  const image = new Image();
  image.decoding = "async";
  image.src = src;
  return image.decode().catch(() => {});
}
async function warmAll(sources: Iterable<string | null | undefined>) {
  const list = [...new Set([...sources].filter((src): src is string => !!src))];
  for (let i = 0; i < list.length; i += AT_ONCE) {
    await Promise.all(list.slice(i, i + AT_ONCE).map(warm));
  }
}
async function step(name: string, work: () => Promise<unknown>) {
  await idle();
  try {
    await work();
  } catch (error) {
    // One page not ready is no reason to stop: it loads itself when opened.
    console.debug(`Preload of ${name} skipped:`, error);
  }
}
export async function preloadApp() {
  if (started || !isTauri()) return;
  started = true;
  // While the splash shows: each page's data, and the first page's icons.
  await step("Install Apps icons", () => warmAll(appsState.curated.map((app) => iconSources(app)[0])));
  await step("Game Saves", () => gameSavesState.preload());
  await step("Windows Optimization", () => Promise.all([debloat.load(), windowsOptimizationState.load()]));
  await step("Creative Suite", () => creativeState.load());
  await step("Spotify Hub", () => spotifyHubState.load(false, { checkUpdates: false }));
  await step("System Maintenance", () => maintenanceState.load());
  // Once the window is open: pictures of other pages and online checks.
  await whenShown();
  await step("Game Saves covers", () => gameSavesState.loadCovers());
  await step("Windows Optimization icons", () => warmAll(["/icons/Windows-AutoLogon.svg", "/icons/Restart-to-BIOS.svg"]));
  await step("Creative Suite icons", () => warmAll(creativeState.apps.map((app) => app.icon)));
  await step("Spotify Hub updates", () => spotifyHubState.checkUpdates());
}
