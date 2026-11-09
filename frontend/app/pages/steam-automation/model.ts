// The page's pure logic: how status, prices and times read, and how games and items are sorted and filtered.
export interface WaitingView {
  kind: "noSteam" | "otherAccount" | "realGame" | "paused";
  appid: number | null;
}

export type GameSort = "playtime" | "name" | "achievements";
export type ItemSort = "name" | "game" | "kind";
export type ItemFilter = "all" | "card" | "foil" | "background" | "emoticon" | "booster" | "onSale" | "duplicates" | "locked";
export type RunningSource = "hand" | "idler" | "farming";

export const IDLE_LIMIT = 32;
/** A price looked up longer ago than this says little about the Market now. */
const LOWEST_KEEP_SECS = 600;

/** The cheapest price a lookup found, null when the Market had none, and when it looked. */
export interface Lowest {
  cents: number | null;
  at: number;
}
export const GAME_SORTS: { value: GameSort; label: string }[] = [
  { value: "playtime", label: "Playtime (high to low)" },
  { value: "name", label: "Name (A to Z)" },
  { value: "achievements", label: "Achievements (most complete)" },
];
export const ITEM_SORTS: { value: ItemSort; label: string }[] = [
  { value: "name", label: "Name (A to Z)" },
  { value: "game", label: "Game (A to Z)" },
  { value: "kind", label: "Type" },
];

interface GameLike {
  appid: number;
  name: string;
  minutes: number;
  achievementsUnlocked: number;
  achievementsTotal: number;
  achievementsOk: boolean;
}

interface ItemLike {
  assetId: string;
  hashName: string;
  name: string;
  game: string;
  kind: string;
  marketable: boolean;
}

export interface ItemMarks {
  locked: Set<string>;
  onSale: Set<string>;
  duplicates: Set<string>;
}

export interface JobResult {
  done: number;
  listed: number;
  waiting: number;
  skipped: number;
  error: string | null;
  stopped: boolean;
}

const WAITING: Record<WaitingView["kind"], string> = {
  realGame: "Paused while you play. Everything picks up again when you close your game.",
  noSteam: "Steam is not running.",
  otherAccount: "Steam and Steam Community are signed in to different accounts, so nothing is running.",
  paused: "Paused. Nothing runs until you resume.",
};

const KINDS: Record<string, string> = {
  card: "Trading card",
  foil: "Foil card",
  background: "Background",
  emoticon: "Emoticon",
  booster: "Booster pack",
};

export const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function waitingText(waiting: WaitingView | null): string {
  return waiting ? WAITING[waiting.kind] : "";
}

export function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

/** A price as it sits in its field: `0,31`. */
export function priceText(cents: number): string {
  return `${Math.floor(cents / 100)},${String(cents % 100).padStart(2, "0")}`;
}

export function euros(cents: number): string {
  return `${priceText(cents)}€`;
}

export function validMinimum(cents: number): boolean {
  return Number.isInteger(cents) && cents >= 1;
}

/** Cents from what was typed, or null when it is not a price. */
export function parseEuros(text: string): number | null {
  const match = /^(\d+)(?:[.,](\d{1,2}))?$/.exec(text.replace(/[\s€]/g, ""));
  if (!match) return null;
  const cents = Number(match[1]) * 100 + Number((match[2] ?? "").padEnd(2, "0"));
  return cents > 0 ? cents : null;
}

/** What a price field asks for: cents, null when left empty so the Market decides, or "invalid". */
export function requestedPrice(typed: string | undefined): number | null | "invalid" {
  if (!typed?.trim()) return null;
  return parseEuros(typed) ?? "invalid";
}

export function kindLabel(kind: string): string {
  return KINDS[kind] ?? "Item";
}

export function untilText(at: number | null, now: number): string {
  if (at === null) return "";
  const seconds = at - now;
  if (seconds <= 0) return "soon";
  if (seconds < 3_600) return `in ${Math.ceil(seconds / 60)} min`;
  return `in ${Math.floor(seconds / 3_600)} h`;
}

/** What the top of Card farming says: the plan before starting, what is happening once started. */
export function farmPanel(
  started: boolean,
  farm: { phase: string; gamesLeft: number; dropsLeft: number },
  cards: number,
  games: number,
  current: string | null,
): { title: string; detail: string } {
  if (!started) {
    if (cards === 0) return { title: "No cards left to collect", detail: "Games that can still drop trading cards will show up here." };
    return { title: `${plural(cards, "card")} to collect from ${plural(games, "game")}`, detail: "Press Start and MYLE plays these games in the background until every card has dropped. Untick a game to leave it out." };
  }
  switch (farm.phase) {
    case "reading":
      return { title: "Checking your games for cards", detail: "This takes a few seconds." };
    case "signedOut":
      return { title: "Waiting for you to sign in to Steam", detail: "MYLE cannot see your cards until you do." };
    case "building":
      return { title: `Warming up ${plural(farm.gamesLeft, "game")}`, detail: "Steam starts dropping cards after 2 hours of playtime, so these run together first." };
    case "solo":
      return { title: current ? `Farming ${current}` : "Farming your cards", detail: `${plural(farm.dropsLeft, "card")} left to collect.` };
    default:
      return { title: "All your cards have dropped", detail: "New games with cards will show up here." };
  }
}

export function playtimeText(minutes: number): string {
  if (minutes <= 0) return "";
  const hours = minutes / 60;
  if (hours < 10) return `${Math.max(hours, 0.1).toFixed(1)}h`;
  return `${Math.round(hours).toLocaleString("en-US")}h`;
}

export function achievementsText(game: { achievementsUnlocked: number; achievementsTotal: number }): string {
  return game.achievementsTotal > 0 ? `${game.achievementsUnlocked}/${game.achievementsTotal}` : "";
}

const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
const completion = (game: GameLike) => (game.achievementsTotal > 0 ? game.achievementsUnlocked / game.achievementsTotal : -1);

export function sortGames<T extends GameLike>(games: T[], sort: GameSort): T[] {
  const compare: Record<GameSort, (a: T, b: T) => number> = {
    playtime: (a, b) => b.minutes - a.minutes || byName(a, b),
    name: byName,
    achievements: (a, b) => completion(b) - completion(a) || byName(a, b),
  };
  return [...games].sort(compare[sort]);
}

export function searchGames<T extends { name: string }>(games: T[], query: string): T[] {
  const wanted = query.trim().toLowerCase();
  return wanted ? games.filter((game) => game.name.toLowerCase().includes(wanted)) : games;
}

export function toggleInList<T>(list: T[], value: T): T[] {
  return list.includes(value) ? list.filter((entry) => entry !== value) : [...list, value];
}

/** The queue with every game that can be unlocked added at its end. */
export function addAll(queue: number[], games: GameLike[]): number[] {
  return [...queue, ...games.filter((game) => game.achievementsOk && !queue.includes(game.appid)).map((game) => game.appid)];
}

export function canQueueIdle(queue: number[], appid: number): boolean {
  return queue.includes(appid) || queue.length < IDLE_LIMIT;
}

/** Every copy of a card but one. A copy on sale is on its way out, so it is never the one kept. */
export function duplicateIds(items: ItemLike[], onSale: Set<string>): Set<string> {
  const seen = new Set<string>();
  const found = new Set<string>();
  for (const item of items) {
    if (!item.marketable || (item.kind !== "card" && item.kind !== "foil") || onSale.has(item.assetId)) continue;
    if (seen.has(item.hashName)) found.add(item.assetId);
    else seen.add(item.hashName);
  }
  return found;
}

/** The inventory as the page shows it: only what could go on the Market. */
export function sellable<T extends ItemLike>(items: T[]): T[] {
  return items.filter((item) => item.marketable);
}

export function freshLowest(entry: Lowest | undefined, now: number): Lowest | null {
  return entry && now - entry.at < LOWEST_KEEP_SECS ? entry : null;
}

export function listable<T extends ItemLike>(items: T[], locked: Set<string>, onSale: Set<string>): T[] {
  return items.filter((item) => item.marketable && !locked.has(item.assetId) && !onSale.has(item.assetId));
}

function passes(item: ItemLike, filter: ItemFilter, marks: ItemMarks): boolean {
  switch (filter) {
    case "all":
      return true;
    case "onSale":
      return marks.onSale.has(item.assetId);
    case "duplicates":
      return marks.duplicates.has(item.assetId);
    case "locked":
      return marks.locked.has(item.assetId);
    default:
      return item.kind === filter;
  }
}

export function filterItems<T extends ItemLike>(items: T[], filter: ItemFilter, query: string, marks: ItemMarks): T[] {
  const wanted = query.trim().toLowerCase();
  return items.filter((item) => passes(item, filter, marks) && (!wanted || item.name.toLowerCase().includes(wanted) || item.game.toLowerCase().includes(wanted)));
}

export function sortItems<T extends ItemLike>(items: T[], sort: ItemSort): T[] {
  const compare: Record<ItemSort, (a: T, b: T) => number> = {
    name: byName,
    game: (a, b) => a.game.localeCompare(b.game, undefined, { sensitivity: "base" }) || byName(a, b),
    kind: (a, b) => a.kind.localeCompare(b.kind) || byName(a, b),
  };
  return [...items].sort(compare[sort]);
}

/** What put a running game there. */
export function runningSource(
  appid: number,
  status: { manual: number[]; started: { idle: boolean }; farm: { phase: string } },
  settings: { idleGames: number[] },
): RunningSource {
  if (status.manual.includes(appid)) return "hand";
  const farming = status.farm.phase === "building" || status.farm.phase === "solo";
  return status.started.idle && settings.idleGames.includes(appid) && !farming ? "idler" : "farming";
}

/** Goes through `items` one at a time, ending early when asked to or at the first failure. */
export async function listEach<T>(
  items: T[],
  act: (item: T) => Promise<{ listed: boolean; waitingConfirmation: boolean }>,
  onStep: (progress: JobResult) => void,
  stopped: () => boolean,
): Promise<JobResult> {
  const result: JobResult = { done: 0, listed: 0, waiting: 0, skipped: 0, error: null, stopped: false };
  for (const item of items) {
    if (stopped()) {
      result.stopped = true;
      break;
    }
    try {
      const outcome = await act(item);
      result.done += 1;
      if (outcome.listed) result.listed += 1;
      else result.skipped += 1;
      if (outcome.waitingConfirmation) result.waiting += 1;
    } catch (error) {
      result.error = messageOf(error);
      break;
    }
    onStep({ ...result });
  }
  return result;
}
