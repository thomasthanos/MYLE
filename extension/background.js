// MYLE Passwords: the go-between for the page and the app.
//
// Pages (content.js) and the popup ask here; this asks the app through the
// native messaging host ("com.thomasthanos.myle"). The page's address always
// comes from the browser (sender.url or webNavigation), never from the page.
//
// Our script in a page may ask only about its own frame: its logins, a fill,
// a new password, and saving what the user typed and sent there. A sign-in
// form embedded from another site is filled only from the toolbar popup,
// where the user sees whose it is. Only the popup looks across a tab's frames.
// Chromium's service worker loads the Public Suffix List here; Firefox lists it
// before this script in its manifest.
if (!globalThis.MYLE_PUBLIC_SUFFIXES && typeof importScripts === "function") importScripts("psl.js");

const ext = globalThis.browser ?? globalThis.chrome;
const HOST = "com.thomasthanos.myle";
const OWN_PAGES = ext.runtime.getURL("");
/** A sent login waits this long for the next page, and then for a click. */
const WAIT_FOR_PAGE = 60_000;
const WAIT_FOR_CLICK = 3 * 60_000;
/** An offer shows on one page: again only on a page right after it (a
 *  sign-in that redirects once more), never on a reload or a later page. */
const SHOWN_GRACE = 5_000;
/** A password suggested in a sign-up form, until that form is sent. */
const KEEP_SUGGESTED = 30 * 60_000;

/** Why the browser could not reach MYLE, from the browser's own message. */
function hostProblem(detail) {
  // MYLE never registered with this browser, or the note it left is gone.
  if (/not found|no such native application/i.test(detail)) return "hostMissing";
  // This copy of the extension has an id MYLE does not know.
  if (/forbidden/i.test(detail)) return "hostForbidden";
  // MYLE started but turned the browser away, or stopped.
  if (/exited|communicating|unexpected/i.test(detail)) return "hostExited";
  return "noHost";
}

/** How long an answer may take. MYLE is started by the host when it is closed,
 *  which takes a few seconds; past this, the browser is told it is not there,
 *  so a page is never left waiting on a call that will not come back. */
const ANSWER_LIMIT = 30_000;
// Native verification has a total 120s deadline, plus time to start MYLE.
const PASSKEY_ANSWER_LIMIT = 150_000;

async function ask(message, limit = ANSWER_LIMIT) {
  let deadline;
  try {
    return await Promise.race([
      // The version tells MYLE when a folder copy is older than its own.
      ext.runtime.sendNativeMessage(HOST, { ...message, extVersion: ext.runtime.getManifest?.()?.version ?? "" }),
      new Promise((resolve) => {
        deadline = setTimeout(() => resolve({ ok: false, error: "noHost", detail: "no answer in time" }), limit);
      }),
    ]);
  } catch (error) {
    const detail = String(error?.message ?? error);
    return { ok: false, error: hostProblem(detail), detail };
  } finally {
    clearTimeout(deadline);
  }
}

function hostOf(value) {
  try {
    return new URL(value).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

/**
 * The part of a host that one owner controls, by the Public Suffix List, as the
 * app reckons it (`login.example.com` → `example.com`, `a.github.io` stays
 * `a.github.io`, since each github.io name has its own owner).
 */
function siteOf(host) {
  const labels = host.toLowerCase().replace(/\.$/, "").split(".");
  if (labels.length < 2 || /^[\d.]+$/.test(host) || host.startsWith("[")) return host;
  const rules = globalThis.MYLE_PUBLIC_SUFFIXES;
  // The longest rule that fits wins; with none, the last label is the suffix.
  let suffix = 1;
  for (let i = 0; i < labels.length; i++) {
    const name = labels.slice(i).join(".");
    if (rules.has(`!${name}`)) {
      suffix = labels.length - i - 1;
      break;
    }
    if (rules.has(name) || (i + 1 < labels.length && rules.has(`*.${labels.slice(i + 1).join(".")}`))) {
      suffix = labels.length - i;
      break;
    }
  }
  return labels.length > suffix ? labels.slice(-suffix - 1).join(".") : host;
}

/** The same site: the same host, or hosts of one owner (login.example.com, example.com). */
function sameSite(a, b) {
  const ha = hostOf(a);
  const hb = hostOf(b);
  return !!ha && !!hb && (ha === hb || siteOf(ha) === siteOf(hb));
}

function allowedUrl(value) {
  try {
    const url = new URL(value);
    return url.protocol === "https:" ||
      (url.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname));
  } catch {
    return false;
  }
}

const text = (value, max) => typeof value === "string" && value.length <= max;
const refused = { ok: false, error: "badRequest" };

// --- Who is asking ------------------------------------------------------------

/** One of this extension's own pages: the toolbar popup. */
const fromPopup = (sender) => typeof sender.url === "string" && sender.url.startsWith(OWN_PAGES);

/** Our script in a frame of a page we fill, in the tab the user is looking at. */
const fromPage = (sender) =>
  !fromPopup(sender) &&
  Number.isInteger(sender.tab?.id) &&
  sender.tab.active !== false &&
  Number.isInteger(sender.frameId) &&
  allowedUrl(sender.url) &&
  (!sender.documentLifecycle || sender.documentLifecycle === "active");

/** For a frame inside a page of another site: that page's host, else "". */
async function embeddedIn(sender) {
  if (sender.frameId === 0) return "";
  let top = "";
  try {
    top = (await ext.webNavigation.getFrame({ tabId: sender.tab.id, frameId: 0 }))?.url ?? "";
  } catch {
    // Not known: treated as another site.
  }
  return sameSite(sender.url, top) ? "" : hostOf(top) || "another site";
}

let lastOpen = 0;

/** Brings MYLE forward, at most every two seconds. */
function openApp() {
  if (Date.now() - lastOpen < 2000) return { ok: true };
  lastOpen = Date.now();
  return ask({ type: "open" });
}

// --- The popup: every sign-in frame of the tab ------------------------------------

async function framesFor(tabId) {
  if (!Number.isInteger(tabId)) return null;
  const tab = await ext.tabs.get(tabId);
  if (!tab?.active) return null;
  const frames = await ext.webNavigation.getAllFrames({ tabId });
  return (frames ?? [])
    .filter((frame) => allowedUrl(frame.url) && (!frame.documentLifecycle || frame.documentLifecycle === "active"))
    .sort((a, b) => a.frameId - b.frameId);
}

async function inspectFrame(tabId, frameId) {
  try {
    const reply = await ext.tabs.sendMessage(tabId, { type: "inspect" }, { frameId });
    return {
      fields: reply?.fields === true,
      password: reply?.password === true,
      focused: reply?.focused === true,
      insecure: reply?.insecure === true,
    };
  } catch {
    return { fields: false, password: false, focused: false, insecure: false };
  }
}

async function loginsForTab(tabId) {
  let frames;
  try {
    frames = await framesFor(tabId);
  } catch {
    return { ok: false, error: "pageChanged" };
  }
  if (!frames?.length) return { ok: false, error: "insecure" };
  const topUrl = frames.find((frame) => frame.frameId === 0)?.url ?? "";

  const items = [];
  const seen = new Map();
  // The main page is useful even before its sign-in form appears. For embedded
  // sites, list only frames where our content script sees a login field.
  for (const frame of frames) {
    const inspection = await inspectFrame(tabId, frame.frameId);
    if (frame.frameId !== 0 && !inspection.fields) continue;
    const answer = await ask({ type: "logins", url: frame.url });
    if (!answer?.ok) return answer;
    const frameSite = hostOf(frame.url);
    const embedded = frame.frameId !== 0 && !sameSite(frame.url, topUrl);
    const rank = Number(inspection.fields) + Number(inspection.password) + Number(inspection.focused) * 2;
    for (const login of answer.logins ?? []) {
      const key = `${new URL(frame.url).origin}:${login.id}`;
      const item = { ...login, frameId: frame.frameId, frameSite, embedded };
      const previous = seen.get(key);
      if (previous) {
        // Prefer the frame whose sign-in field is focused, then a password
        // field over a lone email field on the same origin.
        if (rank > previous.rank) {
          items[previous.index] = item;
          previous.rank = rank;
        }
      } else {
        seen.set(key, { index: items.length, rank });
        items.push(item);
      }
    }
  }
  return { ok: true, logins: items, site: hostOf(topUrl) };
}

async function fillTab(tabId, frameId, id) {
  if (!Number.isInteger(frameId) || !text(id, 100)) return refused;
  let frames;
  try {
    frames = await framesFor(tabId);
  } catch {
    return { ok: false, error: "pageChanged" };
  }
  const frame = frames?.find((item) => item.frameId === frameId);
  if (!frame) return { ok: false, error: "pageChanged" };
  const inspection = await inspectFrame(tabId, frameId);
  if (!inspection.fields) return { ok: false, error: "noFields" };
  // Refused before the vault is asked for the password.
  if (inspection.insecure) return { ok: false, error: "insecureForm" };

  // Authorize the real frame URL with the vault immediately before sending to
  // that exact frame. The content script checks its origin once more.
  const credentials = await ask({ type: "fill", id, url: frame.url });
  if (!credentials?.ok) return credentials;
  try {
    const current = (await framesFor(tabId))?.find((item) => item.frameId === frameId);
    if (!current || current.url !== frame.url) return { ok: false, error: "pageChanged" };
    const reply = await ext.tabs.sendMessage(tabId, {
      type: "apply",
      origin: new URL(frame.url).origin,
      username: credentials.username,
      password: credentials.password,
    }, { frameId });
    return reply?.applied === true ? { ok: true, applied: true } :
      { ok: false, error: reply?.error ?? "noFields" };
  } catch {
    return { ok: false, error: "noContent" };
  }
}

/**
 * Has the app copy a login's user name, password or 2FA code for a frame of
 * the tab (the popup's Copy buttons). The app checks the login is saved for
 * that frame's site, puts the text on the clipboard itself (marked secret,
 * cleared after 30 seconds) and answers only whether it did: the text never
 * comes through the browser.
 */
async function copyTab(tabId, frameId, id, field) {
  if (!Number.isInteger(frameId) || !text(id, 100) || !["username", "password", "totp"].includes(field)) return refused;
  let frames;
  try {
    frames = await framesFor(tabId);
  } catch {
    return { ok: false, error: "pageChanged" };
  }
  const frame = frames?.find((item) => item.frameId === frameId);
  if (!frame) return { ok: false, error: "pageChanged" };
  const answer = await ask({ type: "copy", id, url: frame.url, field });
  // A MYLE older than this extension does not know the request.
  if (answer?.ok === false && answer.error === "badRequest") return { ok: false, error: "oldApp" };
  return answer;
}

// --- Offering to save ------------------------------------------------------------
//
// A login the user typed and sent is kept in the browser's memory (never on
// disk), under a random nonce, until the next page of that site offers to
// save it. The page gets only the nonce and the user name back, never the
// password, and "save" works only with the nonce of an offer it was shown.

const pendingKey = (tabId) => `pending:${tabId}`;
/** Sent logins still being checked with the app, by tab: a fast next page
 *  waits for them before it asks for an offer. */
const checking = new Map();
/** Saves waiting on an unlock; dismissing or replacing an offer cancels them. */
const saveWaits = new Map();
const suggestedKey = (tabId) => `suggested:${tabId}`;

async function readPending(tabId) {
  const key = pendingKey(tabId);
  const found = (await ext.storage.session.get(key))[key];
  if (!found) return null;
  if (Date.now() >= (found.shownAt ? found.shownAt + WAIT_FOR_CLICK : found.at + WAIT_FOR_PAGE)) {
    await ext.storage.session.remove(key);
    return null;
  }
  return found;
}

/** A strong password from the app, for the sign-up form the user clicked. */
async function suggest(sender) {
  const answer = await ask({ type: "generate" });
  if (answer?.ok) {
    await ext.storage.session.set({
      [suggestedKey(sender.tab.id)]: { password: answer.password, url: sender.url, at: Date.now() },
    });
  }
  return answer;
}

async function submitted(message, sender) {
  if (!text(message.username, 256) || !text(message.password, 512) || !message.password) return refused;
  const tabId = sender.tab.id;
  const known = await ask({ type: "known", url: sender.url, username: message.username, password: message.password });
  // While the vault is locked nothing can be checked, so it is offered:
  // saving waits for the unlock, and skips a login the vault already has.
  const locked = !known?.ok && known?.error === "locked";
  if ((known?.ok && !known.known) || locked) {
    saveWaits.delete(tabId);
    await ext.storage.session.set({
      [pendingKey(tabId)]: {
        url: sender.url,
        username: message.username,
        password: message.password,
        update: known?.update === true,
        locked,
        at: Date.now(),
        nonce: crypto.randomUUID(),
      },
    });
  }
  return { ok: true };
}

/** Waits (two minutes at most) for the user to unlock the vault in MYLE;
 *  `stopped()` ends the wait early (the page no longer needs it). The wait
 *  covers MYLE being started here too, so a page that asked while it was
 *  closed carries on by itself once it is open and unlocked. */
async function whenUnlocked(stopped = () => false, limit = 120_000) {
  return (await untilUnlocked(stopped, limit)).unlocked;
}

/** Polls MYLE until its vault is open, `stopped()` or `limit` ms, answering
 *  with the last thing seen. `unlocked` only when MYLE said so; a status that
 *  names no vault state ends the wait at once instead of polling for two
 *  minutes and then blaming the vault: that answer means a copy of MYLE this
 *  extension does not understand. */
async function untilUnlocked(stopped, limit) {
  const until = Date.now() + limit;
  let state = "unknown";
  let detail = "";
  while (Date.now() < until && !stopped()) {
    const status = await ask({ type: "status" });
    if (stopped()) return { unlocked: false, state, detail };
    const answer = (value, note = "") => ({ unlocked: false, state: value, detail: note });
    if (!status?.ok) {
      state = String(status?.error ?? "noHost");
      detail = String(status?.detail ?? "");
      if (state === "disabled" || state === "noVault") return answer(state, detail);
    } else {
      detail = JSON.stringify(status);
      state = typeof status.state === "string" && status.state ? status.state : "noState";
      if (state === "new") return answer("noVault", detail);
      if (status.enabled === false) return answer("disabled", detail);
      if (state === "unlocked") return { unlocked: true, state, detail };
      if (state === "noState") return answer(state, detail);
    }
    await new Promise((resolve) => setTimeout(resolve, 1500));
  }
  return { unlocked: false, state, detail };
}

/** A page waiting for the vault to open, by tab: one wait each, and a new
 *  one (or the page saying it is done) ends the one before. */
const unlockWaits = new Map();

/** `seconds` (at most 30): the page asks again in rounds, saying in its
 *  prompt how far MYLE is (starting, locked), until its own time is up. */
async function waitForUnlock(sender, seconds) {
  const tabId = sender.tab.id;
  const token = {};
  unlockWaits.set(tabId, token);
  const limit = Number.isFinite(seconds) ? Math.min(Math.max(seconds, 2), 30) * 1000 : 120_000;
  const { unlocked, state, detail } = await untilUnlocked(() => unlockWaits.get(tabId) !== token, limit);
  if (unlockWaits.get(tabId) === token) unlockWaits.delete(tabId);
  if (unlocked) void syncPasskeySites();
  return { ok: true, unlocked, state, detail };
}

async function offer(sender) {
  await checking.get(sender.tab.id)?.catch(() => {});
  const found = await readPending(sender.tab.id);
  // An identity provider can redirect through another site before returning
  // to the sign-in site: the offer waits for a page of that site.
  if (!found || !sameSite(found.url, sender.url)) return null;
  // Seen on a page that stayed (not a redirect), or long ago: said once.
  if (found.settled || (found.shownAt && Date.now() - found.shownAt > SHOWN_GRACE)) return null;
  if (!found.shownAt) {
    found.shownAt = Date.now();
    await ext.storage.session.set({ [pendingKey(sender.tab.id)]: found });
  }
  return {
    nonce: found.nonce,
    username: found.username,
    update: found.update,
    locked: found.locked === true,
    site: hostOf(found.url),
  };
}

/** The page showing the offer stayed: it is not shown on another page. */
async function offerSettled(message, sender) {
  const found = await readPending(sender.tab.id);
  if (found?.nonce === message.nonce && !found.settled) {
    found.settled = true;
    await ext.storage.session.set({ [pendingKey(sender.tab.id)]: found });
  }
  return { ok: true };
}

async function answerOffer(message, sender, save) {
  if (!text(message.nonce, 64)) return refused;
  const found = await readPending(sender.tab.id);
  if (!found?.shownAt || found.nonce !== message.nonce || !sameSite(found.url, sender.url)) {
    return { ok: false, error: "expired" };
  }
  const tabId = sender.tab.id;
  if (!save) saveWaits.delete(tabId);
  let answer = { ok: true };
  if (save) {
    const token = {};
    saveWaits.set(tabId, token);
    const stopped = () => saveWaits.get(tabId) !== token;
    const login = { url: found.url, username: found.username, password: found.password };
    try {
      answer = await ask({ type: "save", ...login });
      if (stopped()) return { ok: false, error: "expired" };
      if (answer?.error === "locked") {
        // MYLE comes forward; the login is saved once the vault is open,
        // only while the user still wants this offer saved.
        void openApp();
        if (!(await whenUnlocked(stopped))) return { ok: false, error: stopped() ? "expired" : "locked" };
        const known = await ask({ type: "known", ...login });
        if (stopped()) return { ok: false, error: "expired" };
        answer = known?.ok && known.known ? { ok: true, already: true } : await ask({ type: "save", ...login });
      }
      if (stopped()) return { ok: false, error: "expired" };
      // Still not saved: the offer stays, to try again.
      if (!answer?.ok) return answer;
    } finally {
      if (!stopped()) saveWaits.delete(tabId);
    }
  }
  await ext.storage.session.remove([pendingKey(sender.tab.id), suggestedKey(sender.tab.id)]);
  return { ok: true, already: answer.already === true, updated: answer.updated === true };
}

// --- 2FA keys ---------------------------------------------------------------------

/** The toolbar popup: the 2FA QR code shown in the tab, read by MYLE. */
async function scanTab(tabId) {
  if (!Number.isInteger(tabId)) return refused;
  let tab;
  try {
    tab = await ext.tabs.get(tabId);
  } catch {
    return { ok: false, error: "pageChanged" };
  }
  if (!tab?.active || !allowedUrl(tab.url)) return { ok: false, error: "insecure" };
  let image;
  try {
    image = await ext.tabs.captureVisibleTab(tab.windowId, { format: "png" });
  } catch {
    return { ok: false, error: "noCapture" };
  }
  return ask({ type: "totpFromImage", image });
}

/** A 2FA key's short fingerprint, for remembering "Not now" without the key. */
async function keyPrint(secret) {
  const bare = secret.replace(/[\s-]/g, "").toUpperCase();
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(bare)));
  return Array.from(hash.slice(0, 12), (b) => b.toString(16).padStart(2, "0")).join("");
}

const DISMISSED_KEYS = "dismissedKeys";

/** Whether to offer a 2FA key a page shows: not when turned down for this
 *  site this session, nor when a login of the site has it already. */
async function keyWorth(message, sender) {
  if (!text(message.secret, 2048) || !message.secret) return refused;
  const site = siteOf(hostOf(sender.url));
  const print = await keyPrint(message.secret);
  const dismissed = (await ext.storage.session.get(DISMISSED_KEYS))[DISMISSED_KEYS] ?? [];
  if (dismissed.includes(`${site}|${print}`)) return { ok: true, offer: false };
  const known = await ask({ type: "totpKnown", url: sender.url, secret: message.secret });
  return { ok: true, offer: !(known?.ok && known.known) };
}

async function keyDismiss(message, sender) {
  if (!text(message.secret, 2048)) return refused;
  const entry = `${siteOf(hostOf(sender.url))}|${await keyPrint(message.secret)}`;
  const dismissed = (await ext.storage.session.get(DISMISSED_KEYS))[DISMISSED_KEYS] ?? [];
  if (!dismissed.includes(entry)) await ext.storage.session.set({ [DISMISSED_KEYS]: [...dismissed, entry].slice(-200) });
  return { ok: true };
}

/** Keeps a 2FA key for the tab's site: with login `id`, or as a new one. */
async function saveTotpFor(url, id, secret) {
  if (!(id === null || text(id, 100)) || !text(secret, 2048) || !secret) return refused;
  return ask({ type: "saveTotp", url, id, secret });
}

ext.tabs.onRemoved.addListener((tabId) => {
  unlockWaits.delete(tabId);
  saveWaits.delete(tabId);
  for (const [key, operation] of passkeyOperations) {
    if (operation.tabId !== tabId) continue;
    passkeyOperations.delete(key);
    void ask({ type: "passkeyCancel", url: operation.url, operationId: operation.id });
  }
  void ext.storage.session.remove([pendingKey(tabId), suggestedKey(tabId)]);
});

// --- Passkeys ---------------------------------------------------------------------
//
// Only the page itself asks (not a frame inside it), and the app gets the
// page's address from the browser; what the page sends is only checked for
// shape here, and judged in the app.

const list = (value) => Array.isArray(value) && value.length <= 64 && value.every((item) => text(item, 1400));
const passkeyOperations = new Map();
const passkeyOperationKey = (sender, requestId) =>
  `${sender.tab.id}:${sender.frameId}:${sender.documentId ?? new URL(sender.url).origin}:${requestId}`;

// A departing document may disappear before its pagehide message reaches us.
// Preserve requests already made by the newly committed document.
ext.webNavigation?.onCommitted?.addListener((navigation) => {
  for (const [key, operation] of passkeyOperations) {
    if (operation.tabId !== navigation.tabId || operation.frameId !== navigation.frameId) continue;
    if (navigation.documentId && operation.documentId === navigation.documentId) continue;
    if (Number.isFinite(navigation.timeStamp) && operation.at > navigation.timeStamp) continue;
    passkeyOperations.delete(key);
    void ask({ type: "passkeyCancel", url: operation.url, operationId: operation.id });
  }
});

function cancelPasskey(message, sender) {
  if (sender.frameId !== 0 || !text(message.requestId, 80) || !message.requestId) return refused;
  const key = passkeyOperationKey(sender, message.requestId);
  const operation = passkeyOperations.get(key);
  if (!operation) return { ok: true };
  passkeyOperations.delete(key);
  return ask({ type: "passkeyCancel", url: operation.url, operationId: operation.id });
}

async function runPasskey(request, sender, requestId) {
  if (requestId !== undefined && (!text(requestId, 80) || !requestId)) return refused;
  const key = passkeyOperationKey(sender, requestId ?? crypto.randomUUID());
  const previous = passkeyOperations.get(key);
  if (previous) return { ok: false, error: "busy" };
  const operation = {
    id: crypto.randomUUID(), url: sender.url, tabId: sender.tab.id,
    frameId: sender.frameId, documentId: sender.documentId, at: Date.now(),
  };
  passkeyOperations.set(key, operation);
  try {
    const answer = await ask({ ...request, operationId: operation.id }, PASSKEY_ANSWER_LIMIT);
    if (answer?.error === "noHost" || answer?.error === "hostExited") {
      void ask({ type: "passkeyCancel", url: operation.url, operationId: operation.id });
    }
    return answer;
  } finally {
    if (passkeyOperations.get(key) === operation) passkeyOperations.delete(key);
  }
}

// Which sites have a passkey in MYLE, remembered from the vault's answers so
// that a locked (or closed) MYLE can still be offered on those sites: kept
// as salted hashes of the site's name, never the names or the passkeys.
const PASSKEY_SITES = "passkeySites";
const PASSKEY_SALT = "passkeySalt";

async function passkeySiteKey(rpId) {
  let salt = (await ext.storage.local.get(PASSKEY_SALT))[PASSKEY_SALT];
  if (typeof salt !== "string") {
    salt = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
    await ext.storage.local.set({ [PASSKEY_SALT]: salt });
  }
  const bytes = new TextEncoder().encode(`${salt}|${rpId.toLowerCase().replace(/\.$/, "")}`);
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(hash.slice(0, 16), (b) => b.toString(16).padStart(2, "0")).join("");
}

async function passkeySites() {
  const found = (await ext.storage.local.get(PASSKEY_SITES))[PASSKEY_SITES];
  return Array.isArray(found) ? found : [];
}

async function rememberPasskeySite(rpId, has) {
  if (!text(rpId, 253) || !rpId) return;
  const key = await passkeySiteKey(rpId);
  const sites = await passkeySites();
  const listed = sites.includes(key);
  if (has && !listed) await ext.storage.local.set({ [PASSKEY_SITES]: [...sites, key].slice(-500) });
  if (!has && listed) await ext.storage.local.set({ [PASSKEY_SITES]: sites.filter((item) => item !== key) });
}

let sitesSynced = 0;

/** Learns every site the vault has passkeys for (at most every five
 *  minutes, while it is unlocked), so that a closed or locked MYLE is
 *  offered on them even before a passkey was used here. */
async function syncPasskeySites() {
  if (Date.now() - sitesSynced < 5 * 60_000) return;
  sitesSynced = Date.now();
  const answer = await ask({ type: "passkeySites" });
  // An older MYLE does not know the request: what was learned stays.
  if (!answer?.ok || !Array.isArray(answer.sites)) return;
  const keys = [];
  for (const site of answer.sites.slice(0, 500)) if (text(site, 253) && site) keys.push(await passkeySiteKey(site));
  await ext.storage.local.set({ [PASSKEY_SITES]: keys });
}

/** Whether MYLE had a passkey for `rpId` (or the page's host) last time. */
async function passkeySiteKnown(rpId, sender) {
  const site = rpId || hostOf(sender.url) && new URL(sender.url).hostname;
  return text(site, 253) && !!site && (await passkeySites()).includes(await passkeySiteKey(site));
}

async function listPasskeys(message, sender, rpId) {
  const allow = list(message.allow) ? message.allow : [];
  const answer = await ask({ type: "passkeyList", url: sender.url, rpId, allow, wake: message.wake === true });
  if (answer?.ok && Array.isArray(answer.passkeys)) {
    void syncPasskeySites();
    // Only a full list says the site has none; a short "allow" list may
    // just name another account's.
    if (answer.passkeys.length || !allow.length) await rememberPasskeySite(answer.rpId, answer.passkeys.length > 0);
    return answer;
  }
  // MYLE locked, or not running: whether it is worth asking the user to open it.
  if (answer?.error === "locked" || answer?.error === "notRunning") {
    return { ...answer, known: await passkeySiteKnown(rpId, sender) };
  }
  return answer;
}

function passkey(message, sender) {
  if (sender.frameId !== 0) return refused;
  const rpId = message.rpId ?? null;
  if (rpId !== null && !text(rpId, 253)) return refused;
  const verification = text(message.userVerification, 20) ? message.userVerification : "preferred";
  switch (message.type) {
    case "passkeyList":
      return listPasskeys(message, sender, rpId);
    case "passkeyGet":
      if (!text(message.challenge, 1400) || !text(message.credentialId, 1400)) return refused;
      return runPasskey({
        type: "passkeyGet",
        url: sender.url,
        rpId,
        challenge: message.challenge,
        credentialId: message.credentialId,
        userVerification: verification,
      }, sender, message.requestId);
    case "passkeyCreate": {
      const algorithms = Array.isArray(message.algorithms) && message.algorithms.length <= 32 &&
        message.algorithms.every(Number.isInteger) ? message.algorithms : [];
      if (!text(message.challenge, 1400) || !text(message.userId, 100) || !list(message.exclude ?? [])) return refused;
      return createPasskey({
        type: "passkeyCreate",
        url: sender.url,
        rpId,
        rpName: text(message.rpName, 200) ? message.rpName : "",
        userId: message.userId,
        userName: text(message.userName, 256) ? message.userName : "",
        userDisplayName: text(message.userDisplayName, 256) ? message.userDisplayName : "",
        challenge: message.challenge,
        algorithms,
        exclude: message.exclude ?? [],
        userVerification: verification,
      }, sender, message.requestId);
    }
    default:
      return refused;
  }
}

async function createPasskey(request, sender, requestId) {
  const answer = await runPasskey(request, sender, requestId);
  if (answer?.ok) {
    const rpId = request.rpId || hostOf(request.url) && new URL(request.url).hostname;
    await rememberPasskeySite(rpId, true);
  }
  return answer;
}

// --- Requests ---------------------------------------------------------------------

async function handle(message, sender) {
  if (typeof message?.type !== "string") return refused;
  if (fromPopup(sender)) {
    switch (message.type) {
      case "status": {
        const status = await ask({ type: "status" });
        if (status?.ok && status.state === "unlocked") void syncPasskeySites();
        return status;
      }
      case "open":
        return openApp();
      case "tabLogins":
        return loginsForTab(message.tabId);
      case "fillTab":
        return fillTab(message.tabId, message.frameId, message.id);
      case "copyTab":
        return copyTab(message.tabId, message.frameId, message.id, message.field);
      case "scanTab":
        return scanTab(message.tabId);
      case "saveTotpTab": {
        let frames;
        try {
          frames = await framesFor(message.tabId);
        } catch {
          return { ok: false, error: "pageChanged" };
        }
        const top = frames?.find((frame) => frame.frameId === 0);
        return top ? saveTotpFor(top.url, message.id ?? null, message.secret) : { ok: false, error: "pageChanged" };
      }
      default:
        return refused;
    }
  }
  if (!fromPage(sender)) return refused;
  switch (message.type) {
    case "open":
      return openApp();
    case "waitUnlocked":
      // While MYLE's prompt on the page waits for the vault to open.
      return sender.frameId === 0 || !(await embeddedIn(sender)) ? waitForUnlock(sender, message.seconds) : refused;
    case "stopWaiting":
      if (unlockWaits.has(sender.tab.id)) unlockWaits.delete(sender.tab.id);
      return { ok: true };
    case "logins":
    case "fill":
    case "totp":
    case "generate": {
      const top = await embeddedIn(sender);
      if (top) return { ok: false, error: "embedded", site: hostOf(sender.url), top };
      if (message.type === "logins") {
        const answer = await ask({ type: "logins", url: sender.url });
        if (answer?.ok) void syncPasskeySites();
        return answer;
      }
      if (message.type === "generate") return suggest(sender);
      return text(message.id, 100) ? ask({ type: message.type, id: message.id, url: sender.url }) : refused;
    }
    case "submitted":
      // A form sent to the app: ask whether the login is new, and if so offer
      // to save it on the next page (the form usually navigates).
      if (await embeddedIn(sender)) return { ok: true };
      {
        const work = submitted(message, sender);
        checking.set(sender.tab.id, work);
        void work.finally(() => checking.get(sender.tab.id) === work && checking.delete(sender.tab.id));
        return work;
      }
    case "pending":
      return sender.frameId === 0 ? offer(sender) : null;
    case "offerSettled":
      return sender.frameId === 0 && text(message.nonce, 64) ? offerSettled(message, sender) : refused;
    case "qrPixels":
      // A picture of the page, read as grey levels, for its 2FA QR code.
      if (!Number.isInteger(message.width) || !Number.isInteger(message.height) || !text(message.pixels, 2_000_000)) {
        return refused;
      }
      return ask({ type: "totpFromPixels", width: message.width, height: message.height, pixels: message.pixels });
    case "save":
    case "dismiss":
      return sender.frameId === 0 ? answerOffer(message, sender, message.type === "save") : refused;
    case "passkeyList":
    case "passkeyGet":
    case "passkeyCreate":
      return passkey(message, sender);
    case "passkeyCancel":
      return cancelPasskey(message, sender);
    case "saveTotp":
      // A key the page shows, kept at the user's click in MYLE's bar.
      return (await embeddedIn(sender)) ? refused : saveTotpFor(sender.url, message.id ?? null, message.secret);
    case "keyWorth":
      return sender.frameId === 0 ? keyWorth(message, sender) : refused;
    case "keyDismiss":
      return sender.frameId === 0 ? keyDismiss(message, sender) : refused;
    default:
      return { ok: false, error: "unknown" };
  }
}

ext.runtime.onMessage.addListener((message, sender, sendResponse) => {
  // Only this extension's own pages and scripts.
  if (sender.id !== ext.runtime.id) return false;
  handle(message, sender).then(sendResponse, () => sendResponse({ ok: false, error: "failed" }));
  return true;
});
