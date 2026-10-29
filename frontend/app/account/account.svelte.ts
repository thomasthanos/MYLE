// The signed-in account and the sync of this app's settings through it. Preferences use the newer snapshot's `updatedAt`. GitHub project paths have their own timestamp, so unrelated preferences cannot replace them. A change here is pushed a couple of seconds after it happens; the cloud is read at start, after signing in, and on "Sync now".
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "../../lib/confirm.svelte";
import { onStorageChange, readJson } from "../../lib/storage";
import { toast } from "../../lib/toast.svelte";
import { ACCESS_EVENT, accountApi, type Profile, type Provider } from "./api";
import { apply, applyGithubProjects, collect, GITHUB_PROJECTS_CHANGE, isSnapshot, isSynced, PROJECTS_UPDATED_AT_KEY } from "./snapshot";
const UPDATED_AT_KEY = "myle.sync.updatedAt";
const SYNCED_AT_KEY = "myle.sync.lastSyncedAt";
const PUSH_DELAY_MS = 2000;
const PROVIDER_NAMES: Record<Provider, string> = { discord: "Discord", google: "Google" };
function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
/** Written with localStorage directly: these must not count as a change. */
function writeRaw(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Sync still works for this run.
  }
}
function localUpdatedAt(): string | null {
  try {
    return localStorage.getItem(UPDATED_AT_KEY);
  } catch {
    return null;
  }
}
export class AccountState {
  profile = $state<Profile | null>(null);
  /** The provider whose browser sign-in is being waited for. */
  signingIn = $state<Provider | null>(null);
  syncing = $state(false);
  lastSyncedAt = $state<number | null>(readJson<number | null>(SYNCED_AT_KEY, null, (v) => typeof v === "number"));
  error = $state<string | null>(null);
  /* The owner-only pages (GitHub Releases, Project Backups) are open. The backend decides and enforces it; this only shows or hides them. */
  owner = $state(false);
  /** The first answer about `owner` is in (until then a page waits). */
  accessKnown = $state(false);
  #started = false;
  #authEpoch = 0;
  #accessEpoch = 0;
  #signingOut = false;
  #applying = false;
  #pushTimer: ReturnType<typeof setTimeout> | undefined;
  readonly signedIn = $derived(this.profile !== null);
  /** Once per run, from the app shell: restores the session and syncs. */
  async init() {
    if (!isTauri() || this.#started) return;
    this.#started = true;
    const epoch = this.#authEpoch;
    onStorageChange((key) => this.#onLocalChange(key));
    void listen<boolean>(ACCESS_EVENT, (event) => {
      this.#accessEpoch++;
      this.owner = event.payload === true && this.profile !== null;
      this.accessKnown = true;
    });
    // Checked again now and then (the backend skips it when it checked lately).
    window.addEventListener("focus", () => void this.refreshAccess());
    try {
      const profile = await accountApi.profile();
      if (epoch !== this.#authEpoch) return;
      this.profile = profile;
    } catch {
      if (epoch !== this.#authEpoch) return;
      this.profile = null;
    }
    if (this.profile) {
      await this.refreshAccess();
      void this.sync();
    } else {
      this.accessKnown = true;
    }
  }
  async refreshAccess(recheck = false) {
    const request = ++this.#accessEpoch;
    if (!isTauri() || !this.profile) {
      this.owner = false;
      this.accessKnown = true;
      return;
    }
    const epoch = this.#authEpoch;
    const userId = this.profile.id;
    const current = () => this.#isCurrent(epoch, userId) && request === this.#accessEpoch;
    try {
      const owner = await accountApi.access(recheck);
      if (current()) this.owner = owner;
    } catch {
      if (current()) this.owner = false;
    } finally {
      if (current()) this.accessKnown = true;
    }
  }
  async signIn(provider: Provider) {
    if (this.signingIn || this.profile || this.#signingOut) return;
    const epoch = ++this.#authEpoch;
    this.signingIn = provider;
    this.error = null;
    try {
      const profile = await accountApi.signIn(provider);
      if (epoch !== this.#authEpoch) return;
      this.profile = profile;
      await this.refreshAccess();
      if (!this.#isCurrent(epoch, profile.id)) return;
      toast.success(`Signed in with ${PROVIDER_NAMES[provider]} as ${this.profile.name ?? this.profile.email ?? "you"}.`);
      await this.sync();
    } catch (error) {
      if (epoch !== this.#authEpoch) return;
      const text = message(error);
      if (!/cancelled/i.test(text)) {
        this.error = text;
        toast.error(`${PROVIDER_NAMES[provider]} sign-in failed: ${text}`);
      }
    } finally {
      if (epoch === this.#authEpoch) this.signingIn = null;
    }
  }
  cancelSignIn() {
    void accountApi.cancelSignIn();
  }
  async signOut() {
    if (!this.profile || this.#signingOut) return;
    this.#signingOut = true;
    try {
      const ok = await confirm({
        title: "Sign out?",
        message:
          "Your settings stay on this PC and in your account. Changes made while signed out are not synced until you sign in again.",
        confirmLabel: "Sign out",
      });
      if (!ok) return;
      // Invalidate callbacks even if this same account signs in again later.
      this.#authEpoch++;
      this.syncing = false;
      this.#applying = false;
      clearTimeout(this.#pushTimer);
      await accountApi.signOut();
      this.profile = null;
      this.signingIn = null;
      this.owner = false;
      this.accessKnown = true;
      this.error = null;
      toast.info("Signed out.");
    } catch (error) {
      toast.error(`Could not sign out: ${message(error)}`);
    } finally {
      this.#signingOut = false;
    }
  }
  /** Reads the cloud copy and keeps whichever side changed last. */
  async sync(announce = false) {
    if (!this.profile || this.syncing || this.#signingOut) return;
    const epoch = this.#authEpoch;
    const userId = this.profile.id;
    clearTimeout(this.#pushTimer);
    this.syncing = true;
    this.error = null;
    try {
      const cloud = await accountApi.pull();
      if (!this.#isCurrent(epoch, userId)) return;
      const local = localUpdatedAt();
      let pushed = false;
      let pushProjects = false;
      if (this.owner && isSnapshot(cloud) && cloud.githubReleases != null) {
        let projectsChanged: string | null = null;
        try { projectsChanged = localStorage.getItem(PROJECTS_UPDATED_AT_KEY); } catch { /* a fresh PC */ }
        const remoteChanged = cloud.githubReleasesUpdatedAt ?? cloud.updatedAt;
        if (!projectsChanged || remoteChanged > projectsChanged) {
          await applyGithubProjects(cloud.githubReleases);
          if (!this.#isCurrent(epoch, userId)) return;
          writeRaw(PROJECTS_UPDATED_AT_KEY, remoteChanged);
        } else if (projectsChanged > remoteChanged) {
          pushProjects = true;
        }
      }
      if (isSnapshot(cloud) && (!local || cloud.updatedAt > local)) {
        this.#applying = true;
        try {
          const note = await apply(cloud, false);
          if (!this.#isCurrent(epoch, userId)) return;
          writeRaw(UPDATED_AT_KEY, cloud.updatedAt);
          if (note) toast.info(note);
          else if (announce || local) toast.success("Settings updated from your account.");
        } finally {
          if (epoch === this.#authEpoch) this.#applying = false;
        }
      } else if (!isSnapshot(cloud) || (local && local > cloud.updatedAt)) {
        await this.#push();
        if (!this.#isCurrent(epoch, userId)) return;
        pushed = true;
      }
      // Upgrade an existing account once, even when its timestamp equals this PC's. Older snapshots never meant "delete all local project paths".
      if (!pushed && this.owner && isSnapshot(cloud) && (cloud.githubReleases == null || pushProjects)) {
        const snapshot = await collect(localUpdatedAt() ?? cloud.updatedAt, true);
        if (!this.#isCurrent(epoch, userId)) return;
        if (snapshot.githubReleases && (snapshot.githubReleases.paths.length || snapshot.githubReleasesUpdatedAt || pushProjects)) {
          snapshot.updatedAt = new Date().toISOString();
          snapshot.githubReleasesUpdatedAt ??= snapshot.updatedAt;
          if (!this.#isCurrent(epoch, userId)) return;
          await accountApi.push(snapshot);
          if (!this.#isCurrent(epoch, userId)) return;
          writeRaw(UPDATED_AT_KEY, snapshot.updatedAt);
          writeRaw(PROJECTS_UPDATED_AT_KEY, snapshot.githubReleasesUpdatedAt);
        }
      }
      this.#markSynced();
      if (announce) toast.success("Everything is in sync.");
    } catch (error) {
      if (this.#isCurrent(epoch, userId)) this.#fail(error);
    } finally {
      if (epoch === this.#authEpoch) this.syncing = false;
    }
  }
  #onLocalChange(key: string) {
    if ((this.#applying && key !== GITHUB_PROJECTS_CHANGE) || !isSynced(key)) return;
    writeRaw(UPDATED_AT_KEY, new Date().toISOString());
    if (key === GITHUB_PROJECTS_CHANGE) writeRaw(PROJECTS_UPDATED_AT_KEY, new Date().toISOString());
    if (!this.profile) return;
    this.#schedulePush();
  }
  #schedulePush() {
    clearTimeout(this.#pushTimer);
    this.#pushTimer = setTimeout(() => void this.#pushNow(), PUSH_DELAY_MS);
  }
  async #pushNow() {
    if (!this.profile) return;
    // A sync is running: try again after it rather than drop this change.
    if (this.syncing || this.#signingOut) {
      this.#schedulePush();
      return;
    }
    const epoch = this.#authEpoch;
    const userId = this.profile.id;
    this.syncing = true;
    try {
      await this.#push();
      if (!this.#isCurrent(epoch, userId)) return;
      this.#markSynced();
      this.error = null;
    } catch (error) {
      if (this.#isCurrent(epoch, userId)) this.#fail(error);
    } finally {
      if (epoch === this.#authEpoch) this.syncing = false;
    }
  }
  async #push() {
    const epoch = this.#authEpoch;
    const userId = this.profile?.id;
    if (!userId) return;
    const updatedAt = localUpdatedAt() ?? new Date().toISOString();
    writeRaw(UPDATED_AT_KEY, updatedAt);
    const snapshot = await collect(updatedAt, this.owner);
    if (!this.#isCurrent(epoch, userId)) throw new Error("The account changed. Sync again with the current account.");
    if (snapshot.githubReleases) snapshot.githubReleasesUpdatedAt ??= updatedAt;
    await accountApi.push(snapshot);
    if (!this.#isCurrent(epoch, userId)) return;
    if (snapshot.githubReleasesUpdatedAt) writeRaw(PROJECTS_UPDATED_AT_KEY, snapshot.githubReleasesUpdatedAt);
  }
  #markSynced() {
    this.lastSyncedAt = Date.now();
    writeRaw(SYNCED_AT_KEY, JSON.stringify(this.lastSyncedAt));
  }
  #isCurrent(epoch: number, userId: string) {
    return epoch === this.#authEpoch && this.profile?.id === userId;
  }
  #fail(error: unknown) {
    const text = message(error);
    this.error = text;
    // The backend drops a session it can no longer refresh.
    if (/sign in again|not signed in/i.test(text)) {
      this.#authEpoch++;
      this.profile = null;
      this.owner = false;
      this.accessKnown = true;
      this.signingIn = null;
      this.syncing = false;
      this.#applying = false;
      clearTimeout(this.#pushTimer);
    }
  }
}
export const account = new AccountState();
