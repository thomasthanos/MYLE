// Typed bridge to the debloater (backend/src/debloat). The page sends only
// ids of the backend's fixed tables.
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
export type Category = "privacy" | "taskbar" | "explorer" | "ai" | "system" | "apps" | "features";
export type Risk = "safe" | "caution";
export type TweakState = "applied" | "notApplied" | "partial" | "unavailable";
export type AppGroup = "microsoft" | "bing" | "xbox" | "thirdParty";
/** The Quick setup profile something belongs to; each takes in the ones before it. */
export type Level = "light" | "recommended" | "maximum";
export interface TweakStatus {
  id: string;
  title: string;
  summary: string;
  category: Category;
  risk: Risk;
  /** The Quick setup profile it is part of; null: only when picked on its own. */
  level: Level | null;
  /** What to know before turning it on: what stops working or works differently. */
  note: string | null;
  /** Complete only after Windows restarts. */
  restart: boolean;
  /** Asked again on its own before it runs. */
  confirm: string | null;
  state: TweakState;
  /** MYLE applied it and kept what was there before. */
  canUndo: boolean;
}
export interface AppStatus {
  id: string;
  title: string;
  group: AppGroup;
  /** The Quick setup profile that removes it; null: only when picked. */
  level: Level | null;
  /** What it is, in a few words. */
  about: string;
  /** Why someone might keep it, when there is a reason. */
  keep: string | null;
  /** Installed packages for this user; empty when it is not installed. */
  packages: string[];
  removedByMyle: boolean;
  storeId: string | null;
}
export type StartAlignment = "left" | "center";
export type StartLayout = "default" | "morePins" | "moreRecommendations";
export type StartAllAppsView = "category" | "grid" | "list";
export interface StartMenuStatus {
  supported: boolean;
  alignment: StartAlignment;
  layout: StartLayout;
  allAppsView: StartAllAppsView;
  hideRecommended: boolean;
  showRecentApps: boolean;
  showMostUsedApps: boolean;
  showRecentFiles: boolean;
  showRecommendations: boolean;
  showAccountNotifications: boolean;
  folders: string[];
  hasPinsBackup: boolean;
}
export interface StartMenuUpdate {
  alignment?: StartAlignment;
  layout?: StartLayout;
  allAppsView?: StartAllAppsView;
  showRecentApps?: boolean;
  showMostUsedApps?: boolean;
  showRecentFiles?: boolean;
  showRecommendations?: boolean;
  showAccountNotifications?: boolean;
  folders?: string[];
}
export interface DebloatStatus {
  windows: { build: number; name: string; windows11: boolean };
  tweaks: TweakStatus[];
  apps: AppStatus[];
  startMenu: StartMenuStatus;
  adminReady: boolean;
}
export type RestorePoint = { result: "created" } | { result: "protectionOff" } | { result: "failed"; message: string };
export type StepState = "running" | "done" | "unchanged" | "failed";
export interface Step {
  id: string;
  label: string;
  state: StepState;
  detail: string | null;
}
export type DebloatEvent = { event: "step"; data: Step };
export interface DebloatOutcome {
  changed: number;
  failed: { label: string; message: string }[];
  reboot: boolean;
  needsAdmin: boolean;
}
export interface DebloatApi {
  status(): Promise<DebloatStatus>;
  restorePoint(turnOn: boolean): Promise<RestorePoint>;
  run(tweaks: string[], apps: string[], onEvent: (event: DebloatEvent) => void): Promise<DebloatOutcome>;
  undo(tweaks: string[], onEvent: (event: DebloatEvent) => void): Promise<DebloatOutcome>;
  openStore(app: string): Promise<void>;
  startMenuSet(update: StartMenuUpdate): Promise<StartMenuStatus>;
  startMenuHideRecommended(hide: boolean): Promise<StartMenuStatus>;
  startMenuApplyPins(pins: string[]): Promise<StartMenuStatus>;
  startMenuRestorePins(): Promise<StartMenuStatus>;
}
function channel(onEvent: (event: DebloatEvent) => void): Channel<DebloatEvent> {
  const events = new Channel<DebloatEvent>();
  events.onmessage = onEvent;
  return events;
}
const tauriApi: DebloatApi = {
  status: () => invoke("debloat_status"),
  restorePoint: (turnOn) => invoke("debloat_restore_point", { turnOn }),
  run: (tweaks, apps, onEvent) => invoke("debloat_run", { tweaks, apps, onEvent: channel(onEvent) }),
  undo: (tweaks, onEvent) => invoke("debloat_undo", { tweaks, onEvent: channel(onEvent) }),
  openStore: (app) => invoke("debloat_open_store", { app }),
  startMenuSet: (update) => invoke("debloat_start_menu_set", { update }),
  startMenuHideRecommended: (hide) => invoke("debloat_start_menu_hide_recommended", { hide }),
  startMenuApplyPins: (pins) => invoke("debloat_start_menu_apply_pins", { pins }),
  startMenuRestorePins: () => invoke("debloat_start_menu_restore_pins"),
};
/** In a plain browser (`npx vite`): a pretend PC, to work on the page. */
function previewApi(): DebloatApi {
  const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
  const tweak = (id: string, title: string, summary: string, category: Category, level: Level | null, state: TweakState, extra: Partial<TweakStatus> = {}): TweakStatus => ({
    id, title, summary, category, risk: "safe", level, note: null, restart: false, confirm: null, state, canUndo: false, ...extra,
  });
  let tweaks: TweakStatus[] = [
    tweak("telemetry", "Turn off telemetry", "Windows sends Microsoft as little about how you use the PC as it allows, keeps no advertising ID and stops asking for feedback.", "privacy", "light", "partial"),
    tweak("edge", "Remove Microsoft Edge", "Uninstalls the Edge browser. WebView2, which MYLE and other apps are built on, stays.", "apps", null, "notApplied", {
      risk: "caution",
      confirm: "Remove Microsoft Edge? Links that open in Edge by default will ask for another browser.",
      note: "Links that open in Edge will ask for another browser. Edge can be installed again with Undo.",
    }),
    tweak("clock-24h", "24-hour clock", "The taskbar clock and every app show the time as 13:45 instead of 1:45 PM.", "system", "maximum", "notApplied", { note: "Changes the time format of your user account, not only the taskbar." }),
    tweak("background-apps", "Stop Store apps running in the background", "Store apps no longer run, update tiles or use the network while they are closed.", "system", "recommended", "notApplied", { note: "Store apps such as Mail update and notify only while they are open." }),
    tweak("services", "Set unneeded services to manual", "Background services most people never use (Maps, Retail Demo, Remote Registry and a few more) start only when something needs them.", "system", "recommended", "partial", { restart: true, note: "Takes full effect after the next restart." }),
    tweak("copilot", "Turn off Copilot", "Removes the Copilot app and button, and turns off Recall, Click to Do and the AI features in Notepad.", "ai", "recommended", "notApplied", { note: "Copilot, Recall and Click to Do go away until you turn this off again." }),
    tweak("location", "Turn off location tracking", "Apps and Windows can no longer ask where the PC is.", "privacy", "maximum", "notApplied", { note: "Maps, weather and Find my device can no longer tell where the PC is." }),
    tweak("taskbar-search", "Hide search on the taskbar", "Removes the search box or icon; Start still searches when you type.", "taskbar", "maximum", "notApplied", { note: "Press the Windows key and type: Start still searches." }),
    tweak("end-task", "End Task on right-click", "Right-click an app on the taskbar to close it at once.", "taskbar", "recommended", "applied"),
    tweak("taskview-widgets", "Hide Task View and Widgets", "Removes the Task View button and the Widgets board from the taskbar.", "taskbar", "maximum", "partial"),
    tweak("bing", "Remove Bing from search", "Start searches only your PC: no web results, no Bing suggestions.", "privacy", "recommended", "notApplied"),
    tweak("classic-context-menu", "Classic right-click menu", "The full Windows 10 right-click menu in File Explorer.", "explorer", "maximum", "notApplied"),
    tweak("suggestions", "Stop suggested apps and ads", "Windows no longer installs promoted apps by itself, nor shows suggestions.", "privacy", "light", "partial"),
    tweak("activity-history", "Turn off activity history", "Windows stops recording which apps and files you use.", "privacy", "light", "applied", { canUndo: true }),
    tweak("delivery-optimization", "No update sharing with other PCs", "Windows Update downloads only from Microsoft.", "system", "light", "notApplied"),
    tweak("game-dvr", "Turn off Game DVR", "No background recording of games by the Xbox Game Bar.", "system", "maximum", "notApplied"),
    tweak("file-extensions", "Show file extensions", "File Explorer shows .exe, .pdf and the rest.", "explorer", "recommended", "applied"),
    tweak("hidden-files", "Show hidden files", "File Explorer shows hidden files and folders.", "explorer", null, "notApplied"),
    tweak("dark-mode", "Dark mode", "Windows and apps use the dark theme.", "system", null, "applied"),
    tweak("taskbar-left", "Taskbar icons on the left", "Start and the taskbar icons sit on the left.", "taskbar", null, "notApplied"),
    tweak("mouse-acceleration", "Turn off mouse acceleration", "The pointer moves as far as the mouse does.", "system", null, "notApplied"),
    tweak("sticky-keys", "No Sticky Keys prompt", "Pressing Shift five times no longer asks about Sticky Keys.", "system", null, "notApplied"),
    tweak("steps-recorder", "Remove Steps Recorder", "Records the steps of a problem as screenshots and text. Microsoft is retiring it.", "features", "maximum", "notApplied"),
    tweak("math-input", "Remove Math Input Panel", "Turns handwritten maths into text for documents.", "features", "maximum", "applied", { canUndo: true }),
    tweak("ie-mode", "Remove Internet Explorer mode", "The old engine behind Internet Explorer mode in Edge.", "features", null, "notApplied", {
      risk: "caution",
      note: "Old company and government sites that need Internet Explorer mode stop working in Edge.",
    }),
    tweak("wordpad", "Remove WordPad", "The old rich-text editor, which Microsoft has retired.", "features", "maximum", "unavailable"),
  ];
  const app = (id: string, title: string, group: AppGroup, level: Level | null, installed: boolean, storeId: string | null = null, about = "An app.", keep: string | null = null): AppStatus => ({
    id, title, group, level, about, keep, packages: installed ? [`Preview.${id}`] : [], removedByMyle: false, storeId,
  });
  let apps: AppStatus[] = [
    app("clipchamp", "Clipchamp", "microsoft", "recommended", true, "9P1J8S7CCWWT", "Video editor."),
    app("get-started", "Get Started (Tips)", "microsoft", "recommended", true),
    app("office-hub", "Microsoft 365 (Office)", "microsoft", "recommended", true),
    app("solitaire", "Solitaire Collection", "microsoft", "recommended", true),
    app("todo", "Microsoft To Do", "microsoft", "recommended", true),
    app("feedback-hub", "Feedback Hub", "microsoft", "recommended", true),
    app("dev-home", "Dev Home", "microsoft", "recommended", true),
    app("copilot", "Microsoft Copilot", "microsoft", "recommended", true),
    app("mail-calendar", "Mail & Calendar", "microsoft", null, true),
    app("calculator", "Calculator", "microsoft", null, true, null, "Everyday and scientific calculator.", "Most people use it."),
    app("photos", "Photos", "microsoft", null, true),
    app("notepad", "Notepad", "microsoft", null, true),
    app("snipping-tool", "Snipping Tool", "microsoft", null, true, null, "Screenshots (Win+Shift+S) and screen recording.", "Screenshots with Win+Shift+S need it."),
    app("phone-link", "Phone Link", "microsoft", "maximum", true, null, "Your phone's messages, calls and photos on the PC.", "Keep it to use your phone from the PC."),
    app("bing-news", "Bing News", "bing", "recommended", true, "9WZDNCRFHVFW"),
    app("bing-weather", "Bing Weather", "bing", "recommended", true, "9WZDNCRFJ3Q2"),
    app("bing-search", "Bing Search", "bing", "recommended", true),
    app("xbox-app", "Xbox", "xbox", null, true, null, "Xbox games and PC Game Pass.", "Needed for PC Game Pass and many Xbox games."),
    app("xbox-game-bar", "Xbox Game Bar", "xbox", null, true),
    app("candy-crush", "Candy Crush Saga", "thirdParty", "light", true, null, "Puzzle game installed as an ad."),
    app("tiktok", "TikTok", "thirdParty", "light", true),
    app("spotify", "Spotify", "thirdParty", "maximum", true, null, "Music streaming.", "Keep it if you listen to Spotify."),
    app("maps", "Maps", "microsoft", "recommended", false),
  ];
  const run = async (ids: string[], appIds: string[], onEvent: (event: DebloatEvent) => void, undo: boolean) => {
    let changed = 0;
    for (const id of ids) {
      const found = tweaks.find((t) => t.id === id);
      if (!found) continue;
      const label = undo ? `Undo: ${found.title}` : found.title;
      onEvent({ event: "step", data: { id, label, state: "running", detail: null } });
      await wait(id === "edge" ? 2200 : 450);
      const already = !undo && found.state === "applied";
      onEvent({ event: "step", data: { id, label, state: already ? "unchanged" : "done", detail: null } });
      if (!already) changed++;
      tweaks = tweaks.map((t) => (t.id === id ? { ...t, state: undo ? "notApplied" : "applied", canUndo: !undo } : t));
    }
    for (const appId of appIds) {
      const found = apps.find((a) => a.id === appId);
      if (!found?.packages.length) continue;
      const id = `app:${appId}`;
      onEvent({ event: "step", data: { id, label: `Remove ${found.title}`, state: "running", detail: null } });
      await wait(350);
      onEvent({ event: "step", data: { id, label: `Remove ${found.title}`, state: "done", detail: null } });
      apps = apps.map((a) => (a.id === appId ? { ...a, packages: [], removedByMyle: true } : a));
      changed++;
    }
    onEvent({ event: "step", data: { id: "explorer", label: "Restart Explorer", state: "running", detail: null } });
    await wait(600);
    onEvent({ event: "step", data: { id: "explorer", label: "Restart Explorer", state: "done", detail: null } });
    return { changed, failed: [], reboot: ids.includes("services"), needsAdmin: false };
  };
  let startMenu: StartMenuStatus = {
    supported: true,
    alignment: "center",
    layout: "default",
    allAppsView: "category",
    hideRecommended: false,
    showRecentApps: true,
    showMostUsedApps: true,
    showRecentFiles: true,
    showRecommendations: true,
    showAccountNotifications: true,
    folders: ["downloads", "settings"],
    hasPinsBackup: false,
  };
  return {
    async status() {
      await wait(250);
      return { windows: { build: 26200, name: "Windows 11 Pro 25H2 (26200.6584)", windows11: true }, tweaks, apps, startMenu, adminReady: false };
    },
    async restorePoint() {
      await wait(1200);
      return { result: "created" };
    },
    run: (tweaksToRun, appIds, onEvent) => run(tweaksToRun, appIds, onEvent, false),
    undo: (ids, onEvent) => run(ids, [], onEvent, true),
    async openStore() {},
    async startMenuSet(update) {
      await wait(150);
      startMenu = { ...startMenu, ...update };
      return startMenu;
    },
    async startMenuHideRecommended(hide) {
      await wait(400);
      startMenu = {
        ...startMenu,
        hideRecommended: hide,
        ...(hide ? { showRecentApps: false, showRecentFiles: false, showRecommendations: false } : { showRecentApps: true, showRecentFiles: true }),
      };
      return startMenu;
    },
    async startMenuApplyPins() {
      await wait(900);
      startMenu = { ...startMenu, hasPinsBackup: true };
      return startMenu;
    },
    async startMenuRestorePins() {
      await wait(700);
      return startMenu;
    },
  };
}
export const debloatApi: DebloatApi = isTauri() ? tauriApi : previewApi();
