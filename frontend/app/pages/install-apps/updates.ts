// The update cards MYLE makes for winget packages its own catalog does not name:
// GitHub CLI, the .NET runtimes, and anything else winget reports an update for.
export interface WingetUpgrade {
  id: string;
  name: string;
  /** Installed version; "Unknown" when winget cannot read it. */
  version: string;
  /** Newer version offered by winget. */
  available: string;
  /** Homepage, filled in when the card's website is looked up. */
  site?: string;
  iconDomain?: string;
}
export interface UpdateCard {
  id: string;
  name: string;
}
const key = (id: string) => id.toLowerCase();
/**
 * One card per package id, only for updates the page does not already list
 * (curated apps, custom apps, pinned cards), matching the search box the same
 * way the other cards do.
 */
export function autoUpdateCards(
  updates: WingetUpgrade[],
  knownIds: Iterable<string>,
  query = "",
): UpdateCard[] {
  const known = new Set([...knownIds].map(key));
  const needle = query.trim().toLowerCase();
  const seen = new Set<string>();
  const cards: UpdateCard[] = [];
  for (const update of updates) {
    const id = key(update.id);
    if (!id || seen.has(id) || known.has(id)) continue;
    seen.add(id);
    if (needle && !update.name.toLowerCase().includes(needle) && !id.includes(needle)) continue;
    cards.push({ id: update.id, name: update.name });
  }
  return cards;
}
