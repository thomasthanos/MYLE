// The page's pure logic: how status, prices and times read, and which games a picker offers.
export interface WaitingView {
  kind: "noSteam" | "otherAccount" | "realGame" | "paused";
  appid: number | null;
}

export type PickerKind = "idle" | "achievements";

const WAITING: Record<WaitingView["kind"], string> = {
  realGame: "Paused while you play. Everything picks up again when you close your game.",
  noSteam: "Steam is not running.",
  otherAccount: "Steam and Steam Community are signed in to different accounts, so nothing is running.",
  paused: "Paused. Nothing runs until you resume.",
};

export function waitingText(waiting: WaitingView | null): string {
  return waiting ? WAITING[waiting.kind] : "";
}

export function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

export function euros(cents: number): string {
  return `${Math.floor(cents / 100)},${String(cents % 100).padStart(2, "0")}€`;
}

export function validMinimum(cents: number): boolean {
  return Number.isInteger(cents) && cents >= 1;
}

export function pickable<T extends { name: string; achievementsOk: boolean }>(games: T[], kind: PickerKind, query: string): T[] {
  const wanted = query.trim().toLowerCase();
  return games.filter((game) => (kind === "idle" || game.achievementsOk) && game.name.toLowerCase().includes(wanted));
}

export function untilText(at: number | null, now: number): string {
  if (at === null) return "";
  const seconds = at - now;
  if (seconds <= 0) return "soon";
  if (seconds < 3_600) return `in ${Math.ceil(seconds / 60)} min`;
  return `in ${Math.floor(seconds / 3_600)} h`;
}

export function farmLine(farm: { phase: string; gamesLeft: number; dropsLeft: number }): string {
  if (farm.phase === "done") return "No cards left to drop";
  const cards = `${farm.dropsLeft} card${farm.dropsLeft === 1 ? "" : "s"}`;
  const games = farm.gamesLeft === 1 ? "in 1 game" : `across ${farm.gamesLeft} games`;
  return `${cards} left to drop, ${games}`;
}
