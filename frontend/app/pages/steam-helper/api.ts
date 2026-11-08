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
  endsAt: number | null;
  offerId: string;
}

export interface Section<T> {
  items: T[];
  updatedAt: number | null;
  stale: boolean;
  error: string | null;
}

export type RowKind = "deals" | "freeToPlay";

export interface MorePage {
  games: Game[];
  nextCursor: number | null;
}

export const steamHelperApi = {
  free: (force: boolean) => invoke<Section<FreeOffer>>("steam_helper_free", { force }),
  picks: (filters: Filters, force: boolean) => invoke<Section<Game>>("steam_helper_picks", { filters, force }),
  row: (kind: RowKind, filters: Filters, force: boolean) => invoke<Section<Game>>("steam_helper_row", { kind, filters, force }),
  more: (kind: RowKind, filters: Filters, cursor: number) => invoke<MorePage>("steam_helper_more", { kind, filters, cursor }),
};
