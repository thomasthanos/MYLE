// The Password Manager page's state. It lives outside the page component so switching pages keeps the open entry and the search.
import { confirm } from "../../../lib/confirm.svelte";
import { ANDROID, DEVICE, MOBILE, QUICK_UNLOCK } from "../../../lib/platform";
import { readFlag, readJson, writeFlag, writeJson } from "../../../lib/storage";
import { toast } from "../../../lib/toast.svelte";
import { passwordsApi as api, type EntryInput, type Summary, type SyncResult, type VaultStatus, type AppLink } from "./api";
export type Filter = "all" | "favorites" | "weak" | "reused" | "totp";
/** The list's order: by name, or the most recently changed first. */
export type Sort = "name" | "recent";
const SORT_KEY = "myle.passwords.sort";
/** How long a Copy button shows that it worked. */
const COPIED_FOR = 1600;
/** Missing icons are tried again while the vault stays open. */
const ICON_RETRY_AFTER = 5 * 60_000;
/** The words of a search, lowercased: every one has to match somewhere. */
export function searchWords(query: string): string[] {
  return query.trim().toLowerCase().split(/\s+/).filter(Boolean);
}
/** Whether `entry` has every word of the search in one of its fields. */
export function matches(entry: Summary, words: string[]): boolean {
  if (!words.length) return true;
  const haystack = [
    entry.title,
    entry.username,
    entry.folder,
    entry.notes,
    ...entry.urls,
    ...entry.apps.flatMap((a) => [a.name, a.exe]),
    ...entry.passkeys.flatMap((k) => [k.rpId, k.userName]),
  ]
    .join("\n")
    .toLowerCase();
  return words.every((word) => haystack.includes(word));
}
/** Phones: Face ID / fingerprint was offered after an unlock. */
const QUICK_UNLOCK_OFFERED_KEY = "myle.passwords.quickUnlockOffered";
/** How the vault stands with the account, for the sync indicator. */
export type SyncView =
  | { kind: "idle" }
  | { kind: "syncing" }
  | { kind: "synced"; at: number; pending: number }
  | { kind: "signedOut" }
  | { kind: "otherVault" }
  | { kind: "error"; message: string };
/** Where the right-hand panel is. */
export type Panel =
  | { kind: "none" }
  | { kind: "view"; id: string }
  /** `program`: a new login, linked to the Windows program it is for. */
  | { kind: "edit"; id: string | null; program?: AppLink };
/* The host an entry's address is known by, written the way the app writes it (lowercase, no `www.`): the key of its website icon. */
export function iconHost(url: string): string | null {
  try {
    const parsed = new URL(url.includes("://") ? url : `https://${url}`);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    return parsed.hostname.replace(/\.$/, "").replace(/^(?:www\.)+/, "") || null;
  } catch {
    return null;
  }
}
export interface WindowsTarget {
  exe: string;
  path: string;
  /** It runs as administrator: MYLE cannot type into it. */
  elevated: boolean;
}
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
class PasswordsState {
  status = $state<VaultStatus | null>(null);
  autoLockMinutes = $state(5);
  entries = $state<Summary[]>([]);
  query = $state("");
  filter = $state<Filter>("all");
  sort = $state<Sort>(readJson<Sort>(SORT_KEY, "name", (v) => v === "name" || v === "recent"));
  /** The Copy button that just worked ("id:field"), for its tick. */
  copied = $state<string | null>(null);
  #copiedTimer: ReturnType<typeof setTimeout> | undefined;
  /** When each shown password hides itself again, by entry id. */
  #revealTimers = new Map<string, ReturnType<typeof setTimeout>>();
  panel = $state<Panel>({ kind: "none" });
  windowsTarget = $state<WindowsTarget | null>(null);
  /** Passwords the user chose to show, by entry id. Forgotten on lock. */
  revealed = $state<Record<string, string>>({});
  busy = $state(false);
  error = $state<string | null>(null);
  sync = $state<SyncView>({ kind: "idle" });
  /** A new vault waits for the account check: it may already have one. */
  checkedAccount = $state(false);
  /** Entries that did not open with the vault key (damaged or changed). */
  damaged = $state(0);
  /** Windows Hello on this PC: set up at all, and opening this vault. */
  hello = $state({ available: false, enabled: false });
  /** Website icons (`data:` URLs) by host, while the vault is open. */
  icons = $state<Record<string, string>>({});
  websiteIcons = $state(true);
  /* Until when opening the page asks Windows Hello by itself: only just after the user came to the page, never when the vault locks while they are on it (by hand, or after a while away). */
  #helloOnOpenUntil = 0;
  /* A website's sign-in waits for the vault (the browser extension asked): the lock screen says so and asks Windows Hello by itself, once. */
  siteWaiting = $state(false);
  helloForSite = $state(false);
  /** Hosts already asked about since the vault opened. */
  #askedIcons = new Map<string, number>();
  #iconRetryTimer: ReturnType<typeof setTimeout> | undefined;
  #iconEpoch = 0;
  #unlistenIcon: (() => void) | null = null;
  #unlisten: (() => void) | null = null;
  #unlistenSync: (() => void) | null = null;
  #unlistenChanged: (() => void) | null = null;
  #syncTimer: ReturnType<typeof setTimeout> | undefined;
  #dataEpoch = 0;
  /** Decrypted replies belong to the unlock that requested them. */
  #vaultEpoch = 0;
  #listSequence = 0;
  #loading: { epoch: number; promise: Promise<void> } | null = null;
  readonly visible = $derived.by(() => {
    const words = searchWords(this.query);
    const byName = (a: Summary, b: Summary) =>
      a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true }) ||
      a.username.localeCompare(b.username, undefined, { sensitivity: "base" });
    return this.entries
      .filter((e) => {
        if (this.filter === "favorites" && !e.favorite) return false;
        if (this.filter === "weak" && e.strength !== "weak") return false;
        if (this.filter === "reused" && !e.reused) return false;
        if (this.filter === "totp" && !e.hasTotp) return false;
        return matches(e, words);
      })
      .sort((a, b) =>
        this.sort === "recent"
          ? b.updatedAt - a.updatedAt || byName(a, b)
          : Number(b.favorite) - Number(a.favorite) || byName(a, b),
      );
  });
  readonly counts = $derived({
    all: this.entries.length,
    favorites: this.entries.filter((e) => e.favorite).length,
    weak: this.entries.filter((e) => e.strength === "weak").length,
    reused: this.entries.filter((e) => e.reused).length,
    totp: this.entries.filter((e) => e.hasTotp).length,
  });
  setSort(sort: Sort) {
    this.sort = sort;
    writeJson(SORT_KEY, sort);
  }
  /** Back to every login: no search, no filter. */
  clearSearch() {
    this.query = "";
    this.filter = "all";
  }
  readonly folders = $derived([...new Set(this.entries.map((e) => e.folder).filter(Boolean))].sort());
  entry(id: string) {
    return this.entries.find((e) => e.id === id) ?? null;
  }
  async load(): Promise<void> {
    const epoch = this.#dataEpoch;
    if (this.#loading) {
      const loading = this.#loading;
      await loading.promise;
      // A new account cannot reuse the status check that was invalidated by local reset.
      if (epoch === this.#dataEpoch && loading.epoch !== epoch) await this.load();
      return;
    }
    const promise = this.#load();
    this.#loading = { epoch, promise };
    try {
      await promise;
    } finally {
      if (this.#loading?.promise === promise) this.#loading = null;
    }
  }
  async #load() {
    const epoch = this.#dataEpoch;
    this.#unlisten ??= await api.onLocked(() => this.#onLocked(true)).catch(() => null);
    this.#unlistenSync ??= await api.onSynced((result) => {
      if (this.checkedAccount || this.status !== "new") void this.#afterSync(result);
    }).catch(() => null);
    this.#unlistenIcon ??= await api
      .onIcon(({ host, icon }) => {
        if (this.status === "unlocked" && this.websiteIcons) this.icons[host] = icon;
      })
      .catch(() => null);
    // Saved from the browser: show it, and send it up.
    this.#unlistenChanged ??= await api
      .onChanged(() => {
        if (this.status === "unlocked") void this.refresh();
        this.#syncSoon();
      })
      .catch(() => null);
    if (epoch !== this.#dataEpoch) return;
    await this.#readStatus();
    if (epoch !== this.#dataEpoch) return;
    await this.syncNow();
    if (epoch === this.#dataEpoch) this.checkedAccount = true;
  }
  async #readStatus() {
    const epoch = this.#dataEpoch;
    await this.#readHello();
    if (epoch !== this.#dataEpoch) return;
    try {
      const info = await api.status();
      if (epoch !== this.#dataEpoch) return;
      this.autoLockMinutes = info.autoLockMinutes;
      this.damaged = info.damaged;
      this.websiteIcons = info.websiteIcons;
      if (info.status !== "unlocked" && this.status === "unlocked") this.#onLocked(false);
      this.status = info.status;
      if (info.status === "unlocked") await this.refresh();
    } catch (error) {
      if (epoch === this.#dataEpoch) this.error = message(error);
    }
  }
  async #readHello() {
    const epoch = this.#dataEpoch;
    try {
      const hello = await api.helloStatus();
      if (epoch === this.#dataEpoch) this.hello = hello;
    } catch {
      if (epoch === this.#dataEpoch) this.hello = { available: false, enabled: false };
    }
  }
  /** The browser extension wants the vault for a sign-in. */
  async wantUnlock() {
    // Asked as the app starts: the vault's state may not be read yet.
    const info = await api.status().catch(() => null);
    if (info?.status !== "locked") return;
    if (!this.hello.enabled) await this.#readHello();
    this.siteWaiting = true;
    this.helloForSite = true;
  }
  /** The user came to the page. */
  pageOpened() {
    this.#helloOnOpenUntil = Date.now() + 4000;
    void this.#loadIcons();
  }
  /* Whether the lock screen should ask Windows Hello by itself: once, just after the page was opened, with the window in front. */
  takeHelloOnOpen() {
    if (MOBILE) {
      return this.hello.enabled && document.visibilityState === "visible";
    }
    const due = Date.now() < this.#helloOnOpenUntil;
    this.#helloOnOpenUntil = 0;
    return due && this.hello.enabled && document.visibilityState === "visible" && document.hasFocus();
  }
  /* Opens the vault with Windows Hello (PIN, fingerprint or face). `quiet`: asked by itself, so a cancelled prompt leaves no error behind. */
  async unlockWithHello(quiet = false) {
    const ok = await this.#run(() => api.helloUnlock().then(() => true));
    if (!ok && quiet) this.error = null;
    if (ok) {
      this.siteWaiting = false;
      this.status = "unlocked";
      await this.refresh();
      this.#syncSoon();
    } else {
      // Reset in Windows, or another vault: it may be off now.
      await this.#readHello();
    }
    return !!ok;
  }
  async setHello(enabled: boolean) {
    try {
      if (enabled) await api.helloEnable();
      else await api.helloDisable();
      toast.success(enabled ? `${QUICK_UNLOCK} opens your vault on this ${DEVICE} now.` : `${QUICK_UNLOCK} no longer opens your vault.`);
    } catch (error) {
      toast.error(message(error));
    }
    await this.#readHello();
  }
  /** Syncs with the account now. Quiet: the indicator shows how it went. */
  async syncNow() {
    clearTimeout(this.#syncTimer);
    if (this.sync.kind === "syncing") return;
    this.sync = { kind: "syncing" };
    const epoch = this.#dataEpoch;
    try {
      const result = await api.sync();
      if (epoch === this.#dataEpoch) await this.#afterSync(result);
    } catch (error) {
      if (epoch === this.#dataEpoch) this.sync = { kind: "error", message: message(error) };
    }
  }
  /** Soon after a change, so a burst of edits goes up together. */
  #syncSoon() {
    clearTimeout(this.#syncTimer);
    this.#syncTimer = setTimeout(() => void this.syncNow(), 1500);
  }
  async #afterSync(result: SyncResult) {
    switch (result.state) {
      case "signedOut":
        this.sync = { kind: "signedOut" };
        return;
      case "nothing":
        this.sync = { kind: "idle" };
        return;
      case "otherVault":
        this.sync = { kind: "otherVault" };
        return;
      case "adopted":
        this.sync = { kind: "synced", at: Date.now(), pending: 0 };
        await this.#readStatus();
        toast.info(`Your vault from your account is on this ${DEVICE} now. Open it with its master password.`);
        return;
      case "synced":
        this.sync = { kind: "synced", at: Date.now(), pending: result.pending };
        if (result.rejected) {
          toast.error(
            `${result.rejected} ${result.rejected === 1 ? "entry" : "entries"} from your account did not open with your vault's key and were left out.`,
          );
        }
        if (this.status === "unlocked") await this.refresh();
    }
  }
  /** Replaces this PC's vault with the account's, after asking. */
  async useAccountVault() {
    const ok = await confirm({
      title: "Use the vault from your account?",
      message:
        `This ${DEVICE}'s vault is replaced by the one in your account, which opens with its own master password. ` +
        `Entries only on this ${DEVICE} are lost` +
        (DEVICE === "PC" ? ": export a backup first if you want to keep them." : "."),
      confirmLabel: `Replace this ${DEVICE}'s vault`,
      danger: true,
    });
    if (!ok) return;
    try {
      await api.useAccountVault();
      this.#onLocked(false);
      await this.#readStatus();
      this.sync = { kind: "synced", at: Date.now(), pending: 0 };
    } catch (error) {
      toast.error(message(error));
    }
  }
  async refresh() {
    if (this.status !== "unlocked") return;
    const epoch = this.#vaultEpoch;
    const sequence = ++this.#listSequence;
    try {
      const entries = await api.list();
      if (epoch !== this.#vaultEpoch || sequence !== this.#listSequence) return;
      this.entries = entries;
    } catch (error) {
      if (epoch !== this.#vaultEpoch || sequence !== this.#listSequence) return;
      if (message(error).includes("locked")) this.#onLocked(false);
      else toast.error(message(error));
      return;
    }
    void this.#loadIcons();
  }
  /* Asks for the icons of websites new to the page; the app fetches the missing ones in the background and sends each as it arrives. */
  async #loadIcons() {
    if (!this.websiteIcons || this.status !== "unlocked") return;
    const hosts = [...new Set(this.entries.flatMap((e) => e.urls.map(iconHost)).filter((h): h is string => !!h))];
    const now = Date.now();
    const wanted = hosts.filter((host) => !this.icons[host] &&
      (!this.#askedIcons.has(host) || now - this.#askedIcons.get(host)! >= ICON_RETRY_AFTER));
    if (!wanted.length) return;
    for (const host of wanted) this.#askedIcons.set(host, now);
    const epoch = this.#iconEpoch;
    let retryAfter = ICON_RETRY_AFTER;
    try {
      const known = await api.icons(Object.keys(this.icons));
      if (epoch !== this.#iconEpoch) return;
      if (this.status === "unlocked" && this.websiteIcons) Object.assign(this.icons, known);
    } catch {
      if (epoch !== this.#iconEpoch) return;
      // A failed IPC call must not mark these sites as permanently requested.
      for (const host of wanted) this.#askedIcons.delete(host);
      retryAfter = 20_000;
    }
    if (epoch === this.#iconEpoch && this.status === "unlocked" && this.websiteIcons) {
      clearTimeout(this.#iconRetryTimer);
      if (hosts.some((host) => !this.icons[host])) {
        this.#iconRetryTimer = setTimeout(() => void this.#loadIcons(), retryAfter);
      }
    }
  }
  async setWebsiteIcons(on: boolean) {
    try {
      await api.setWebsiteIcons(on);
      this.websiteIcons = on;
      this.#iconEpoch++;
      clearTimeout(this.#iconRetryTimer);
      this.icons = {};
      this.#askedIcons.clear();
      if (on) await this.#loadIcons();
    } catch (error) {
      toast.error(message(error));
    }
  }
  /** The icon for an entry's addresses, if one was found. */
  iconFor(urls: string[]) {
    for (const url of urls) {
      const host = iconHost(url);
      if (host && this.icons[host]) return this.icons[host];
    }
    return null;
  }
  /** Runs `work` with the page marked busy; the error goes to `this.error`. */
  async #run<T>(work: () => Promise<T>): Promise<T | null> {
    const epoch = this.#dataEpoch;
    const vaultEpoch = this.#vaultEpoch;
    this.busy = true;
    this.error = null;
    try {
      const result = await work();
      return epoch === this.#dataEpoch && vaultEpoch === this.#vaultEpoch ? result : null;
    } catch (error) {
      if (epoch === this.#dataEpoch && vaultEpoch === this.#vaultEpoch) this.error = message(error);
      return null;
    } finally {
      if (epoch === this.#dataEpoch && vaultEpoch === this.#vaultEpoch) this.busy = false;
    }
  }
  /** Returns the recovery code to show once. */
  async create(master: string) {
    return this.#run(() => api.create(master));
  }
  /** After the recovery code was saved; with Windows Hello if asked. */
  async finishSetup(useHello = false) {
    this.status = "unlocked";
    await this.refresh();
    this.#syncSoon();
    if (useHello) await this.setHello(true);
  }
  async unlock(master: string) {
    const ok = await this.#run(() => api.unlock(master).then(() => true));
    if (ok) {
      this.status = "unlocked";
      await this.refresh();
      // Merges what changed elsewhere while this PC was locked.
      this.#syncSoon();
      // Offered once: the menu (⋯) turns it on later.
      if (MOBILE && this.hello.available && !this.hello.enabled && !readFlag(QUICK_UNLOCK_OFFERED_KEY, false)) {
        writeFlag(QUICK_UNLOCK_OFFERED_KEY, true);
        void confirm({
          title: `Enable ${QUICK_UNLOCK}?`,
          message: `Open your vault with ${QUICK_UNLOCK} next time instead of typing your master password.`,
          confirmLabel: `Enable ${QUICK_UNLOCK}`,
        }).then((enable) => {
          if (enable) void this.setHello(true);
        });
      }
    }
    return !!ok;
  }
  async recover(code: string, master: string) {
    const ok = await this.#run(() => api.recover(code, master).then(() => true));
    if (ok) {
      this.status = "unlocked";
      toast.success("Your new master password is set.");
      await this.refresh();
      this.#syncSoon();
    }
    return !!ok;
  }
  async changeMaster(current: string, master: string) {
    const ok = await this.#run(() => api.changeMaster(current, master).then(() => true));
    if (ok) {
      toast.success("Master password changed.");
      this.#syncSoon();
    }
    return !!ok;
  }
  async lock() {
    await api.lock().catch(() => {});
    this.#onLocked(false);
  }
  /** After the phone removed its local files: forget all in-memory UI data. */
  resetLocal() {
    this.#dataEpoch++;
    this.#vaultEpoch++;
    this.#iconEpoch++;
    this.#hideAll();
    clearTimeout(this.#copiedTimer);
    clearTimeout(this.#syncTimer);
    clearTimeout(this.#iconRetryTimer);
    this.#askedIcons.clear();
    this.#helloOnOpenUntil = 0;
    this.status = "new";
    this.entries = [];
    this.icons = {};
    this.panel = { kind: "none" };
    this.query = "";
    this.filter = "all";
    this.sort = "name";
    this.copied = null;
    this.windowsTarget = null;
    this.busy = false;
    this.error = null;
    this.damaged = 0;
    this.autoLockMinutes = 5;
    this.hello = { available: false, enabled: false };
    this.websiteIcons = true;
    this.siteWaiting = false;
    this.helloForSite = false;
    this.checkedAccount = false;
    this.sync = { kind: "signedOut" };
  }
  #onLocked(automatic: boolean) {
    this.#vaultEpoch++;
    this.busy = false;
    if (this.status !== "unlocked") return;
    this.status = "locked";
    this.entries = [];
    this.#hideAll();
    this.icons = {};
    this.#iconEpoch++;
    clearTimeout(this.#iconRetryTimer);
    this.#askedIcons.clear();
    this.panel = { kind: "none" };
    if (automatic) toast.info("The password vault locked itself.");
  }
  async setAutoLock(minutes: number) {
    this.autoLockMinutes = minutes;
    await api.setAutoLock(minutes).catch((error) => toast.error(message(error)));
  }
  /** Hides a shown password (and forgets its timer). */
  hide(id: string) {
    clearTimeout(this.#revealTimers.get(id));
    this.#revealTimers.delete(id);
    if (this.revealed[id] === undefined) return;
    const { [id]: _hidden, ...rest } = this.revealed;
    this.revealed = rest;
  }
  #hideAll() {
    for (const timer of this.#revealTimers.values()) clearTimeout(timer);
    this.#revealTimers.clear();
    this.revealed = {};
  }
  async toggleReveal(id: string) {
    if (this.revealed[id] !== undefined) {
      this.hide(id);
      return;
    }
    const epoch = this.#vaultEpoch;
    try {
      const password = await api.reveal(id);
      if (epoch !== this.#vaultEpoch || this.status !== "unlocked") return;
      this.revealed = { ...this.revealed, [id]: password };
      // Hidden again after a while: a shown password is not left on screen. Its own timer, so one shown, hidden and shown again gets the full time.
      clearTimeout(this.#revealTimers.get(id));
      this.#revealTimers.set(
        id,
        setTimeout(() => this.hide(id), 30_000),
      );
    } catch (error) {
      if (epoch === this.#vaultEpoch) toast.error(message(error));
    }
  }
  /** Marks a Copy button as done for a moment. */
  #markCopied(key: string) {
    clearTimeout(this.#copiedTimer);
    this.copied = key;
    this.#copiedTimer = setTimeout(() => {
      if (this.copied === key) this.copied = null;
    }, COPIED_FOR);
  }
  async copy(id: string, field: "password" | "username" | "totp") {
    try {
      await api.copy(id, field);
      this.#markCopied(`${id}:${field}`);
      toast.success(
        field === "password"
          ? "Password copied. The clipboard clears in 30 seconds."
          : field === "totp"
            ? "2FA code copied. The clipboard clears in 30 seconds."
            : "User name copied.",
      );
    } catch (error) {
      toast.error(message(error));
    }
  }
  async copyText(text: string) {
    try {
      await api.copyText(text);
      this.#markCopied("text");
      toast.success("Copied. The clipboard clears in 30 seconds.");
    } catch (error) {
      toast.error(message(error));
    }
  }
  async save(entry: EntryInput) {
    const epoch = this.#vaultEpoch;
    try {
      const id = await api.save(entry);
      if (epoch !== this.#vaultEpoch) return false;
      this.hide(id);
      await this.refresh();
      if (epoch !== this.#vaultEpoch) return false;
      this.#syncSoon();
      this.panel = { kind: "view", id };
      toast.success(entry.id ? "Saved." : "Added to your vault.");
      return true;
    } catch (error) {
      toast.error(message(error));
      return false;
    }
  }
  async toggleFavorite(id: string) {
    const e = this.entry(id);
    if (!e) return;
    try {
      await api.save({ ...e, favorite: !e.favorite });
      await this.refresh();
      this.#syncSoon();
    } catch (error) {
      toast.error(message(error));
    }
  }
  async removePasskey(id: string, credentialId: string) {
    const key = this.entry(id)?.passkeys.find((k) => k.credentialId === credentialId);
    if (!key) return;
    const ok = await confirm({
      title: `Delete the passkey for ${key.rpId}?`,
      message: `${key.userName || "This account"} can no longer sign in to ${key.rpId} with it, on any device your account syncs with. Remove it on the site too.`,
      confirmLabel: "Delete passkey",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.passkeyDelete(id, credentialId);
      await this.refresh();
      this.#syncSoon();
    } catch (error) {
      toast.error(message(error));
    }
  }
  /** "Delete all passwords", then sync so the account and other devices follow. */
  async deleteAll(master: string, withPasskeys: boolean) {
    this.error = null;
    try {
      const r = await api.deleteAll(master, withPasskeys);
      this.panel = { kind: "none" };
      await this.refresh();
      await this.syncNow();
      const parts = [`${r.deleted} ${r.deleted === 1 ? "entry" : "entries"} deleted`];
      if (r.stripped) parts.push(`${r.stripped} kept for their passkeys, password removed`);
      if (r.damaged) parts.push(`${r.damaged} damaged left as they were`);
      const synced = this.sync.kind === "error" ? ` Sync failed: ${this.sync.message}. It is sent next time the account is reached.` : "";
      (synced ? toast.error : toast.success)(`${parts.join(" · ")}.${synced}`);
      return r;
    } catch (e) {
      this.error = message(e);
      return null;
    }
  }
  async remove(id: string) {
    const e = this.entry(id);
    if (!e) return;
    if (ANDROID && e.passkeys.length) {
      toast.info("This login includes a passkey managed on your PC. Delete the login from MYLE on your PC.");
      return;
    }
    const ok = await confirm({
      title: `Delete “${e.title}”?`,
      message: `It is removed from this ${DEVICE} and from every device your account syncs with.`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.remove(id);
      this.panel = { kind: "none" };
      await this.refresh();
      this.#syncSoon();
    } catch (error) {
      toast.error(message(error));
    }
  }
}
export const passwords = new PasswordsState();
/** After an import: the new entries go up to the account. */
export function afterImport() {
  void passwords.syncNow();
}
