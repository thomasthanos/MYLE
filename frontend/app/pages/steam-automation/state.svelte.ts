import { toast } from "../../../lib/toast.svelte";
import { steamAutomationApi as api, type BadgeGame, type Feature, type InventoryItem, type LibraryGame, type Listing, type Settings, type Status } from "./api";
import { listEach, messageOf, plural, priceText, requestedPrice, suggestedPrice, toggleInList, type GameSort, type ItemFilter, type ItemSort, type JobResult } from "./model";

const POLL_MS = 5_000;
/** Long enough for the backend to have acted on a change, so the next look shows it. */
const SETTLE_MS = 500;

export type View = "games" | "idling" | "favorites" | "cards" | "achievements" | "idler" | "inventory";
export type CardSort = "drops" | "name" | "playtime";
type Queue = "idleGames" | "achievementGames";

interface Job {
  label: string;
  total: number;
  progress: JobResult;
}

class SteamAutomationState {
  view = $state<View>("games");
  status = $state<Status | null>(null);
  settings = $state<Settings | null>(null);
  signingIn = $state(false);
  /** Reading the status failed. Cleared by the next answer. */
  error = $state<string | null>(null);
  /** Something the owner asked for failed. Cleared by their next action. */
  actionError = $state<string | null>(null);

  library = $state<LibraryGame[] | null>(null);
  libraryLoading = $state(false);
  libraryError = $state<string | null>(null);
  libraryProblem = $state<string | null>(null);

  drops = $state<BadgeGame[] | null>(null);
  dropsLoading = $state(false);
  dropsError = $state<string | null>(null);

  inventory = $state<InventoryItem[] | null>(null);
  inventoryLoading = $state(false);
  inventoryError = $state<string | null>(null);
  listings = $state<Listing[]>([]);
  selected = $state<Set<string>>(new Set());
  /** What is typed in each item's price field. */
  prices = $state<Record<string, string>>({});
  /** The Market's cheapest price by item name, null when it has none. */
  lowest = $state<Record<string, number | null>>({});
  pricing = $state<Set<string>>(new Set());
  /** Listed since the last look at the Market. */
  justListed = $state<Set<string>>(new Set());
  itemFilter = $state<ItemFilter>("all");
  itemSort = $state<ItemSort>("name");
  itemQuery = $state("");
  job = $state<Job | null>(null);

  find = $state({
    games: { query: "", sort: "playtime" as GameSort },
    favorites: { query: "", sort: "playtime" as GameSort },
    achievements: { query: "", sort: "playtime" as GameSort, tab: "all" as "all" | "queue" },
    idler: { query: "", sort: "playtime" as GameSort, tab: "all" as "all" | "queue" },
    cards: { query: "", sort: "drops" as CardSort, tab: "drops" as "drops" | "whitelist" | "blacklist" },
  });

  onSale = $derived(new Set([...this.listings.map((listing) => listing.assetId), ...this.justListed]));
  locked = $derived(new Set(this.settings?.lockedAssets ?? []));

  #timer: ReturnType<typeof setInterval> | undefined;
  #stopJob = false;

  start() {
    void this.refresh();
    void this.#loadSettings();
    void this.loadLibrary(false);
    clearInterval(this.#timer);
    this.#timer = setInterval(() => void this.refresh(), POLL_MS);
  }

  stop() {
    clearInterval(this.#timer);
  }

  async refresh() {
    try {
      this.status = await api.status();
      this.error = null;
    } catch (error) {
      this.error = messageOf(error);
    }
  }

  /** Lets the backend act on a change, then looks again. */
  #settle() {
    setTimeout(() => void this.refresh(), SETTLE_MS);
  }

  async #loadSettings() {
    try {
      this.settings = await api.settings();
    } catch (error) {
      this.error = messageOf(error);
    }
  }

  async #attempt(action: () => Promise<unknown>) {
    this.actionError = null;
    try {
      await action();
    } catch (error) {
      this.actionError = messageOf(error);
    }
  }

  async change(change: Partial<Settings>) {
    if (!this.settings) return;
    const next = { ...this.settings, ...change };
    this.settings = next;
    await this.#attempt(() => api.setSettings(next));
    this.#settle();
  }

  async signIn() {
    this.signingIn = true;
    await this.#attempt(async () => {
      await api.signIn();
      await this.refresh();
      await this.loadLibrary(true);
    });
    this.signingIn = false;
  }

  async setPaused(paused: boolean) {
    if (this.settings) this.settings = { ...this.settings, paused };
    await this.#attempt(() => api.pause(paused));
    this.#settle();
  }

  async setStarted(feature: Feature, on: boolean) {
    if (this.status) this.status = { ...this.status, started: { ...this.status.started, [feature]: on } };
    await this.#attempt(() => api.setStarted(feature, on));
    this.#settle();
  }

  async idleGame(appid: number, on: boolean) {
    await this.#attempt(() => api.idleGame(appid, on));
    this.#settle();
  }

  async stopAll() {
    await this.#attempt(() => api.stopAll());
    this.#settle();
  }

  toggleFavorite(appid: number) {
    if (this.settings) void this.change({ favorites: toggleInList(this.settings.favorites, appid) });
  }

  toggleQueue(queue: Queue, appid: number) {
    if (this.settings) void this.change({ [queue]: toggleInList(this.settings[queue], appid) });
  }

  /** A game is on one list at most, so moving it to one takes it off the other. */
  toggleCardList(list: "cardWhitelist" | "cardBlacklist", appid: number) {
    if (!this.settings) return;
    const other = list === "cardWhitelist" ? "cardBlacklist" : "cardWhitelist";
    void this.change({ [list]: toggleInList(this.settings[list], appid), [other]: this.settings[other].filter((id) => id !== appid) });
  }

  toggleLocked(assetId: string) {
    if (this.settings) void this.change({ lockedAssets: toggleInList(this.settings.lockedAssets, assetId) });
  }

  toggleSelected(assetId: string) {
    const next = new Set(this.selected);
    if (!next.delete(assetId)) next.add(assetId);
    this.selected = next;
  }

  async loadLibrary(force: boolean) {
    if (this.libraryLoading || (this.library && !force)) return;
    this.libraryLoading = true;
    this.libraryError = null;
    try {
      const answer = await api.library(force);
      this.library = answer.games;
      this.libraryProblem = answer.problem;
    } catch (error) {
      this.libraryError = messageOf(error);
    } finally {
      this.libraryLoading = false;
    }
  }

  async loadDrops(force: boolean) {
    if (this.dropsLoading || (this.drops && !force)) return;
    this.dropsLoading = true;
    this.dropsError = null;
    try {
      this.drops = await api.drops(force);
    } catch (error) {
      this.dropsError = messageOf(error);
    } finally {
      this.dropsLoading = false;
    }
  }

  async loadInventory(force: boolean) {
    if (this.inventoryLoading || (this.inventory && !force)) return;
    this.inventoryLoading = true;
    this.inventoryError = null;
    try {
      this.inventory = await api.inventory(force);
      await this.#loadListings();
      this.justListed = new Set();
    } catch (error) {
      this.inventoryError = messageOf(error);
    } finally {
      this.inventoryLoading = false;
    }
  }

  async #loadListings() {
    this.listings = await api.listings();
  }

  async lookupPrice(item: InventoryItem) {
    this.pricing = new Set([...this.pricing, item.assetId]);
    await this.#attempt(async () => {
      const cents = await api.price(item.hashName);
      this.lowest = { ...this.lowest, [item.hashName]: cents };
      if (cents !== null) this.prices = { ...this.prices, [item.assetId]: priceText(suggestedPrice(cents, this.settings?.minimumCents ?? 1)) };
    });
    const left = new Set(this.pricing);
    left.delete(item.assetId);
    this.pricing = left;
  }

  async #run<T>(label: string, items: T[], act: (item: T) => Promise<{ listed: boolean; waitingConfirmation: boolean }>): Promise<JobResult | null> {
    if (this.job || items.length === 0) return null;
    this.actionError = null;
    this.#stopJob = false;
    this.job = { label, total: items.length, progress: { done: 0, listed: 0, waiting: 0, skipped: 0, error: null, stopped: false } };
    const result = await listEach(items, act, (progress) => {
      if (this.job) this.job.progress = progress;
    }, () => this.#stopJob);
    this.job = null;
    if (result.error) this.actionError = result.error;
    return result;
  }

  stopJob() {
    this.#stopJob = true;
  }

  async listItems(items: InventoryItem[], label: string) {
    const result = await this.#run(label, items, async (item) => {
      const buyerCents = requestedPrice(this.prices[item.assetId]);
      // A mistyped price is not left for the Market to fill in: the owner meant a number.
      if (buyerCents === "invalid") return { listed: false, waitingConfirmation: false };
      const outcome = await api.listItem({ assetId: item.assetId, hashName: item.hashName, buyerCents });
      if (outcome.listed) {
        this.justListed = new Set([...this.justListed, item.assetId]);
        const left = new Set(this.selected);
        left.delete(item.assetId);
        this.selected = left;
      } else if (items.length === 1 && outcome.reason) {
        toast.error(`${item.name}: ${outcome.reason}`);
      }
      return outcome;
    });
    if (!result) return;
    if (result.listed > 0) {
      const waiting = result.waiting > 0 ? ` Confirm ${result.waiting === 1 ? "it" : "them"} in Steam Guard.` : "";
      toast.success(`Listed ${plural(result.listed, "item")}.${waiting}`);
    }
    if (result.skipped > 0 && items.length > 1) toast.info(`${plural(result.skipped, "item")} could not be listed.`);
    await this.#attempt(() => this.#loadListings());
  }

  async removeListings(listings: Listing[]) {
    const result = await this.#run("Removing listings", listings, async (listing) => {
      await api.removeListing(listing.listingId);
      this.listings = this.listings.filter((entry) => entry.listingId !== listing.listingId);
      const left = new Set(this.justListed);
      left.delete(listing.assetId);
      this.justListed = left;
      return { listed: true, waitingConfirmation: false };
    });
    if (result && result.listed > 0) toast.success(`Removed ${plural(result.listed, "listing")}.`);
  }
}

export const steamAutomationState = new SteamAutomationState();
