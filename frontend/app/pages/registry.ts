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
import CreativeHub from "./creative-hub/CreativeHub.svelte";
import GameSaves from "./game-saves/GameSaves.svelte";
import GithubReleases from "./github-releases/GithubReleases.svelte";
import InstallApps from "./install-apps/InstallApps.svelte";
import PasswordManager from "./password-manager/PasswordManager.svelte";
import ProjectBackups from "./project-backups/ProjectBackups.svelte";
import SpotifyHub from "./spotify-hub/SpotifyHub.svelte";
import Settings from "./settings/Settings.svelte";
import SystemCleaner from "./system-cleaner/SystemCleaner.svelte";
import SystemMaintenance from "./system-maintenance/SystemMaintenance.svelte";
import WindowsOptimization from "./windows-optimization/WindowsOptimization.svelte";

export interface PageDef {
  id: string;
  label: string;
  icon: Component;
  component: Component;
  /** Pin to the bottom group of the sidebar. */
  bottom?: boolean;
  /** Fills the window's height and scrolls inside itself (a list beside
   *  its details), instead of the whole page scrolling. */
  fill?: boolean;
  /** Only for the app's owner (see backend/src/account/owner.rs): hidden
   *  from everyone else, and the backend refuses its commands. */
  ownerOnly?: boolean;
}

const defs = [
  { id: "install-apps",         label: "Install Apps",          icon: IconInstallApps,   component: InstallApps },
  { id: "spotify-hub",          label: "Spotify Hub",           icon: IconSpotifyHub,    component: SpotifyHub },
  { id: "game-saves",           label: "Game Saves",            icon: IconGameSaves,     component: GameSaves },
  { id: "project-backups",      label: "Project Backups",       icon: IconProjectBackups, component: ProjectBackups, ownerOnly: true },
  { id: "github-releases",      label: "GitHub Releases",       icon: IconGithubReleases, component: GithubReleases, fill: true, ownerOnly: true },
  { id: "creative-hub",         label: "Creative Suite",        icon: IconCreativeHub,   component: CreativeHub },
  { id: "windows-optimization", label: "Windows Optimization",  icon: IconWindowsOpt,    component: WindowsOptimization },
  { id: "system-cleaner",       label: "System Cleaner",        icon: IconSystemCleaner, component: SystemCleaner },
  { id: "system-maintenance",   label: "System Maintenance",    icon: IconSystemMaint,   component: SystemMaintenance },
  { id: "password-manager",     label: "Password Manager",      icon: IconPasswords,     component: PasswordManager, fill: true },
  { id: "settings",             label: "Settings",              icon: IconSettings,      component: Settings, bottom: true },
] as const satisfies readonly PageDef[];

export type PageId = (typeof defs)[number]["id"];

export const pages: readonly (PageDef & { id: PageId })[] = defs;

/** Whether the page can be shown, given whether the owner is signed in. */
export function canOpen(page: PageDef, owner: boolean): boolean {
  return !page.ownerOnly || owner;
}
