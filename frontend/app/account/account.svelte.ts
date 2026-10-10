// The signed-in account and the sync of this app's settings through it.
//
// Sync is last-writer-wins over the whole snapshot: the side with the newer
// `updatedAt` (the time of the last change on that PC) is kept. A change here
// is pushed a couple of seconds after it happens; the cloud is read at start,
// after signing in, and on "Sync now".
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "../../lib/confirm.svelte";
import { onStorageChange, readJson } from "../../lib/storage";
import { toast } from "../../lib/toast.svelte";
import { ACCESS_EVENT, accountApi, type Profile, type Provider } from "./api";
import { apply, collect, isSnapshot, isSynced } from "./snapshot";
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
class AccountState {
  profile = $state<Profile | null>(null);
  /** The provider whose browser sign-in is being waited for. */
  signingIn = $state<Provider | null>(null);
  syncing = $state(false);
  lastSyncedAt = $state<number | null>(readJson<number | null>(SYNCED_AT_KEY, null, (v) => typeof v === "number"));
  error = $state<string | null>(null);
  /** The owner-only pages (GitHub Releases, Project Backups) are open. The
   *  backend decides and enforces it; this only shows or hides them. */
  owner = $state(false);
  /** The first answer about `owner` is in (until then a page waits). */
  accessKnown = $state(false);
  #started = false;
  #applying = false;
  #pushTimer: ReturnType<typeof setTimeout> | undefined;
  readonly signedIn = $derived(this.profile !== null);
  /** Once per run, from the app shell: restores the session and syncs. */
  async init() {
    if (!isTauri() || this.#started) return;
    this.#started = true;
    onStorageChange((key) => this.#onLocalChange(key));
    void listen<boolean>(ACCESS_EVENT, (event) => (this.owner = event.payload === true));
    // Checked again now and then (the backend skips it when it checked lately).
    window.addEventListener("focus", () => void this.refreshAccess());
    try {
      this.profile = await accountApi.profile();
    } catch {
      this.profile = null;
    }
    if (this.profile) {
      void this.refreshAccess();
      void this.sync();
    } else {
      this.accessKnown = true;
    }
  }
  async refreshAccess(recheck = false) {
    if (!isTauri() || !this.profile) {
      this.owner = false;
      this.accessKnown = true;
      return;
    }
    try {
      this.owner = await accountApi.access(recheck);
    } catch {
      this.owner = false;
    } finally {
      this.accessKnown = true;
    }
  }
  async signIn(provider: Provider) {
    if (this.signingIn || this.profile) return;
    this.signingIn = provider;
    this.error = null;
    try {
      this.profile = await accountApi.signIn(provider);
      void this.refreshAccess();
      toast.success(`Signed in with ${PROVIDER_NAMES[provider]} as ${this.profile.name ?? this.profile.email ?? "you"}.`);
      await this.sync();
    } catch (error) {
      const text = message(error);
      if (!/cancelled/i.test(text)) {
        this.error = text;
        toast.error(`${PROVIDER_NAMES[provider]} sign-in failed: ${text}`);
      }
    } finally {
      this.signingIn = null;
    }
  }
  cancelSignIn() {
    void accountApi.cancelSignIn();
  }
  async signOut() {
    if (!this.profile) return;
    const ok = await confirm({
      title: "Sign out?",
      message:
        "Your settings stay on this PC and in your account. Changes made while signed out are not synced until you sign in again.",
      confirmLabel: "Sign out",
    });
    if (!ok) return;
    clearTimeout(this.#pushTimer);
    try {
      await accountApi.signOut();
    } catch (error) {
      toast.error(`Could not sign out: ${message(error)}`);
      return;
    }
    this.profile = null;
    this.owner = false;
    this.error = null;
    toast.info("Signed out.");
  }
  /** Reads the cloud copy and keeps whichever side changed last. */
  async sync(announce = false) {
    if (!this.profile || this.syncing) return;
    clearTimeout(this.#pushTimer);
    this.syncing = true;
    this.error = null;
    try {
      const cloud = await accountApi.pull();
      const local = localUpdatedAt();
      if (isSnapshot(cloud) && (!local || cloud.updatedAt > local)) {
        this.#applying = true;
        try {
          const note = await apply(cloud);
          writeRaw(UPDATED_AT_KEY, cloud.updatedAt);
          if (note) toast.info(note);
          else if (announce || local) toast.success("Settings updated from your account.");
        } finally {
          this.#applying = false;
        }
      } else if (!isSnapshot(cloud) || (local && local > cloud.updatedAt)) {
        await this.#push();
      }
      this.#markSynced();
      if (announce) toast.success("Everything is in sync.");
    } catch (error) {
      this.#fail(error);
    } finally {
      this.syncing = false;
    }
  }
  #onLocalChange(key: string) {
    if (this.#applying || !isSynced(key)) return;
    writeRaw(UPDATED_AT_KEY, new Date().toISOString());
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
    if (this.syncing) {
      this.#schedulePush();
      return;
    }
    this.syncing = true;
    try {
      await this.#push();
      this.#markSynced();
      this.error = null;
    } catch (error) {
      this.#fail(error);
    } finally {
      this.syncing = false;
    }
  }
  async #push() {
    const updatedAt = localUpdatedAt() ?? new Date().toISOString();
    writeRaw(UPDATED_AT_KEY, updatedAt);
    await accountApi.push(await collect(updatedAt));
  }
  #markSynced() {
    this.lastSyncedAt = Date.now();
    writeRaw(SYNCED_AT_KEY, JSON.stringify(this.lastSyncedAt));
  }
  #fail(error: unknown) {
    const text = message(error);
    this.error = text;
    // The backend drops a session it can no longer refresh.
    if (/sign in again|not signed in/i.test(text)) this.profile = null;
  }
}
export const account = new AccountState();
