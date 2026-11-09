import { invoke } from "@tauri-apps/api/core";
import type { WaitingView } from "./model";

export type Feature = "farm" | "idle" | "achievements";

export interface Status {
  ready: boolean;
  steamRunning: boolean;
  communitySignedIn: boolean;
  communityOk: boolean;
  waiting: WaitingView | null;
  purchaseDatesUnknown: boolean;
  started: Record<Feature, boolean>;
  manual: number[];
  idleSkipped: number;
  storeStale: boolean;
  running: number[];
  farm: { phase: "building" | "solo" | "reading" | "signedOut" | "done" | ""; gamesLeft: number; dropsLeft: number; skippedRefundable: number; nextCheckAt: number | null };
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
  favorites: number[];
  cardWhitelist: number[];
  cardBlacklist: number[];
  lockedAssets: string[];
  paused: boolean;
}

export interface LibraryGame {
  appid: number;
  name: string;
  coverUrl: string | null;
  headerUrl: string | null;
  minutes: number;
  achievementsUnlocked: number;
  achievementsTotal: number;
  achievementsOk: boolean;
  shared: boolean;
}

export interface LibraryAnswer {
  games: LibraryGame[];
  savedAt: number;
  problem: string | null;
}

export interface BadgeGame {
  appid: number;
  name: string;
  drops: number;
  minutesPlayed: number;
}

export interface InventoryItem {
  assetId: string;
  hashName: string;
  name: string;
  game: string;
  kind: string;
  iconUrl: string | null;
  marketable: boolean;
  amount: number;
}

export interface ListRequest {
  assetId: string;
  hashName: string;
  buyerCents: number | null;
}

export interface ListOutcome {
  listed: boolean;
  buyerCents: number;
  waitingConfirmation: boolean;
  reason: string | null;
}

export interface Listing {
  listingId: string;
  assetId: string;
  hashName: string;
  name: string;
  iconUrl: string | null;
  buyerCents: number;
  waitingConfirmation: boolean;
}

export const steamAutomationApi = {
  status: () => invoke<Status>("steam_automation_status"),
  signIn: () => invoke<void>("steam_automation_sign_in"),
  settings: () => invoke<Settings>("steam_automation_settings"),
  setSettings: (settings: Settings) => invoke<void>("steam_automation_set_settings", { settings }),
  pause: (paused: boolean) => invoke<void>("steam_automation_pause", { paused }),
  setStarted: (feature: Feature, on: boolean) => invoke<void>("steam_automation_set_started", { feature, on }),
  idleGame: (appid: number, on: boolean) => invoke<void>("steam_automation_idle_game", { appid, on }),
  stopAll: () => invoke<void>("steam_automation_stop_all"),
  library: (force: boolean) => invoke<LibraryAnswer>("steam_automation_library", { force }),
  drops: (force: boolean) => invoke<BadgeGame[]>("steam_automation_drops", { force }),
  inventory: (force: boolean) => invoke<InventoryItem[]>("steam_automation_inventory", { force }),
  price: (hashName: string) => invoke<number | null>("steam_automation_price", { hashName }),
  listItem: (item: ListRequest) => invoke<ListOutcome>("steam_automation_list_item", { item }),
  listings: () => invoke<Listing[]>("steam_automation_listings"),
  removeListing: (listingId: string) => invoke<void>("steam_automation_remove_listing", { listingId }),
};
