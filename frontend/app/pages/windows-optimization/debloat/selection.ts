// What the page's choices mean, without Svelte: the Quick setup profiles,
// the pending changes and their upkeep. Tested in
// scripts/tests/debloat-selection.test.mjs.
import type { AppStatus, Level, TweakStatus } from "./api";
export type Profile = Level;
export const profiles: readonly Profile[] = ["light", "recommended", "maximum"];
const rank: Record<Level, number> = { light: 0, recommended: 1, maximum: 2 };
type TweakLike = Pick<TweakStatus, "id" | "level" | "state">;
type AppLike = Pick<AppStatus, "id" | "level" | "packages">;
/** Whether something of `level` is part of `profile`: each takes in the ones before it. */
export function inProfile(level: Level | null, profile: Profile): boolean {
  return level !== null && rank[level] <= rank[profile];
}
/** Whether a tweak is in place, as its switch shows it before any choice. */
export const isApplied = (tweak: Pick<TweakStatus, "state">) => tweak.state === "applied";
const canChange = (tweak: Pick<TweakStatus, "state">) => tweak.state !== "unavailable";
/** What a profile would do on this PC: its tweaks not yet in place and its installed apps. */
export function profilePlan<T extends TweakLike, A extends AppLike>(tweaks: readonly T[], apps: readonly A[], profile: Profile) {
  return {
    tweaks: tweaks.filter((tweak) => canChange(tweak) && !isApplied(tweak) && inProfile(tweak.level, profile)),
    apps: apps.filter((app) => app.packages.length > 0 && inProfile(app.level, profile)),
  };
}
/**
 * The choices after picking a profile. It decides everything a profile can
 * reach (the tweaks and apps that have a level), and keeps what was picked
 * by hand outside every profile, and every "turn off". It never turns
 * anything off.
 */
export function withProfile(
  tweaks: readonly TweakLike[],
  apps: readonly AppLike[],
  desired: ReadonlyMap<string, boolean>,
  removing: ReadonlySet<string>,
  profile: Profile,
) {
  const plan = profilePlan(tweaks, apps, profile);
  const wanted = new Set(plan.tweaks.map((tweak) => tweak.id));
  const nextDesired = new Map<string, boolean>();
  for (const [id, on] of desired) {
    const tweak = tweaks.find((known) => known.id === id);
    if (!on || !tweak || tweak.level === null) nextDesired.set(id, on);
  }
  for (const id of wanted) nextDesired.set(id, true);
  const nextApps = new Set([...removing].filter((id) => apps.find((app) => app.id === id)?.level === null));
  for (const app of plan.apps) nextApps.add(app.id);
  return { desired: nextDesired, apps: nextApps };
}
/** The profile the choices are exactly, if any: for the card that shows as picked. */
export function matchingProfile(
  tweaks: readonly TweakLike[],
  apps: readonly AppLike[],
  desired: ReadonlyMap<string, boolean>,
  removing: ReadonlySet<string>,
): Profile | null {
  const { on } = pendingOf(tweaks, desired);
  const leveledOn = new Set(on.filter((tweak) => tweak.level !== null).map((tweak) => tweak.id));
  const leveledApps = new Set([...removing].filter((id) => apps.some((app) => app.id === id && app.level !== null && app.packages.length > 0)));
  if (!leveledOn.size && !leveledApps.size) return null;
  for (const profile of profiles) {
    const plan = profilePlan(tweaks, apps, profile);
    const sameTweaks = plan.tweaks.length === leveledOn.size && plan.tweaks.every((tweak) => leveledOn.has(tweak.id));
    const sameApps = plan.apps.length === leveledApps.size && plan.apps.every((app) => leveledApps.has(app.id));
    if (sameTweaks && sameApps) return profile;
  }
  return null;
}
/** The changes the choices make: tweaks to turn on, and tweaks to turn off. */
export function pendingOf<T extends TweakLike>(tweaks: readonly T[], desired: ReadonlyMap<string, boolean>) {
  const on: T[] = [];
  const off: T[] = [];
  for (const tweak of tweaks) {
    const want = desired.get(tweak.id);
    if (want === undefined || !canChange(tweak)) continue;
    if (want && !isApplied(tweak)) on.push(tweak);
    else if (!want && isApplied(tweak)) off.push(tweak);
  }
  return { on, off };
}
/**
 * The choices that still change something: after a run, or when Windows
 * changed on its own, a choice that is now how things are goes, and so does
 * an app that is no longer installed or a tweak that is no longer known.
 */
export function prune(
  tweaks: readonly TweakLike[],
  apps: readonly AppLike[],
  desired: ReadonlyMap<string, boolean>,
  removing: ReadonlySet<string>,
) {
  const nextDesired = new Map<string, boolean>();
  for (const [id, on] of desired) {
    const tweak = tweaks.find((known) => known.id === id);
    if (tweak && canChange(tweak) && on !== isApplied(tweak)) nextDesired.set(id, on);
  }
  const nextApps = new Set([...removing].filter((id) => apps.some((app) => app.id === id && app.packages.length > 0)));
  return { desired: nextDesired, apps: nextApps };
}
/** Choices saved by an older version: the apps the user ticked by hand are kept; the
 *  tweaks it left out of its one-click run say nothing about what to change. */
export function fromLegacy(savedApps: readonly string[] | null) {
  return { desired: new Map<string, boolean>(), apps: new Set(savedApps ?? []) };
}
/** Apps for the Apps tab's quick choices. */
export function appPresetIds(apps: readonly AppLike[], preset: "recommended" | "all" | "none"): string[] {
  return apps
    .filter((app) => app.packages.length > 0 && (preset === "all" || (preset === "recommended" && inProfile(app.level, "recommended"))))
    .map((app) => app.id);
}
