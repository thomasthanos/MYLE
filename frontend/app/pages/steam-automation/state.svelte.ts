import { toast } from "../../../lib/toast.svelte";
import { steamAutomationApi as api, type BadgeGame, type Feature, type InventoryItem, type LibraryGame, type Listing, type Settings, type Status } from "./api";
import { duplicateIds, filterItems, freshLowest, IDLE_LIMIT, listable, listEach, messageOf, plural, requestedPrice, sellable, sortItems, toggleInList, type GameSort, type ItemFilter, type ItemMarks, type ItemSort, type JobResult, type Lowest } from "./model";

const POLL_MS = 5_000;
/** Long enough for the backend to have acted on a change, so the next look shows it. */
const SETTLE_MS = 500;

export type View = "games" | "cards" | "achievements" | "inventory";
export type GamesShown = "all" | "favorites" | "idling";
export type BulkKind = "selected" | "all" | "duplicates";

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
  /** Selling in bulk needs to know what is already on the Market. */
  listingsKnown = $state(false);
  selected = $state<Set<string>>(new Set());
  /** What the owner typed in each item's price field. Empty means the Market's price at the time. */
  prices = $state<Record<string, string>>({});
  lowest = $state<Record<string, Lowest>>({});
  pricing = $state<Set<string>>(new Set());
  /** Listed since the inventory was last read. */
  justListed = $state<Set<string>>(new Set());
  itemFilter = $state<ItemFilter>("all");
  itemSort = $state<ItemSort>("name");
  itemQuery = $state("");
  job = $state<Job | null>(null);

  find = $state({
    games: { query: "", sort: "playtime" as GameSort, show: "all" as GamesShown },
    achievements: { query: "", sort: "playtime" as GameSort, show: "all" as "all" | "queue" },
  });

  onSale = $derived(new Set([...this.listings.map((listing) => listing.assetId), ...this.justListed]));
  locked = $derived(new Set(this.settings?.lockedAssets ?? []));
  marks = $derived<ItemMarks>({ locked: this.locked, onSale: this.onSale, duplicates: duplicateIds(this.inventory ?? [], this.onSale) });
  shownItems = $derived(sortItems(filterItems(this.inventory ?? [], this.itemFilter, this.itemQuery, this.marks), this.itemSort));
  /** Steam Guard is the owner's to do, wherever in the section they are. */
  waitingConfirmation = $derived(this.listings.filter((listing) => listing.waitingConfirmation).length);

  #timer: ReturnType<typeof setInterval> | undefined;
  #stopJob = false;
  /** Catches up at once when the window comes back from the tray. */
  #onVisible = () => {
    if (document.visibilityState === "visible") void this.refresh();
  };

  start() {
    void this.refresh();
    void this.#loadSettings();
    void this.loadLibrary(false);
    void this.#readListings();
    clearInterval(this.#timer);
    // Hidden in the tray, nobody is looking: no point asking every few seconds.
    this.#timer = setInterval(() => {
      if (document.visibilityState === "visible") void this.refresh();
    }, POLL_MS);
    document.addEventListener("visibilitychange", this.#onVisible);
  }

  stop() {
    clearInterval(this.#timer);
    document.removeEventListener("visibilitychange", this.#onVisible);
  }

  async refresh() {
    try {
      const before = this.status?.inventory.waitingConfirmation ?? 0;
      this.status = await api.status();
      this.error = null;
      // An automatic run has just put more on the Market.
      if (this.status.inventory.waitingConfirmation !== before) void this.#readListings();
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
      await this.#readListings();
    });
    this.signingIn = false;
  }

  async setPaused(paused: boolean) {
    if (paused) this.stopJob();
    if (this.settings) this.settings = { ...this.settings, paused };
    await this.#attempt(() => api.pause(paused));
    this.#settle();
  }

  async setStarted(feature: Feature, on: boolean) {
    if (this.status) this.status = { ...this.status, started: { ...this.status.started, [feature]: on } };
    await this.#attempt(() => api.setStarted(feature, on));
    this.#settle();
  }

  isIdling(appid: number): boolean {
    return !!this.status?.manual.includes(appid) || (!!this.status?.started.idle && !!this.settings?.idleGames.includes(appid));
  }

  /** Runs now, and is remembered for when MYLE next opens if that switch is on. */
  async idleGame(appid: number, on: boolean) {
    if (!this.settings) return;
    const others = this.settings.idleGames.filter((id) => id !== appid);
    if (on && others.length >= IDLE_LIMIT) {
      toast.info(`Steam runs up to ${IDLE_LIMIT} games at once.`);
      return;
    }
    void this.change({ idleGames: on ? [...others, appid] : others });
    await this.#attempt(() => api.idleGame(appid, on));
    this.#settle();
  }

  async stopAll() {
    this.stopJob();
    void this.change({ idleGames: [] });
    await this.#attempt(() => api.stopAll());
    this.#settle();
  }

  toggleFavorite(appid: number) {
    if (this.settings) void this.change({ favorites: toggleInList(this.settings.favorites, appid) });
  }

  /** What the achievements window found or changed, so the game's tile shows it at once. */
  noteAchievements(appid: number, progress: { unlocked: number; total: number }) {
    if (!this.library?.some((game) => game.appid === appid)) return;
    this.library = this.library.map((game) => (game.appid === appid ? { ...game, achievementsUnlocked: progress.unlocked, achievementsTotal: progress.total } : game));
  }

  toggleAchievementGame(appid: number) {
    if (this.settings) void this.change({ achievementGames: toggleInList(this.settings.achievementGames, appid) });
  }

  toggleSkipped(appid: number) {
    if (this.settings) void this.change({ cardBlacklist: toggleInList(this.settings.cardBlacklist, appid) });
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
      this.inventory = sellable(await api.inventory(force));
      this.listings = await api.listings();
      this.listingsKnown = true;
      this.justListed = new Set();
    } catch (error) {
      this.listingsKnown = false;
      this.inventoryError = messageOf(error);
    } finally {
      this.inventoryLoading = false;
    }
  }

  /** Quietly: a failure here is said by the sign-in state, not by an error of its own. */
  async #readListings() {
    try {
      this.listings = await api.listings();
      this.listingsKnown = true;
    } catch {
      this.listingsKnown = false;
    }
  }

  lowestOf(hashName: string, now: number) {
    return freshLowest(this.lowest[hashName], now);
  }

  async lookupPrice(item: InventoryItem) {
    this.pricing = new Set([...this.pricing, item.assetId]);
    await this.#attempt(async () => {
      const cents = await api.price(item.hashName);
      this.lowest = { ...this.lowest, [item.hashName]: { cents, at: Date.now() / 1000 } };
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

  bulkItems(kind: BulkKind): InventoryItem[] {
    const can = listable(this.inventory ?? [], this.locked, this.onSale);
    if (kind === "selected") return can.filter((item) => this.selected.has(item.assetId));
    if (kind === "duplicates") return can.filter((item) => this.marks.duplicates.has(item.assetId));
    return listable(this.shownItems, this.locked, this.onSale);
  }

  /** The inventory may be days old by now, so "one of each is kept" is worked out again first. */
  async listBulk(kind: BulkKind, label: string) {
    if (this.job) return;
    await this.loadInventory(true);
    if (!this.listingsKnown) return;
    await this.listItems(this.bulkItems(kind), label);
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
    await this.#readListings();
  }

  async removeListings(listings: Listing[]) {
    const result = await this.#run("Removing listings", listings, async (listing) => {
      await api.removeListing(listing.listingId, listing.assetId);
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
