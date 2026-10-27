// What account sync carries between PCs, and how it is gathered and applied.
//
// Preferences travel as the raw localStorage values of SYNCED_KEYS; each page
// re-reads them with its own validation. Game Saves settings live in the
// backend and travel through its export/import commands. Owner-only GitHub
// project paths use their own export/import and change timestamp.
import { invoke } from "@tauri-apps/api/core";
import { nav } from "../../lib/nav.svelte";
import { settings } from "../../lib/settings.svelte";
import { gameSavesState } from "../pages/game-saves/state.svelte";
import type { GameSavesSettings } from "../pages/game-saves/api";
import { diskState } from "../pages/disk-analyzer/state.svelte";
import { appsState } from "../pages/install-apps/state.svelte";
import { cleanerState } from "../pages/system-cleaner/state.svelte";
import { maintenanceState } from "../pages/system-maintenance/state.svelte";
import { githubReleases as githubReleasesState } from "../pages/github-releases/state.svelte";
import type { PageState, SyncedProjects } from "../pages/github-releases/api";

export const SYNCED_KEYS = [
  "myle.dark",
  "myle.sidebar.collapsed",
  "myle.apps.view",
  "myle.apps.filter",
  "myle.apps.sort",
  "myle.apps.selected",
  "myle.apps.pinned",
  "myle.gameSaves.tab",
  "myle.gameSaves.filter",
  "myle.maintenance.view",
  "cleaner.selected",
  // View choices only: scan results and paths never leave this PC.
  "myle.diskAnalyzer.sizeMode",
  "myle.diskAnalyzer.view",
] as const;

/** Announced by the Game Saves page (see `notifyChange`), not a storage key. */
export const GAME_SAVES_CHANGE = "gameSaves";
export const GITHUB_PROJECTS_CHANGE = "githubReleases.projects";
export const PROJECTS_UPDATED_AT_KEY = "myle.githubReleases.projectsUpdatedAt";

export function isSynced(key: string): boolean {
  return key === GAME_SAVES_CHANGE || key === GITHUB_PROJECTS_CHANGE || (SYNCED_KEYS as readonly string[]).includes(key);
}

export interface SyncedGameSaves {
  backupFolder?: string | null;
  schedule: string;
  scheduleTime: string;
  scheduleWeekday: string;
  autoBackupExcludedGameIds: string[];
  customGames: unknown[];
}

export interface Snapshot {
  version: 1;
  /** ISO time of the last change on the PC that saved it. */
  updatedAt: string;
  storage: Record<string, string>;
  gameSaves: SyncedGameSaves | null;
  /** Missing in older clients; an explicit empty list means no projects. */
  githubReleases?: SyncedProjects;
  githubReleasesUpdatedAt?: string;
}

export function isSnapshot(value: unknown): value is Snapshot {
  if (!value || typeof value !== "object") return false;
  const v = value as Partial<Snapshot>;
  return v.version === 1 && typeof v.updatedAt === "string" && !!v.storage && typeof v.storage === "object";
}

export async function collect(updatedAt: string, includeGithubProjects = false): Promise<Snapshot> {
  const storage: Record<string, string> = {};
  for (const key of SYNCED_KEYS) {
    try {
      const value = localStorage.getItem(key);
      if (value !== null) storage[key] = value;
    } catch {
      // Unreadable storage: that preference is simply not sent.
    }
  }
  const gameSaves = await invoke<SyncedGameSaves>("game_saves_sync_export").catch(() => null);
  const githubReleases = includeGithubProjects ? await invoke<SyncedProjects>("github_releases_sync_export") : undefined;
  let githubReleasesUpdatedAt: string | null = null;
  try { githubReleasesUpdatedAt = localStorage.getItem(PROJECTS_UPDATED_AT_KEY); } catch { /* restored on the next successful sync */ }
  return { version: 1, updatedAt, storage, gameSaves, ...(githubReleases ? { githubReleases, ...(githubReleasesUpdatedAt ? { githubReleasesUpdatedAt } : {}) } : {}) };
}

export async function applyGithubProjects(projects: SyncedProjects): Promise<void> {
  const page = await invoke<PageState>("github_releases_sync_import", { synced: projects });
  await githubReleasesState.applySyncedProjects(page);
}

/**
 * Writes a snapshot from another PC into this one and refreshes the pages.
 * Returns a note when part of it could not be applied yet.
 */
export async function apply(snapshot: Snapshot, includeGithubProjects = false): Promise<string | null> {
  for (const key of SYNCED_KEYS) {
    const value = snapshot.storage[key];
    if (typeof value !== "string") continue;
    try {
      localStorage.setItem(key, value);
    } catch {
      // Not persisted here; the live state below still follows it.
    }
  }
  settings.reload();
  nav.reload();
  appsState.reloadChoices();
  maintenanceState.reloadView();
  cleanerState.reloadSelection();
  diskState.reloadChoices();
  gameSavesState.reloadChoices();

  if (includeGithubProjects && snapshot.githubReleases != null) {
    // A failed/deferred import must keep the account's timestamp retryable.
    await applyGithubProjects(snapshot.githubReleases);
  }

  if (!snapshot.gameSaves) return null;
  try {
    const updated = await invoke<GameSavesSettings>("game_saves_sync_import", { synced: snapshot.gameSaves });
    await gameSavesState.applySyncedSettings(updated);
    return null;
  } catch (error) {
    return `Game Saves settings were not applied: ${error instanceof Error ? error.message : String(error)}`;
  }
}
