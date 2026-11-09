import { invoke } from "@tauri-apps/api/core";
import type { Filters } from "./model";

export interface Game {
  appid: number;
  name: string;
  coverUrl: string | null;
  storeUrl: string;
  reviewPercent: number;
  reviewCount: number;
  isFree: boolean;
  price: string | null;
  originalPrice: string | null;
  discountPercent: number;
  dealEndsAt: number | null;
  onlineCoop: boolean;
  singlePlayer: boolean;
  story: boolean;
}

export type OfferKind = "freeWeekend" | "freeToKeep";

export interface FreeOffer {
  game: Game;
  kind: OfferKind;
  label: string;
  startsAt: number | null;
  endsAt: number | null;
  offerId: string;
}

export interface Section<T> {
  items: T[];
  updatedAt: number | null;
  stale: boolean;
  error: string | null;
  /** Recent enough that Steam need not be asked. */
  fresh: boolean;
}

export type RowKind = "deals" | "freeToPlay";

export interface MorePage {
  games: Game[];
  nextCursor: number | null;
}

export const steamHelperApi = {
  free: (force: boolean, saved = false) => invoke<Section<FreeOffer>>("steam_helper_free", { force, saved }),
  picks: (filters: Filters, force: boolean, saved = false) => invoke<Section<Game>>("steam_helper_picks", { filters, force, saved }),
  row: (kind: RowKind, filters: Filters, force: boolean, saved = false) => invoke<Section<Game>>("steam_helper_row", { kind, filters, force, saved }),
  install: (appid: number) => invoke<void>("steam_helper_install", { appid }),
  ownedGames: () => invoke<{ games: { appid: number }[] }>("steam_automation_library", { force: false }),
  claim: (appid: number) => invoke<"added" | "alreadyOwned">("steam_automation_claim", { appid }),
  more: (kind: RowKind, filters: Filters, cursor: number) => invoke<MorePage>("steam_helper_more", { kind, filters, cursor }),
};
