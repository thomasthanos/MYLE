// Typed bridge to the setup crate (backend/installer/src/ui.rs).
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

export interface SetupState {
  mode: "install" | "uninstall";
  product: string;
  /** What this setup installs; the installed version when uninstalling. */
  version: string;
  installed: { version: string | null; dir: string } | null;
  dir: string;
  /** Bytes on disk once installed. */
  size: number;
  shortcuts: { desktop: boolean; startMenu: boolean; startup: boolean; taskbar: boolean; startMinimized: boolean };
  /** `/P`: installs at once and closes by itself. */
  passive: boolean;
  /** False for a setup built without the app inside. */
  ready: boolean;
  /** The browser extension's download page, offered when there is one. */
  extensionUrl: string | null;
}

export type Stage =
  | "closingApp"
  | "preparing"
  | "copying"
  | "registering"
  | "shortcuts"
  | "removingFiles"
  | "removingData"
  | "finishing";

export type Progress =
  | { event: "stage"; data: { stage: Stage } }
  | { event: "files"; data: { done: number; total: number; file: string } };

export interface InstallRequest {
  desktop: boolean;
  startMenu: boolean;
  startup: boolean;
  taskbar: boolean;
  /** Started by Windows at sign-in: minimized to the taskbar, or on screen. */
  startMinimized: boolean;
}

export interface InstallDone {
  /** The install folder. */
  dir: string;
  /** Shortcuts Windows would not take; the app itself is installed. */
  warnings: string[];
}

export interface SetupApi {
  state(): Promise<SetupState>;
  running(dir: string): Promise<string[]>;
  checkFolder(dir: string): Promise<void>;
  install(request: InstallRequest, onEvent: (e: Progress) => void): Promise<InstallDone>;
  uninstall(removeData: boolean, onEvent: (e: Progress) => void): Promise<void>;
  launch(): Promise<void>;
  /** Opens the extension's download page in the default browser. */
  openExtension(): Promise<void>;
  exit(): Promise<void>;
}

function channel(onEvent: (e: Progress) => void): Channel<Progress> {
  const events = new Channel<Progress>();
  events.onmessage = onEvent;
  return events;
}

const tauriApi: SetupApi = {
  state: () => invoke("setup_state"),
  running: (dir) => invoke("setup_running", { dir }),
  checkFolder: (dir) => invoke("setup_check_folder", { dir }),
  install: (request, onEvent) => invoke("setup_install", { request, onEvent: channel(onEvent) }),
  uninstall: (removeData, onEvent) =>
    invoke("setup_uninstall", { request: { removeData }, onEvent: channel(onEvent) }),
  launch: () => invoke("setup_launch"),
  openExtension: () => invoke("setup_open_extension"),
  exit: () => invoke("setup_exit"),
};

/** The real setup inside the app; in a dev browser, `?demo=` picks a fake. */
export async function loadApi(): Promise<SetupApi | null> {
  if (isTauri()) return tauriApi;
  const demo = new URLSearchParams(location.search).get("demo");
  if (import.meta.env.DEV && demo !== null) {
    const { previewApi } = await import("./preview");
    return previewApi(demo);
  }
  return null;
}
