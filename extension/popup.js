// The toolbar popup: whether MYLE is there and its vault unlocked, and the
// logins saved for the site in the current tab and for the sign-in frames
// inside it. It is part of the browser, not of the page, so it is also the
// safe way to fill a form embedded from another site. Copy buttons ask MYLE to
// put the text on the clipboard itself (marked secret, cleared after 30
// seconds): a password never comes to this page.
//
// One screen at a time: every flow (the list, a 2FA scan, a retry) takes a new
// view token, and a flow whose token is no longer the current one drops what
// it found, so a late list never covers a scanned 2FA key.
const ext = globalThis.browser ?? globalThis.chrome;
const byId = (id) => document.getElementById(id);
const ui = {
  content: byId("content"),
  site: byId("site"),
  chip: byId("chip"),
  chipText: byId("chip-text"),
  searchRow: byId("search-row"),
  search: byId("search"),
  count: byId("count"),
  live: byId("live"),
  toast: byId("toast"),
  scan: byId("scan"),
  open: byId("open"),
  version: byId("version"),
  appVersion: byId("app-version"),
  debug: byId("debug"),
};

let view = 0;
/** The active tab, read once. */
let tab = null;
/** MYLE's last answer to `status`, and whether an answer said it is too old. */
let status = null;
let oldApp = false;
/** The list's rows: `{ login, item, fill, search }`. */
let rows = [];
let scanning = false;
let toastTimer;

// --- Words --------------------------------------------------------------------

/** Why a fill, copy or save did not happen, in a sentence (null: no sentence). */
function reasonFor(error) {
  switch (error) {
    case "noFields": return "No sign-in field is ready in this page or frame. Open the sign-in form and try again.";
    case "noContent":
    case "reloaded": return "This page is not ready for MYLE: reload it, then try again.";
    case "pageChanged": return "The page changed while filling. Open the sign-in form and try again.";
    case "ambiguousFields": return "This page has several password fields. Click the one to fill in the page, then try again.";
    case "insecureForm": return "This form would send your password over plain http, so it is not filled.";
    case "locked": return "Your vault locked meanwhile. Unlock it in MYLE and try again.";
    case "busy": return "Too many requests in the last minute. Wait a moment and try again.";
    case "wrongSite": return "That login is saved for another website.";
    case "notFound": return "That login is no longer in your vault.";
    case "badKey": return "MYLE could not read that 2FA key.";
    case "noTotp": return "That login has no 2FA key.";
    case "empty": return "That login has nothing to copy there.";
    case "oldApp": return "Update MYLE to copy from here. Filling works as before.";
    case "vaultError": return "MYLE could not read your vault just now. Try again in a moment.";
    case "notRunning": return "MYLE is not running. Open it, then try again.";
    default: return null;
  }
}

/** What the popup says in place of the list after `error`: a title, a
 *  sentence and the actions offered (`open`, `retry` or `back`). */
function errorView(error) {
  switch (error) {
    case "locked":
      return { title: "Your vault is locked", text: "Unlock it in MYLE to see this site's logins.", actions: [["Unlock in MYLE", "open"]] };
    case "noVault":
      return { title: "No vault yet", text: "Create your password vault in MYLE first.", actions: [["Open MYLE", "open"]] };
    case "notRunning":
      return { title: "MYLE is not running", text: "Open it to fill in your logins.", actions: [["Open MYLE", "open"]] };
    case "disabled":
      return { title: "Browser filling is off", text: "Turn it on in MYLE: Password Manager → ⋯ → Browser filling.", actions: [["Open MYLE", "open"]] };
    case "hostMissing":
      return {
        title: "This browser is not connected",
        text: "MYLE is not set up for this browser, or Browser filling is turned off in MYLE (Password Manager → ⋯ → Browser filling). Opening MYLE connects your browsers by itself.",
        actions: [["Open MYLE", "open"], ["Try again", "retry"]],
      };
    case "hostForbidden":
      return {
        title: "MYLE does not know this copy",
        text: "Update MYLE to the latest version (it knows the Chrome Web Store copy), or load the extension from MYLE's folder.",
        actions: [["Open MYLE", "open"], ["Try again", "retry"]],
      };
    case "hostExited":
    case "noHost":
      return { title: "MYLE could not answer", text: "Make sure MYLE is installed and up to date, then try again.", actions: [["Try again", "retry"]] };
    case "vaultError":
      return { title: "The vault could not be read", text: reasonFor("vaultError"), actions: [["Try again", "retry"]] };
    case "insecure":
      return { title: "Not a secure page", text: "Logins are filled only on https pages, and on this computer's own sites.", actions: [] };
    case "busy":
      return { title: "Busy", text: reasonFor("busy"), actions: [["Try again", "retry"]] };
    case "reloaded":
    case "noContent":
      return { title: "Reload the page", text: "The browser updated MYLE Passwords and this page still runs the old copy. Reload it, then try again.", actions: [] };
    default:
      return { title: "Something went wrong", text: "Reload the page and try again.", actions: [["Try again", "retry"]] };
  }
}

/** The header's chip from MYLE's `status` answer: `tone` ok, warn, bad or
 *  idle, and `action` (`open`) when a click on it helps. */
function chipFor(answer, tooOld = false) {
  if (!answer) return { tone: "idle", text: "Checking…", title: "Asking MYLE…", action: null };
  if (answer.ok) {
    if (tooOld) return { tone: "warn", text: "Update MYLE", title: "This MYLE is older than the extension: update it for every feature.", action: "open" };
    if (answer.enabled === false) return { tone: "warn", text: "Filling off", title: "Browser filling is off in MYLE. Click to open MYLE.", action: "open" };
    if (answer.state === "unlocked") return { tone: "ok", text: "Connected · Unlocked", title: "MYLE is connected and your vault is unlocked", action: null };
    if (answer.state === "locked") return { tone: "warn", text: "Locked", title: "Your vault is locked. Click to unlock it in MYLE.", action: "open" };
    if (answer.state === "new") return { tone: "warn", text: "No vault", title: "Create your vault in MYLE first. Click to open MYLE.", action: "open" };
    return { tone: "ok", text: "Connected", title: "MYLE is connected", action: null };
  }
  switch (answer.error) {
    case "notRunning":
      return { tone: "idle", text: "MYLE not running", title: "Click to open MYLE", action: "open" };
    case "disabled":
      return { tone: "warn", text: "Filling off", title: "Browser filling is off in MYLE. Click to open MYLE.", action: "open" };
    case "noVault":
      return { tone: "warn", text: "No vault", title: "Create your vault in MYLE first. Click to open MYLE.", action: "open" };
    case "vaultError":
      return { tone: "bad", text: "Vault error", title: reasonFor("vaultError"), action: null };
    case "oldApp":
      return { tone: "warn", text: "Update MYLE", title: "This MYLE is older than the extension: update it for every feature.", action: "open" };
    default:
      return { tone: "bad", text: "Not connected", title: errorView(answer.error).text, action: "open" };
  }
}

/** What a copy button says once MYLE has copied. */
function copiedText(field) {
  switch (field) {
    case "username": return "User name copied · clears in 30 s";
    case "password": return "Password copied · clears in 30 s";
    default: return "2FA code copied · clears in 30 s";
  }
}

/** `web` (https, or this computer's own sites), `insecure` (plain http) or
 *  `other` (a browser page, a file…). */
function kindOf(url) {
  if (typeof url !== "string") return "other";
  if (/^https:\/\//i.test(url) || /^http:\/\/(?:localhost|127\.0\.0\.1)(?::\d+)?(?:[/?#]|$)/i.test(url)) return "web";
  return /^http:\/\//i.test(url) ? "insecure" : "other";
}

/** The site of a web page's address, without `www.` ("" for anything else). */
function siteOf(url) {
  if (kindOf(url) === "other") return "";
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

/** A site's colour, the same every time (as in the app). */
function hueOf(text) {
  let hash = 7;
  for (const char of text) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return hash % 360;
}

// --- Drawing --------------------------------------------------------------------

// Lucide's shapes (ISC licence), drawn here so no markup is parsed.
const ICONS = {
  user: [["path", { d: "M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" }], ["circle", { cx: 12, cy: 7, r: 4 }]],
  key: [
    ["path", { d: "M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z" }],
    ["circle", { cx: 16.5, cy: 7.5, r: 0.5, fill: "currentColor" }],
  ],
  code: [
    ["path", { d: "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" }],
    ["path", { d: "m9 12 2 2 4-4" }],
  ],
  check: [["path", { d: "M20 6 9 17l-5-5" }]],
  back: [["path", { d: "m12 19-7-7 7-7" }], ["path", { d: "M19 12H5" }]],
  lock: [["rect", { width: 18, height: 11, x: 3, y: 11, rx: 2, ry: 2 }], ["path", { d: "M7 11V7a5 5 0 0 1 10 0v4" }]],
  globe: [["circle", { cx: 12, cy: 12, r: 10 }], ["path", { d: "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" }], ["path", { d: "M2 12h20" }]],
  alert: [["circle", { cx: 12, cy: 12, r: 10 }], ["path", { d: "M12 8v4" }], ["path", { d: "M12 16h.01" }]],
};

function icon(name) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  for (const [tag, attrs] of ICONS[name]) {
    const shape = document.createElementNS(NS, tag);
    for (const [key, value] of Object.entries(attrs)) shape.setAttribute(key, String(value));
    svg.append(shape);
  }
  return svg;
}

/** A button: `kind` primary, secondary, ghost or danger. */
function button(text, kind = "secondary", onClick = null) {
  const node = document.createElement("button");
  node.type = "button";
  node.className = `btn ${kind}`;
  node.textContent = text;
  if (onClick) node.addEventListener("click", () => void onClick(node));
  return node;
}

/** In-flight state that keeps the button focusable (no `disabled`). */
function setBusy(node, busy) {
  if (busy) node.setAttribute("aria-busy", "true");
  else node.removeAttribute("aria-busy");
  node.classList.toggle("pending", busy);
}

function isBusy(node) {
  return node.getAttribute("aria-busy") === "true";
}

/** A short note at the bottom: what a button just did. */
function note(text, good = true) {
  clearTimeout(toastTimer);
  ui.toast.textContent = text;
  ui.toast.classList.toggle("bad", !good);
  ui.toast.hidden = false;
  toastTimer = setTimeout(() => (ui.toast.hidden = true), good ? 2600 : 5000);
}

/** Tells a screen reader what the popup shows now. */
function announce(text) {
  ui.live.textContent = text;
}

function renderChip() {
  const chip = chipFor(status, oldApp);
  ui.chip.className = `chip ${chip.tone}${chip.action ? " action" : ""}`;
  ui.chipText.textContent = chip.text;
  ui.chip.title = chip.title;
  ui.chip.setAttribute("aria-label", `${chip.text}. ${chip.title}`);
  ui.chip.setAttribute("aria-disabled", chip.action ? "false" : "true");
  ui.chip.hidden = false;
  const version = status?.ok && typeof status.appVersion === "string" ? status.appVersion : "";
  ui.appVersion.textContent = version ? `MYLE ${version}` : "";
  ui.appVersion.hidden = !version;
}

/** What an answer says about MYLE, kept for the chip. */
function learn(answer) {
  if (!answer || answer.ok) return;
  if (answer.error === "oldApp") oldApp = true;
  else if (answer.error === "locked") status = { ok: true, enabled: true, state: "locked", appVersion: status?.appVersion };
  else if (answer.error === "noVault") status = { ok: true, enabled: true, state: "new", appVersion: status?.appVersion };
  else if (["notRunning", "disabled", "hostMissing", "hostForbidden", "hostExited", "noHost", "vaultError"].includes(answer.error)) status = answer;
  else return;
  renderChip();
}

async function refreshStatus() {
  status = await send({ type: "status" });
  renderChip();
}

function hideSearch() {
  ui.searchRow.hidden = true;
  ui.count.hidden = true;
}

function setCount(shown, total) {
  ui.count.textContent = shown === total ? String(total) : `${shown}/${total}`;
  ui.count.title = `${total} saved ${total === 1 ? "login" : "logins"} for this page${shown === total ? "" : `, ${shown} shown`}`;
  ui.count.hidden = false;
}

function showLoading(text = "Loading logins…") {
  rows = [];
  hideSearch();
  const skeleton = document.createElement("div");
  skeleton.className = "skeleton";
  skeleton.setAttribute("aria-hidden", "true");
  skeleton.append(document.createElement("i"), document.createElement("i"), document.createElement("i"));
  ui.content.replaceChildren(skeleton);
  announce(text);
}

/** A message in place of the list, with its actions. */
function showState(token, { title, text, actions = [] }, tone = "") {
  if (token !== view) return;
  rows = [];
  hideSearch();
  const box = document.createElement("div");
  box.className = `state ${tone}`.trim();
  const mark = document.createElement("span");
  mark.className = "state-icon";
  mark.append(icon(tone === "ok" ? "check" : title.includes("locked") ? "lock" : title.startsWith("Not a") ? "globe" : "alert"));
  const heading = document.createElement("b");
  heading.textContent = title;
  const body = document.createElement("p");
  body.textContent = text;
  box.append(mark, heading, body);
  if (actions.length) {
    const row = document.createElement("div");
    row.className = "state-actions";
    actions.forEach(([label, kind], index) => {
      row.append(button(label, index === 0 ? "primary" : "secondary", (node) => act(kind, node)));
    });
    box.append(row);
  }
  ui.content.replaceChildren(box);
  announce(`${title}. ${text}`);
  box.querySelector?.("button")?.focus();
}

/** Runs a state's action. */
function act(kind, node) {
  if (kind === "open") return openApp(node);
  return load();
}

/** A message at the top of the list, for a fill or save that did not happen. */
function showFeedback(text) {
  for (const old of [...ui.content.children]) if (old.classList.contains("feedback")) old.remove();
  const p = document.createElement("p");
  p.className = "feedback";
  p.setAttribute("role", "alert");
  p.textContent = text;
  ui.content.prepend(p);
}

/** The website's icon when MYLE has one, else the first letter on the site's colour. */
function avatarFor(login) {
  const avatar = document.createElement("span");
  avatar.className = "avatar";
  const title = login.title || login.site || "?";
  avatar.style.setProperty("--hue", String(hueOf(login.site || title.toLowerCase())));
  const letter = (title.match(/[\p{L}\p{N}]/u)?.[0] ?? "?").toUpperCase();
  if (typeof login.icon === "string" && login.icon.startsWith("data:image/")) {
    const img = document.createElement("img");
    img.alt = "";
    img.addEventListener("error", () => {
      avatar.classList.remove("image");
      avatar.replaceChildren(letter);
    });
    img.src = login.icon;
    avatar.classList.add("image");
    avatar.append(img);
  } else {
    avatar.textContent = letter;
  }
  return avatar;
}

const COPY = {
  username: { label: "Copy the user name", shape: "user" },
  password: { label: "Copy the password", shape: "key" },
  totp: { label: "Copy the 2FA code", shape: "code" },
};

/** A small button that has MYLE copy one field of the login. One restore
 *  timer per button, cleared on every click, so a failure never keeps an
 *  earlier success's check mark. */
function copyButton(login, tabId, field) {
  const { label, shape } = COPY[field];
  const node = document.createElement("button");
  node.type = "button";
  node.className = "copy";
  node.title = label;
  node.setAttribute("aria-label", `${label}: ${login.title || login.site || "login"}`);
  node.append(icon(shape));
  let restore = null;
  const rest = () => {
    clearTimeout(restore);
    restore = null;
    node.classList.remove("done");
    node.replaceChildren(icon(shape));
  };
  node.addEventListener("click", async () => {
    if (isBusy(node)) return;
    rest();
    setBusy(node, true);
    const result = await send({ type: "copyTab", tabId, frameId: login.frameId, id: login.id, field });
    setBusy(node, false);
    if (result?.ok) {
      node.replaceChildren(icon("check"));
      node.classList.add("done");
      note(copiedText(field));
      restore = setTimeout(rest, 1600);
    } else {
      learn(result);
      note(reasonFor(result?.error) ?? "MYLE could not copy that. Try again in MYLE.", false);
    }
  });
  return node;
}

function rowFor(login, tabId) {
  const item = document.createElement("div");
  item.className = "login";
  item.setAttribute("role", "listitem");
  const name = login.title || login.site || "Login";
  const text = document.createElement("span");
  text.className = "text";
  const title = document.createElement("b");
  title.textContent = name;
  const user = document.createElement("small");
  user.textContent = login.username || "No user name";
  // Saved for another host of this site, or for a frame from another site.
  const where = [login.exact ? "" : login.site, login.embedded ? `in a frame from ${login.frameSite}` : ""]
    .filter(Boolean).join(", ");
  if (where) {
    const other = document.createElement("span");
    other.className = login.embedded ? "other embedded" : "other";
    other.textContent = ` · ${where}`;
    user.append(other);
  }
  text.title = [name, login.username || "No user name", where].filter(Boolean).join("\n");
  text.append(title, user);

  const actions = document.createElement("span");
  actions.className = "actions";
  const hasPassword = login.hasPassword !== false;
  if (login.username) actions.append(copyButton(login, tabId, "username"));
  if (hasPassword) actions.append(copyButton(login, tabId, "password"));
  if (login.totp) actions.append(copyButton(login, tabId, "totp"));

  let fill = null;
  if (hasPassword || login.username) {
    fill = button("Fill", "primary fill");
    fill.title = hasPassword ? "Fill this login in the page (Enter)" : "Fill the user name in the page";
    fill.setAttribute("aria-label", `${hasPassword ? "Fill" : "Fill the user name of"} ${name}`);
    fill.addEventListener("click", async () => {
      if (isBusy(fill)) return;
      setBusy(fill, true);
      fill.textContent = "Filling…";
      const result = await send({ type: "fillTab", tabId, frameId: login.frameId, id: login.id });
      if (result?.ok && result.applied) {
        window.close();
        return;
      }
      setBusy(fill, false);
      fill.textContent = "Fill";
      learn(result);
      showFeedback(reasonFor(result?.error) ?? "This login could not be filled. Try the sign-in field in the page.");
      fill.focus();
    });
    actions.append(fill);
  }
  item.append(avatarFor(login), text, actions);
  const search = [login.title, login.username, login.site, login.frameSite].filter(Boolean).join(" ").toLowerCase();
  return { login, item, fill, search };
}

function showList(token, logins, current) {
  if (token !== view) return;
  rows = logins.map((login) => rowFor(login, current.id));
  const list = document.createElement("div");
  list.className = "list";
  list.setAttribute("role", "list");
  list.setAttribute("aria-label", `Logins for ${siteOf(current.url)}`);
  list.append(...rows.map((row) => row.item));
  ui.content.replaceChildren(list);
  ui.search.value = "";
  setCount(rows.length, rows.length);
  const searching = rows.length >= 2;
  ui.searchRow.hidden = !searching;
  announce(`${rows.length} ${rows.length === 1 ? "login" : "logins"} for ${siteOf(current.url)}`);
  if (searching) ui.search.focus();
  else (rows[0].fill ?? rows[0].item.querySelector?.("button"))?.focus();
}

/** Shows the rows that match every word typed; answers how many are shown. */
function filter() {
  const words = ui.search.value.toLowerCase().split(/\s+/).filter(Boolean);
  let shown = 0;
  for (const row of rows) {
    row.item.hidden = !words.every((word) => row.search.includes(word));
    if (!row.item.hidden) shown++;
  }
  if (rows.length) setCount(shown, rows.length);
  for (const old of [...ui.content.children]) if (old.classList.contains("none")) old.remove();
  if (rows.length && !shown) {
    const none = document.createElement("p");
    none.className = "note none";
    none.textContent = "No login matches.";
    ui.content.append(none);
  }
  return shown;
}

/** The rows still shown, in order. */
function shownRows() {
  return rows.filter((row) => !row.item.hidden);
}

/** The control a row is reached by: its Fill, else its first button. */
function focusRow(row) {
  (row.fill ?? row.item.querySelector?.("button"))?.focus();
}

// --- Flows ----------------------------------------------------------------------

function send(message) {
  try {
    return Promise.resolve(ext.runtime.sendMessage(message)).then(
      (reply) => reply ?? { ok: false, error: "noHost" },
      () => ({ ok: false, error: "noHost" }),
    );
  } catch {
    return Promise.resolve({ ok: false, error: "noHost" });
  }
}

async function activeTab() {
  if (!tab) [tab] = await ext.tabs.query({ active: true, currentWindow: true });
  return tab;
}

/** The logins of the current tab, or what stands in their way. */
async function load() {
  const token = ++view;
  showLoading();
  try {
    const current = await activeTab();
    if (token !== view) return;
    const kind = kindOf(current?.url);
    ui.site.textContent = siteOf(current?.url) || "Not a web page";
    if (!Number.isInteger(current?.id) || kind === "other") {
      return showState(token, { title: "Not a web page", text: "Open a website's sign-in page to fill in a login.", actions: [] });
    }
    if (kind === "insecure") return showState(token, errorView("insecure"));
    const answer = await send({ type: "tabLogins", tabId: current.id });
    if (token !== view) return;
    if (!answer?.ok) {
      learn(answer);
      return showState(token, errorView(answer?.error));
    }
    if (!answer.logins.length) {
      return showState(token, {
        title: "No saved login",
        text: `Nothing is saved for ${siteOf(current.url)} yet. Sign in as usual and MYLE offers to keep it, or add it in MYLE.`,
        actions: [["Add it in MYLE", "open"]],
      });
    }
    showList(token, answer.logins, current);
  } catch {
    showState(token, { title: "Could not read this tab", text: "Reload the page and try again.", actions: [["Try again", "retry"]] });
  }
}

/** Brings MYLE forward (its unlock prompt when locked). The popup closes
 *  only when MYLE answered; otherwise it says why. */
async function openApp(node = null) {
  if (node && isBusy(node)) return false;
  if (node) setBusy(node, true);
  const reply = await send({ type: "open" });
  if (node) setBusy(node, false);
  if (reply?.ok) {
    window.close();
    return true;
  }
  learn(reply);
  note(errorView(reply?.error).text, false);
  return false;
}

/** Reads the site's 2FA QR code from the screen, then offers where to keep it. */
async function scanQr() {
  if (scanning) return;
  scanning = true;
  setBusy(ui.scan, true);
  const token = ++view;
  showLoading("Looking for a QR code…");
  try {
    const current = await activeTab();
    if (token !== view) return;
    const back = [["Back", "back"]];
    if (!Number.isInteger(current?.id) || kindOf(current?.url) !== "web") {
      return showState(token, { title: "This page cannot be read", text: "2FA QR codes are read only on secure (https) pages.", actions: back });
    }
    const found = await send({ type: "scanTab", tabId: current.id });
    if (token !== view) return;
    if (!found?.ok || typeof found.link !== "string") {
      if (found?.error === "noQr") {
        return showState(token, { title: "No 2FA QR code found", text: "Show the site's QR code on screen, whole and not covered, then try again.", actions: back });
      }
      if (found?.error === "noCapture" || found?.error === "insecure") {
        return showState(token, { title: "This page cannot be read", text: "2FA QR codes are read only on secure (https) pages.", actions: back });
      }
      learn(found);
      const why = errorView(found?.error);
      return showState(token, { ...why, actions: [...why.actions.filter(([, kind]) => kind === "open"), ...back] });
    }
    const answer = await send({ type: "tabLogins", tabId: current.id });
    if (token !== view) return;
    showScanResult(token, current.id, found, answer?.ok ? answer.logins.filter((login) => login.frameId === 0) : []);
  } catch {
    showState(token, { title: "Could not read this tab", text: "Reload the page and try again.", actions: [["Back", "back"]] });
  } finally {
    scanning = false;
    setBusy(ui.scan, false);
  }
}

/** Where to keep a scanned key: a new login (the default), or a saved one.
 *  The key itself stays in this function's closure, never in the page. */
function showScanResult(token, tabId, found, logins) {
  if (token !== view) return;
  rows = [];
  hideSearch();
  const info = found.info ?? {};
  const what = [info.issuer, info.account].filter(Boolean).join(" · ") || "this site";
  const box = document.createElement("div");
  box.className = "scan";
  const intro = document.createElement("p");
  intro.className = "found";
  const name = document.createElement("b");
  name.textContent = what;
  intro.append("Found a 2FA key for ", name, ".");
  const fresh = button("Save as a new login", "primary wide", (node) => keep(token, tabId, null, found.link, node));
  box.append(intro, fresh);
  if (logins.length) {
    const label = document.createElement("p");
    label.className = "note";
    label.textContent = "Or keep it with a saved login:";
    const list = document.createElement("div");
    list.className = "list picks";
    list.setAttribute("role", "list");
    for (const login of logins) {
      const item = document.createElement("div");
      item.className = "pick";
      item.setAttribute("role", "listitem");
      const text = document.createElement("span");
      text.className = "text";
      const title = document.createElement("b");
      title.textContent = login.title || login.site || "Login";
      const user = document.createElement("small");
      user.textContent = (login.username || "No user name") + (login.totp ? " · has a key" : "");
      text.append(title, user);
      const save = button(login.totp ? "Replace key…" : "Save", "secondary", (node) => keep(token, tabId, login, found.link, node));
      save.setAttribute("aria-label", `${login.totp ? "Replace the 2FA key of" : "Keep the key with"} ${title.textContent}`);
      item.append(avatarFor(login), text, save);
      list.append(item);
    }
    box.append(label, list);
  }
  const back = button("Back", "ghost", () => load());
  back.prepend(icon("back"));
  box.append(back);
  ui.content.replaceChildren(box);
  announce(`Found a 2FA key for ${what}.`);
  fresh.focus();
}

/** Keeps the scanned key. A login that already has a key is replaced only
 *  after a second, explicit click: the old key is gone after that. */
async function keep(token, tabId, login, link, node) {
  if (isBusy(node) || token !== view) return;
  if (login?.totp && node.dataset.confirm !== "yes") {
    node.dataset.confirm = "yes";
    node.textContent = "Replace the key";
    node.className = "btn danger";
    note(`${login.title || login.site || "This login"} already has a 2FA key: replacing it removes the old one.`, false);
    return;
  }
  setBusy(node, true);
  const saved = await send({ type: "saveTotpTab", tabId, id: login?.id ?? null, secret: link });
  setBusy(node, false);
  if (token !== view) return;
  if (saved?.ok) {
    return showState(token, {
      title: saved.replaced ? "Key replaced" : "2FA key saved",
      text: "MYLE fills this site's codes from now on: click the code field when it asks.",
      actions: [["Back", "back"]],
    }, "ok");
  }
  learn(saved);
  showFeedback(reasonFor(saved?.error) ?? "That key could not be kept. Add it to the login in MYLE instead.");
}

// --- Keys -----------------------------------------------------------------------

function onSearchKey(event) {
  const first = shownRows()[0];
  if (event.key === "Enter" && first?.fill) {
    event.preventDefault();
    first.fill.click();
  } else if (event.key === "ArrowDown" && first) {
    event.preventDefault();
    focusRow(first);
  } else if (event.key === "Escape" && ui.search.value) {
    // The first Esc clears the search; the next closes the popup.
    event.preventDefault();
    ui.search.value = "";
    filter();
  }
}

function onListKey(event) {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  const shown = shownRows();
  const at = shown.findIndex((row) => row.item.contains(document.activeElement));
  if (at < 0) return;
  event.preventDefault();
  if (event.key === "Home") return focusRow(shown[0]);
  if (event.key === "End") return focusRow(shown[shown.length - 1]);
  const next = at + (event.key === "ArrowDown" ? 1 : -1);
  if (next < 0 && !ui.searchRow.hidden) return ui.search.focus();
  focusRow(shown[Math.max(0, Math.min(next, shown.length - 1))]);
}

function onPageKey(event) {
  if (event.key === "/" && !ui.searchRow.hidden && document.activeElement !== ui.search) {
    event.preventDefault();
    ui.search.focus();
  }
}

function start() {
  ui.search.addEventListener("input", filter);
  ui.search.addEventListener("keydown", onSearchKey);
  ui.content.addEventListener("keydown", onListKey);
  document.addEventListener("keydown", onPageKey);
  ui.open.addEventListener("click", () => void openApp(ui.open));
  ui.scan.addEventListener("click", () => void scanQr());
  ui.chip.addEventListener("click", () => {
    if (chipFor(status, oldApp).action) void openApp(ui.chip);
  });
  renderChip();
  // MYLE's state and the tab's logins are asked for side by side.
  void refreshStatus();
  void load();

  // The version loaded in this browser, and the debug log (read by the
  // extension's part in each page, which writes to that page's console).
  ui.version.textContent = `MYLE Passwords ${ext.runtime.getManifest().version}`;
  void Promise.resolve(ext.storage.local.get("debug")).then((found) => (ui.debug.checked = found?.debug === true), () => {});
  ui.debug.addEventListener("change", () => {
    void ext.storage.local.set({ debug: ui.debug.checked });
    note(ui.debug.checked ? "Debug log on: reload the page, then open its console (F12)." : "Debug log off.");
  });
}

start();
