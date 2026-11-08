import { readJson, writeJson } from "../../../lib/storage";
import { steamHelperApi, type FreeOffer, type Game, type RowKind, type Section } from "./api";
import { appendUnique, createLatest, DEFAULT_FILTERS, validFilters, type Filters } from "./model";

const FILTERS_KEY = "myle.steamHelper.filters";

export type SeeAllSection = "picks" | "deals" | "freeToPlay";

export interface SectionState<T> extends Section<T> {
  loading: boolean;
}

const notLoaded = <T>(): SectionState<T> => ({ items: [], updatedAt: null, stale: false, error: null, loading: true });
const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Survives navigation: the page component is destroyed when you leave it. */
class SteamHelperState {
  filters = $state<Filters>(readJson(FILTERS_KEY, DEFAULT_FILTERS, validFilters));
  free = $state<SectionState<FreeOffer>>(notLoaded());
  picks = $state<SectionState<Game>>(notLoaded());
  deals = $state<SectionState<Game>>(notLoaded());
  freeToPlay = $state<SectionState<Game>>(notLoaded());
  seeAll = $state<SeeAllSection | null>(null);
  moreGames = $state<Game[]>([]);
  moreLoading = $state(false);
  moreError = $state<string | null>(null);
  moreDone = $state(false);

  #cursor = 0;
  #latest = {
    free: createLatest(),
    picks: createLatest(),
    deals: createLatest(),
    freeToPlay: createLatest(),
    more: createLatest(),
  };

  load(force = false) {
    void this.#loadFree(force);
    this.#loadFiltered(force);
  }

  setFilter(name: keyof Filters, value: boolean) {
    this.filters = { ...this.filters, [name]: value };
    writeJson(FILTERS_KEY, this.filters);
    this.#loadFiltered(false);
    if (this.seeAll && this.seeAll !== "picks") this.#restartMore();
  }

  openSeeAll(section: SeeAllSection) {
    this.seeAll = section;
    if (section !== "picks") this.#restartMore();
  }

  back() {
    this.seeAll = null;
    this.#latest.more.next();
    this.moreGames = [];
  }

  async loadMore() {
    const section = this.seeAll;
    if (!section || section === "picks" || this.moreLoading || this.moreDone) return;
    const latest = this.#latest.more;
    const request = latest.next();
    this.moreLoading = true;
    this.moreError = null;
    try {
      const page = await steamHelperApi.more(section as RowKind, this.filters, this.#cursor);
      if (!latest.isCurrent(request)) return;
      this.moreGames = appendUnique(this.moreGames, page.games);
      this.moreDone = page.nextCursor === null;
      this.#cursor = page.nextCursor ?? this.#cursor;
    } catch (error) {
      if (latest.isCurrent(request)) this.moreError = messageOf(error);
    } finally {
      if (latest.isCurrent(request)) this.moreLoading = false;
    }
  }

  #restartMore() {
    this.#latest.more.next();
    this.#cursor = 0;
    this.moreGames = [];
    this.moreDone = false;
    this.moreError = null;
    this.moreLoading = false;
    void this.loadMore();
  }

  async #loadFree(force: boolean) {
    const latest = this.#latest.free;
    const request = latest.next();
    this.free.loading = true;
    try {
      const section = await steamHelperApi.free(force);
      if (latest.isCurrent(request)) this.free = { ...section, loading: false };
    } catch (error) {
      if (latest.isCurrent(request)) this.free = { ...this.free, error: messageOf(error), loading: false };
    }
  }

  #loadFiltered(force: boolean) {
    const filters = this.filters;
    void this.#loadGames("picks", () => steamHelperApi.picks(filters, force));
    void this.#loadGames("deals", () => steamHelperApi.row("deals", filters, force));
    void this.#loadGames("freeToPlay", () => steamHelperApi.row("freeToPlay", filters, force));
  }

  async #loadGames(name: SeeAllSection, ask: () => Promise<Section<Game>>) {
    const latest = this.#latest[name];
    const request = latest.next();
    this[name].loading = true;
    try {
      const section = await ask();
      if (latest.isCurrent(request)) this[name] = { ...section, loading: false };
    } catch (error) {
      if (latest.isCurrent(request)) this[name] = { ...this[name], error: messageOf(error), loading: false };
    }
  }
}

export const steamHelperState = new SteamHelperState();
