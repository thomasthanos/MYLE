// Typed bridge to the Rust vault (backend/src/passwords). The vault key never
// comes here; a password does only when the user reveals it.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type VaultStatus = "new" | "locked" | "unlocked";
export type Strength = "none" | "weak" | "fair" | "strong";

export interface StatusInfo {
  status: VaultStatus;
  autoLockMinutes: number;
  /** Entries that did not open with the vault key (damaged or changed). */
  damaged: number;
  /** The list shows each website's icon. */
  websiteIcons: boolean;
}

/** A website's icon, fetched in the background. */
export interface WebsiteIcon {
  host: string;
  /** A `data:` URL. */
  icon: string;
}

export interface AppLink {
  /** Full executable path; older links may contain only the file name. */
  exe: string;
  name: string;
}

export interface Summary {
  id: string;
  title: string;
  username: string;
  urls: string[];
  apps: AppLink[];
  notes: string;
  favorite: boolean;
  folder: string;
  updatedAt: number;
  hasPassword: boolean;
  strength: Strength;
  reused: boolean;
  historyCount: number;
  /** It has a 2FA key: its codes can be shown. */
  hasTotp: boolean;
  /** Its passkeys (never their keys). */
  passkeys: PasskeyInfo[];
}

/** A passkey a login holds, as the page sees it. */
export interface PasskeyInfo {
  credentialId: string;
  /** The site it signs in to. */
  rpId: string;
  userName: string;
  userDisplayName: string;
  createdAt: number;
}

/** A login linked to the program the hotkey was pressed in. */
export interface WindowsMatch {
  id: string;
  /** Linked by the program's path; else by file name only (not enough to fill). */
  exact: boolean;
}

/** A passkey's site asks for the master password (no Windows Hello here). */
export interface VerifyRequest {
  id: number;
  site: string;
}

/** A 2FA code, and how long it still holds. */
export interface TotpCode {
  code: string;
  period: number;
  /** Seconds until the next code. */
  remaining: number;
}

/** What a 2FA key says about itself (never the key). */
export interface TotpInfo {
  issuer: string;
  account: string;
  digits: number;
  period: number;
  algorithm: string;
}

export interface OldPassword {
  password: string;
  changedAt: number;
}

export interface EntryInput {
  id?: string;
  title: string;
  username: string;
  /** Leave out to keep the saved one. */
  password?: string;
  urls: string[];
  apps: AppLink[];
  notes: string;
  favorite: boolean;
  folder: string;
  /** The 2FA key or its otpauth:// link; leave out to keep it, "" removes it. */
  totp?: string;
}

export interface GeneratorOptions {
  length: number;
  lower: boolean;
  upper: boolean;
  digits: boolean;
  symbols: boolean;
  avoidAmbiguous: boolean;
}

/** What a sync with the account did. */
export type SyncResult =
  | { state: "signedOut" }
  | { state: "nothing" }
  | { state: "synced"; sent: number; pending: number; rejected: number }
  /** This PC took the account's vault: it opens with that vault's master password. */
  | { state: "adopted" }
  /** The account holds another vault than this PC. */
  | { state: "otherVault" };

/** Windows Hello on this PC. */
export interface HelloStatus {
  /** A PIN, fingerprint or face is set up in Windows. */
  available: boolean;
  /** It opens this vault here. */
  enabled: boolean;
}

/** A browser's request through the extension (no address, only which browser). */
export interface BrowserContact {
  browser: string;
  /** Seconds since 1970. */
  at: number;
  /** Which copy of the extension: the store's, one loaded from a folder, or Firefox's. */
  copy: "store" | "folder" | "firefox" | "";
}

export interface BrowserSetup {
  enabled: boolean;
  /** The extension's folder, for "Load unpacked". */
  extensionDir: string | null;
  /** Why a browser would not find MYLE, if one would not. */
  registrationError: string | null;
  lastContact: BrowserContact | null;
  /** The last start MYLE turned away: another browser, say. */
  lastRefusal: { program: string; reason: string; at: number } | null;
  vault: VaultStatus;
  /** Seconds since 1970, as the app counts them. */
  now: number;
}

export interface ImportPreview {
  path: string;
  /** The app's own backup: needs its password first. */
  backup: boolean;
  count: number;
  sample: [string, string][];
}

export interface PasswordsApi {
  status(): Promise<StatusInfo>;
  create(master: string): Promise<string>;
  unlock(master: string): Promise<void>;
  recover(code: string, master: string): Promise<void>;
  changeMaster(current: string, master: string): Promise<void>;
  lock(): Promise<void>;
  setAutoLock(minutes: number): Promise<void>;
  /** The icons known for the vault's websites, but those in `skip`, by host. */
  icons(skip: string[]): Promise<Record<string, string>>;
  setWebsiteIcons(on: boolean): Promise<void>;
  onIcon(handler: (icon: WebsiteIcon) => void): Promise<() => void>;
  list(): Promise<Summary[]>;
  reveal(id: string): Promise<string>;
  history(id: string): Promise<OldPassword[]>;
  copy(id: string, field: "password" | "username" | "totp"): Promise<void>;
  totp(id: string): Promise<TotpCode>;
  totpCheck(text: string): Promise<TotpInfo>;
  /** The otpauth:// link in a QR code on the clipboard. */
  totpScanClipboard(): Promise<string>;
  /** The same from a picture the user picks; null when they cancel. */
  totpScanFile(): Promise<string | null>;
  passkeyDelete(id: string, credentialId: string): Promise<void>;
  /** A passkey waiting for the master password, if any. */
  verifyPending(): Promise<VerifyRequest | null>;
  /** The master password for it, or null to refuse. */
  verifyAnswer(id: number, master: string | null): Promise<void>;
  onVerify(handler: (request: VerifyRequest) => void): Promise<() => void>;
  copyText(text: string): Promise<void>;
  save(entry: EntryInput): Promise<string>;
  remove(id: string): Promise<void>;
  generate(options: GeneratorOptions): Promise<string>;
  strength(password: string): Promise<Strength>;
  importPick(): Promise<ImportPreview | null>;
  importFile(path: string, backupPassword?: string): Promise<number>;
  /** Every entry, encrypted with `password`; the master password again first. */
  exportBackup(password: string, master: string): Promise<string | null>;
  sync(): Promise<SyncResult>;
  helloStatus(): Promise<HelloStatus>;
  helloEnable(): Promise<void>;
  helloDisable(): Promise<void>;
  helloUnlock(): Promise<void>;
  browserGet(): Promise<BrowserSetup>;
  browserSet(enabled: boolean): Promise<void>;
  openExtensionDir(): Promise<void>;
  onBrowserContact(handler: (contact: BrowserContact) => void): Promise<() => void>;
  /** The hotkey for filling Windows programs ("Ctrl+Shift+L"), or null when taken. */
  windowsHotkey(): Promise<string | null>;
  windowsFill(id: string, field: "username" | "password" | "both"): Promise<void>;
  windowsMatches(): Promise<WindowsMatch[]>;
  /** Links a login to the program the hotkey was pressed in. */
  windowsLink(id: string): Promise<void>;
  /** A program's .exe, picked in a dialog; null when cancelled. */
  pickProgram(): Promise<string | null>;
  onChanged(handler: () => void): Promise<() => void>;
  useAccountVault(): Promise<void>;
  onLocked(handler: () => void): Promise<() => void>;
  onSynced(handler: (result: SyncResult) => void): Promise<() => void>;
}

const tauriApi: PasswordsApi = {
  status: () => invoke("passwords_status"),
  create: (master) => invoke("passwords_create", { master }),
  unlock: (master) => invoke("passwords_unlock", { master }),
  recover: (code, master) => invoke("passwords_recover", { code, master }),
  changeMaster: (current, master) => invoke("passwords_change_master", { current, master }),
  lock: () => invoke("passwords_lock"),
  setAutoLock: (minutes) => invoke("passwords_set_auto_lock", { minutes }),
  icons: (skip) => invoke("passwords_icons", { skip }),
  setWebsiteIcons: (on) => invoke("passwords_set_website_icons", { on }),
  onIcon: (handler) => listen<WebsiteIcon>("passwords-icon", (event) => handler(event.payload)),
  list: () => invoke("passwords_list"),
  reveal: (id) => invoke("passwords_reveal", { id }),
  history: (id) => invoke("passwords_history", { id }),
  copy: (id, field) => invoke("passwords_copy", { id, field }),
  totp: (id) => invoke("passwords_totp", { id }),
  totpCheck: (text) => invoke("passwords_totp_check", { text }),
  totpScanClipboard: () => invoke("passwords_totp_scan_clipboard"),
  totpScanFile: () => invoke("passwords_totp_scan_file"),
  passkeyDelete: (id, credentialId) => invoke("passwords_passkey_delete", { id, credentialId }),
  verifyPending: () => invoke("passwords_verify_pending"),
  verifyAnswer: (id, master) => invoke("passwords_verify_answer", { id, master }),
  onVerify: (handler) => listen<VerifyRequest>("passwords-verify", (event) => handler(event.payload)),
  copyText: (text) => invoke("passwords_copy_text", { text }),
  save: (entry) => invoke("passwords_save", { entry }),
  remove: (id) => invoke("passwords_delete", { id }),
  generate: (options) => invoke("passwords_generate", { options }),
  strength: (password) => invoke("passwords_strength", { password }),
  importPick: () => invoke("passwords_import_pick"),
  importFile: (path, backupPassword) => invoke("passwords_import", { path, backupPassword }),
  exportBackup: (password, master) => invoke("passwords_export", { password, master }),
  sync: () => invoke("passwords_sync"),
  helloStatus: () => invoke("passwords_hello_status"),
  helloEnable: () => invoke("passwords_hello_enable"),
  helloDisable: () => invoke("passwords_hello_disable"),
  helloUnlock: () => invoke("passwords_hello_unlock"),
  browserGet: () => invoke("passwords_browser_get"),
  browserSet: (enabled) => invoke("passwords_browser_set", { enabled }),
  openExtensionDir: () => invoke("passwords_open_extension_dir"),
  onBrowserContact: (handler) =>
    listen<BrowserContact>("passwords-browser-contact", (event) => handler(event.payload)),
  windowsHotkey: () => invoke("passwords_windows_hotkey_status"),
  windowsFill: (id, field) => invoke("passwords_windows_fill", { id, field }),
  windowsMatches: () => invoke("passwords_windows_matches"),
  windowsLink: (id) => invoke("passwords_windows_link", { id }),
  pickProgram: () => invoke("passwords_pick_program"),
  onChanged: (handler) => listen("passwords-changed", handler),
  useAccountVault: () => invoke("passwords_use_account_vault"),
  onLocked: (handler) => listen("passwords-locked", handler),
  onSynced: (handler) => listen<SyncResult>("passwords-synced", (event) => handler(event.payload)),
};

/** In a plain browser (`npx vite`): a vault in memory, to work on the page. */
function previewApi(): PasswordsApi {
  let status: VaultStatus = new URLSearchParams(location.search).has("new-vault") ? "new" : "locked";
  let websiteIcons = true;
  let browserFilling = true;
  // Stand-ins for fetched icons: a coloured mark per site.
  const mark = (color: string, text: string) =>
    `data:image/svg+xml;base64,${btoa(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="7" fill="${color}"/><text x="16" y="22" font-family="Segoe UI,sans-serif" font-size="16" font-weight="700" fill="#fff" text-anchor="middle">${text}</text></svg>`)}`;
  const previewIcons: Record<string, string> = {
    "github.com": mark("#1f2328", "Gh"),
    "account.riotgames.com": mark("#eb0029", "R"),
    "store.steampowered.com": mark("#1b2838", "S"),
    "netflix.com": mark("#e50914", "N"),
    "discord.com": mark("#5865f2", "D"),
    "accounts.google.com": mark("#4285f4", "G"),
    "spotify.com": mark("#1db954", "S"),
  };
  const now = Math.floor(Date.now() / 1000);
  const passwords = new Map<string, string>([
    ["1", "k8#Vq2!zLp9wRt"],
    ["2", "hunter2"],
    ["3", "hunter2"],
    ["4", "Correct-Horse-Battery-Staple-42"],
    ["5", "Netflix-and-2024"],
    ["6", "q7!Lm2#Vx9@Rt4$w"],
    ["7", "Gz8#nQ2!pW5&kR9m"],
    ["8", "spotify-music-1"],
    ["9", "admin"],
    ["10", "hunter2"],
  ]);
  let items: Summary[] = [
    { id: "1", title: "GitHub", username: "thomas@example.com", urls: ["https://github.com/login"], apps: [], notes: "", favorite: true, folder: "Work", updatedAt: now - 3600, hasPassword: true, strength: "strong", reused: false, historyCount: 2, hasTotp: true, passkeys: [{ credentialId: "pk1", rpId: "github.com", userName: "thomas", userDisplayName: "Thomas", createdAt: now - 86400 * 4 }] },
    { id: "2", title: "Old forum", username: "tommy", urls: ["https://forum.example.org"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 40, hasPassword: true, strength: "weak", reused: true, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "3", title: "Riot Games", username: "tommy_gg", urls: ["https://account.riotgames.com"], apps: [{ exe: "riotclientux.exe", name: "Riot Client" }], notes: "", favorite: true, folder: "Games", updatedAt: now - 86400 * 3, hasPassword: true, strength: "weak", reused: true, historyCount: 1, hasTotp: false, passkeys: [] },
    { id: "4", title: "Steam", username: "thomas_steam", urls: ["https://store.steampowered.com"], apps: [{ exe: "steam.exe", name: "Steam" }], notes: "Steam Guard on phone", favorite: false, folder: "Games", updatedAt: now - 86400 * 10, hasPassword: true, strength: "strong", reused: false, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "5", title: "netflix.com", username: "family@example.com", urls: ["https://www.netflix.com/login"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 2, hasPassword: true, strength: "fair", reused: false, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "6", title: "Discord", username: "thomas#0001", urls: ["https://discord.com/login"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 5, hasPassword: true, strength: "strong", reused: false, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "7", title: "Google", username: "thomas@gmail.example", urls: ["https://accounts.google.com"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 8, hasPassword: true, strength: "strong", reused: false, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "8", title: "Spotify", username: "thomas", urls: ["https://spotify.com"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 20, hasPassword: true, strength: "fair", reused: false, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "9", title: "192.168.1.1", username: "admin", urls: ["http://192.168.1.1"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 60, hasPassword: true, strength: "weak", reused: false, historyCount: 0, hasTotp: false, passkeys: [] },
    { id: "10", title: "account.cosmote.gr", username: "6900000000", urls: ["https://account.cosmote.gr"], apps: [], notes: "", favorite: false, folder: "", updatedAt: now - 86400 * 12, hasPassword: true, strength: "fair", reused: true, historyCount: 0, hasTotp: false, passkeys: [] },
  ];
  const wait = (ms = 250) => new Promise((resolve) => setTimeout(resolve, ms));
  const rate = (p: string): Strength => (!p ? "none" : p.length < 10 ? "weak" : p.length < 16 ? "fair" : "strong");
  return {
    async status() {
      return { status, autoLockMinutes: 5, damaged: 0, websiteIcons };
    },
    async create() {
      await wait(600);
      status = "unlocked";
      return "7Q2M-9KXA-4TZB-EH3R-W8NP-6DFC-JY1V-5SGK-0MQT-2BXW-HR7E-9PZC-4A";
    },
    async unlock(master) {
      await wait(500);
      if (master !== "preview") throw new Error("Wrong master password. (Type “preview” here.)");
      status = "unlocked";
    },
    async recover() {
      await wait(500);
      status = "unlocked";
    },
    async changeMaster() {
      await wait(500);
    },
    async lock() {
      status = "locked";
    },
    async setAutoLock() {},
    async icons(skip) {
      await wait(300);
      return websiteIcons ? Object.fromEntries(Object.entries(previewIcons).filter(([host]) => !skip.includes(host))) : {};
    },
    async setWebsiteIcons(on) {
      websiteIcons = on;
    },
    async onIcon() {
      return () => {};
    },
    async list() {
      return items.map((i) => ({ ...i }));
    },
    async reveal(id) {
      return passwords.get(id) ?? "";
    },
    async history(id) {
      return id === "1" ? [{ password: "old-github-pass", changedAt: now - 86400 * 90 }] : [];
    },
    async copy() {},
    async totp() {
      // A stand-in that changes like a real one.
      const now = Math.floor(Date.now() / 1000);
      const step = Math.floor(now / 30);
      return { code: String((step * 7919) % 1_000_000).padStart(6, "0"), period: 30, remaining: 30 - (now % 30) };
    },
    async totpCheck(text) {
      if (!/^otpauth:\/\/totp\//i.test(text.trim()) && !/^[a-z2-7\s=-]{16,}$/i.test(text.trim())) {
        throw new Error("That is not a 2FA key: use the key the site shows (letters A–Z and digits 2–7) or its otpauth:// link.");
      }
      return { issuer: "", account: "", digits: 6, period: 30, algorithm: "SHA1" };
    },
    async totpScanClipboard() {
      await wait(300);
      return "otpauth://totp/GitHub:thomas?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=GitHub";
    },
    async totpScanFile() {
      return null;
    },
    async passkeyDelete(id, credentialId) {
      items = items.map((i) => (i.id === id ? { ...i, passkeys: i.passkeys.filter((k) => k.credentialId !== credentialId) } : i));
    },
    async verifyPending() {
      return new URLSearchParams(location.search).has("verify") ? { id: 1, site: "github.com" } : null;
    },
    async verifyAnswer(_id, master) {
      await wait(300);
      if (master !== null && master !== "preview") throw new Error("Wrong master password. (Type “preview” here.)");
    },
    async onVerify() {
      return () => {};
    },
    async copyText() {},
    async save(entry) {
      const id = entry.id ?? String(Date.now());
      if (entry.password !== undefined) passwords.set(id, entry.password);
      const password = passwords.get(id) ?? "";
      const { totp, ...fields } = entry;
      const before = items.find((i) => i.id === id);
      const summary: Summary = {
        ...fields,
        id,
        updatedAt: Math.floor(Date.now() / 1000),
        hasPassword: !!password,
        strength: rate(password),
        reused: false,
        historyCount: 0,
        hasTotp: totp === undefined ? !!before?.hasTotp : !!totp,
        passkeys: before?.passkeys ?? [],
      };
      items = entry.id ? items.map((i) => (i.id === id ? summary : i)) : [...items, summary];
      return id;
    },
    async remove(id) {
      items = items.filter((i) => i.id !== id);
    },
    async generate(o) {
      const sets = [o.lower && "abcdefghjkmnpqrstuvwxyz", o.upper && "ABCDEFGHJKLMNPQRSTUVWXYZ", o.digits && "23456789", o.symbols && "!@#$%^&*-_=+?"].filter(Boolean).join("");
      return Array.from(crypto.getRandomValues(new Uint32Array(o.length)), (n) => sets[n % sets.length]).join("");
    },
    async strength(password) {
      return rate(password);
    },
    async importPick() {
      return { path: "C:\\Users\\You\\Downloads\\Chrome Passwords.csv", backup: false, count: 42, sample: [["reddit.com", "me"], ["Netflix", "me@example.com"]] };
    },
    async importFile() {
      await wait(400);
      return 42;
    },
    async exportBackup() {
      await wait(400);
      return "C:\\Users\\You\\Documents\\passwords.myle-vault";
    },
    async sync() {
      await wait(300);
      return { state: "signedOut" };
    },
    async useAccountVault() {},
    async helloStatus() {
      return { available: true, enabled: status === "locked" };
    },
    async helloEnable() {
      await wait(500);
    },
    async helloDisable() {},
    async helloUnlock() {
      await wait(700);
      status = "unlocked";
    },
    async browserGet() {
      const seconds = Math.floor(Date.now() / 1000);
      const connected = new URLSearchParams(location.search).has("browser-connected");
      return {
        enabled: browserFilling,
        extensionDir: "C:\\Users\\You\\AppData\\Local\\ThomasThanos\\MakeYourLifeEasier\\extension",
        registrationError: null,
        lastContact: connected
          ? { browser: "Edge", at: seconds - 95, copy: new URLSearchParams(location.search).has("folder-copy") ? "folder" : "store" }
          : null,
        lastRefusal: null,
        vault: status,
        now: seconds,
      };
    },
    async browserSet(enabled) {
      browserFilling = enabled;
    },
    async openExtensionDir() {},
    async onBrowserContact() {
      return () => {};
    },
    async windowsHotkey() {
      return "Ctrl+Shift+L";
    },
    async windowsFill() {
      await wait(300);
    },
    async windowsMatches() {
      return new URLSearchParams(location.search).has("windows-linked") ? [{ id: "3", exact: true }, { id: "4", exact: false }] : [];
    },
    async windowsLink() {},
    async pickProgram() {
      return "C:\\Riot Games\\Riot Client\\RiotClientServices.exe";
    },
    async onChanged() {
      return () => {};
    },
    async onLocked() {
      return () => {};
    },
    async onSynced() {
      return () => {};
    },
  };
}

export const passwordsApi: PasswordsApi = isTauri() ? tauriApi : previewApi();
