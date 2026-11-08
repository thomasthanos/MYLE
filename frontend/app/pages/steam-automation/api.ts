import { invoke } from "@tauri-apps/api/core";
import type { WaitingView } from "./model";

export interface Status {
  ready: boolean;
  steamRunning: boolean;
  communitySignedIn: boolean;
  communityOk: boolean;
  waiting: WaitingView | null;
  purchaseDatesUnknown: boolean;
  running: number[];
  farm: { phase: "building" | "solo" | "done" | ""; gamesLeft: number; dropsLeft: number; skippedRefundable: number; nextCheckAt: number | null };
  achievements: { total: number; locked: number; nextAt: number | null; waitingForFarming: boolean };
  inventory: { listed: number; cents: number; waitingConfirmation: number; nextAt: number | null };
  activity: { at: number; text: string }[];
}

export interface Settings {
  farmOn: boolean;
  idleOn: boolean;
  achievementsOn: boolean;
  inventoryOn: boolean;
  idleGames: number[];
  achievementGames: number[];
  minimumCents: number;
}

export interface LibraryGame {
  appid: number;
  name: string;
  coverUrl: string | null;
  achievementsOk: boolean;
}

export const steamAutomationApi = {
  status: () => invoke<Status>("steam_automation_status"),
  signIn: () => invoke<void>("steam_automation_sign_in"),
  settings: () => invoke<Settings>("steam_automation_settings"),
  setSettings: (settings: Settings) => invoke<void>("steam_automation_set_settings", { settings }),
  pause: (paused: boolean) => invoke<void>("steam_automation_pause", { paused }),
  library: () => invoke<LibraryGame[]>("steam_automation_library"),
};
