import { nav } from "../../../lib/nav.svelte";
import { readJson, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { account } from "../../account/account.svelte";
import { steamHelperApi, type FreeOffer, type Game, type RowKind, type Section } from "./api";
import { appendUnique, createLatest, debounce, DEFAULT_FILTERS, validFilters, type Filters } from "./model";

const FILTERS_KEY = "myle.steamHelper.filters";
/** Quick clicks on several chips ask Steam once, for the last combination. */
const FILTER_QUIET_MS = 300;

export type SeeAllSection = "picks" | "deals" | "freeToPlay";

export interface SectionState<T> extends Section<T> {
  loading: boolean;
}

const notLoaded = <T>(): SectionState<T> => ({ items: [], updatedAt: null, stale: false, error: null, fresh: false, loading: true });
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
  /** Only the owner's account can have games added from here. */
  owned = $state<Set<number>>(new Set());
  adding = $state<Set<number>>(new Set());

  #cursor = 0;
  #latest = {
    free: createLatest(),
    picks: createLatest(),
    deals: createLatest(),
    freeToPlay: createLatest(),
    more: createLatest(),
  };

  #reloadSoon = debounce(() => {
    this.#loadFiltered(false);
    if (this.seeAll && this.seeAll !== "picks") this.#restartMore();
  }, FILTER_QUIET_MS);

  load(force = false) {
    void this.#loadFree(force);
    this.#loadFiltered(force);
    void this.#loadOwned();
  }

  async #loadOwned() {
    if (!account.owner) return;
    try {
      const answer = await steamHelperApi.ownedGames();
      this.owned = new Set(answer.games.map((game) => game.appid));
    } catch {
      // Not signed in to Steam yet: every Add button is still there and says so if pressed.
    }
  }

  async addToAccount(appid: number, name: string) {
    this.adding = new Set([...this.adding, appid]);
    try {
      const result = await steamHelperApi.claim(appid);
      this.owned = new Set([...this.owned, appid]);
      toast.success(result === "added" ? `${name} is now in your Steam library.` : `${name} is already in your Steam library.`);
    } catch (error) {
      const message = messageOf(error);
      toast.error(message, message.toLowerCase().includes("sign in") ? { label: "Open Steam Automation", run: () => nav.go("steam-automation") } : undefined);
    } finally {
      const left = new Set(this.adding);
      left.delete(appid);
      this.adding = left;
    }
  }

  async install(appid: number) {
    try {
      await steamHelperApi.install(appid);
    } catch (error) {
      toast.error(messageOf(error));
    }
  }

  setFilter(name: keyof Filters, value: boolean) {
    this.filters = { ...this.filters, [name]: value };
    writeJson(FILTERS_KEY, this.filters);
    // Show at once that the lists are changing, and drop any answer still on its way for the old filters.
    for (const section of ["picks", "deals", "freeToPlay"] as const) {
      this.#latest[section].next();
      this[section] = notLoaded();
    }
    this.#latest.more.next();
    this.moreGames = [];
    this.moreLoading = this.seeAll !== null && this.seeAll !== "picks";
    this.#reloadSoon();
  }

  openSeeAll(section: SeeAllSection) {
    this.seeAll = section;
    if (section !== "picks") this.#restartMore();
  }

  back() {
    this.seeAll = null;
    this.#latest.more.next();
    this.moreGames = [];
    this.moreLoading = false;
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
    if (!force) {
      // What was saved shows at once. Steam is only waited for when that is out of date.
      const saved = await steamHelperApi.free(false, true).catch(() => null);
      if (!latest.isCurrent(request)) return;
      if (saved?.items.length) this.free = { ...saved, loading: !saved.fresh };
      if (saved?.fresh && saved.items.length) return;
    }
    try {
      const section = await steamHelperApi.free(force);
      if (latest.isCurrent(request)) this.free = { ...section, loading: false };
    } catch (error) {
      if (latest.isCurrent(request)) this.free = { ...this.free, error: messageOf(error), loading: false };
    }
  }

  #loadFiltered(force: boolean) {
    const filters = this.filters;
    void this.#loadGames("picks", force, (saved) => steamHelperApi.picks(filters, force, saved));
    void this.#loadGames("deals", force, (saved) => steamHelperApi.row("deals", filters, force, saved));
    void this.#loadGames("freeToPlay", force, (saved) => steamHelperApi.row("freeToPlay", filters, force, saved));
  }

  async #loadGames(name: SeeAllSection, force: boolean, ask: (saved: boolean) => Promise<Section<Game>>) {
    const latest = this.#latest[name];
    const request = latest.next();
    this[name].loading = true;
    if (!force) {
      const saved = await ask(true).catch(() => null);
      if (!latest.isCurrent(request)) return;
      if (saved?.items.length) this[name] = { ...saved, loading: !saved.fresh };
      if (saved?.fresh && saved.items.length) return;
    }
    try {
      const section = await ask(false);
      if (latest.isCurrent(request)) this[name] = { ...section, loading: false };
    } catch (error) {
      if (latest.isCurrent(request)) this[name] = { ...this[name], error: messageOf(error), loading: false };
    }
  }
}

export const steamHelperState = new SteamHelperState();
