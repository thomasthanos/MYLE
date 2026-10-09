// Every page of the app. Add an entry here and it shows up in the sidebar.
import type { Component } from "svelte";
import IconInstallApps from "../../lib/icons/IconInstallApps.svelte";
import IconSpotifyHub from "../../lib/icons/IconSpotifyHub.svelte";
import IconGameSaves from "../../lib/icons/IconGameSaves.svelte";
import IconProjectBackups from "../../lib/icons/IconProjectBackups.svelte";
import IconGithubReleases from "../../lib/icons/IconGithubReleases.svelte";
import IconCreativeHub from "../../lib/icons/IconCreativeHub.svelte";
import IconWindowsOpt from "../../lib/icons/IconWindowsOpt.svelte";
import IconSystemCleaner from "../../lib/icons/IconSystemCleaner.svelte";
import IconSystemMaint from "../../lib/icons/IconSystemMaint.svelte";
import IconPasswords from "../../lib/icons/IconPasswords.svelte";
import IconSettings from "../../lib/icons/IconSettings.svelte";
import type { NavGroup } from "../../lib/sidebar";
import CreativeHub from "./creative-hub/CreativeHub.svelte";
import GameSaves from "./game-saves/GameSaves.svelte";
import InstallApps from "./install-apps/InstallApps.svelte";
import PasswordManager from "./password-manager/PasswordManager.svelte";
import SpotifyHub from "./spotify-hub/SpotifyHub.svelte";
import Settings from "./settings/Settings.svelte";
import SystemCleaner from "./system-cleaner/SystemCleaner.svelte";
import SystemMaintenance from "./system-maintenance/SystemMaintenance.svelte";
import WindowsOptimization from "./windows-optimization/WindowsOptimization.svelte";

/** A page's component, loaded with the app or the first time it opens. */
export type PageModule = () => Promise<{ default: Component }>;

export interface PageDef {
  id: string;
  label: string;
  /** One line for the sidebar's tooltip. */
  description: string;
  icon: Component;
  /** The sidebar group it is listed in (see lib/sidebar.ts). */
  group: NavGroup;
  /** In the main bundle: opens at once. */
  component?: Component;
  /** Loaded the first time it opens instead (the owner-only pages, which
   *  nobody else ever opens, so nobody else downloads or parses them). */
  load?: PageModule;
  /** Fills the window's height and scrolls inside itself (a list beside
   *  its details), instead of the whole page scrolling. */
  fill?: boolean;
  /** Only for the app's owner (see backend/src/account/owner.rs): hidden
   *  from everyone else, and the backend refuses its commands. */
  ownerOnly?: boolean;
}

// The sidebar shows them in this order, group by group: what you install,
// what you keep, the PC itself, then the owner's tools. Install Apps comes
// first: it is the page the app opens on.
const defs = [
  { id: "install-apps",         label: "Install Apps",          group: "apps",   icon: IconInstallApps,    component: InstallApps,
    description: "Install, update and keep track of your apps." },
  { id: "creative-hub",         label: "Creative Suite",        group: "apps",   icon: IconCreativeHub,    component: CreativeHub,
    description: "Adobe, Clip Studio Paint and Office, set up in one click." },
  { id: "spotify-hub",          label: "Spotify Hub",           group: "apps",   icon: IconSpotifyHub,     component: SpotifyHub,
    description: "Customize Spotify with Spicetify, or return it to stock." },
  { id: "password-manager",     label: "Password Manager",      group: "data",   icon: IconPasswords,      component: PasswordManager, fill: true,
    description: "Your logins, encrypted on this PC before they are saved or synced." },
  { id: "game-saves",           label: "Game Saves",            group: "data",   icon: IconGameSaves,      component: GameSaves,
    description: "Back up your game progress and restore it when you need." },
  { id: "windows-optimization", label: "Windows Optimization",  group: "system", icon: IconWindowsOpt,     component: WindowsOptimization,
    description: "Remove bloat, tune settings and tidy the Start menu." },
  { id: "system-cleaner",       label: "System Cleaner",        group: "system", icon: IconSystemCleaner,  component: SystemCleaner,
    description: "Find and remove the files Windows leaves behind." },
  { id: "system-maintenance",   label: "System Maintenance",    group: "system", icon: IconSystemMaint,    component: SystemMaintenance,
    description: "Repair connections, check system files, keep everything up to date." },
  { id: "project-backups",      label: "Project Backups",       group: "owner",  icon: IconProjectBackups, ownerOnly: true,
    load: () => import("./project-backups/ProjectBackups.svelte"),
    description: "Zip your projects into Google Drive or Dropbox, checked before they are kept." },
  { id: "github-releases",      label: "GitHub Releases",       group: "owner",  icon: IconGithubReleases, ownerOnly: true, fill: true,
    load: () => import("./github-releases/GithubReleases.svelte"),
    description: "Commit, build and release your projects to GitHub." },
  { id: "settings",             label: "Settings",              group: "bottom", icon: IconSettings,       component: Settings,
    description: "Your account, sync, appearance and updates." },
] as const satisfies readonly PageDef[];

export type PageId = (typeof defs)[number]["id"];

export const pages: readonly (PageDef & { id: PageId })[] = defs;

/** Whether the page can be shown, given whether the owner is signed in. */
export function canOpen(page: PageDef, owner: boolean): boolean {
  return !page.ownerOnly || owner;
}
