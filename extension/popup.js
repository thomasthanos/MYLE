// The toolbar popup: the logins saved for the site in the current tab and
// for the sign-in frames inside it. It is part of the browser, not of the
// page, so it is also the safe way to fill a form embedded from another site.
// Copy buttons ask MYLE to put the text on the clipboard itself (marked
// secret, cleared after 30 seconds): a password never comes to this page.
const ext = globalThis.browser ?? globalThis.chrome;
const content = document.getElementById("content");
const site = document.getElementById("site");
const count = document.getElementById("count");
const searchRow = document.getElementById("search-row");
const search = document.getElementById("search");
const toast = document.getElementById("toast");

const send = (message) => ext.runtime.sendMessage(message).catch(() => ({ ok: false, error: "noHost" }));

const reasons = {
  noFields: "No sign-in field is ready in this page or frame. Open the sign-in form and try again.",
  noContent: "The sign-in frame is not ready. Reload the page and try again.",
  pageChanged: "The page changed while filling. Open the sign-in form and try again.",
  ambiguousFields: "This page has several password fields. Click the one to fill in the page, then try again.",
  insecureForm: "This form would send your password over plain http, so it is not filled.",
  locked: "Your vault locked meanwhile. Unlock it in MYLE and try again.",
  busy: "Too many requests in the last minute. Wait a moment and try again.",
  wrongSite: "That login is saved for another website.",
  notFound: "That login is no longer in your vault.",
  badKey: "MYLE could not read that 2FA key.",
  noTotp: "That login has no 2FA key.",
  empty: "That login has nothing to copy there.",
  oldApp: "Update MYLE to copy from here. Filling works as before.",
};

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

let toastTimer;
/** A short note at the bottom: what a button just did. */
function note(text, good = true) {
  clearTimeout(toastTimer);
  toast.textContent = text;
  toast.classList.toggle("bad", !good);
  toast.hidden = false;
  toastTimer = setTimeout(() => (toast.hidden = true), good ? 2600 : 5000);
}

async function openApp() {
  await send({ type: "open" });
  window.close();
}

/** A message in place of the list, with a button when there is something to do. */
function state(title, text, action, run = openApp) {
  const box = document.createElement("div");
  box.className = "state";
  const heading = document.createElement("b");
  heading.textContent = title;
  const body = document.createElement("p");
  body.textContent = text;
  box.append(heading, body);
  if (action) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "primary";
    button.textContent = action;
    button.addEventListener("click", run);
    box.append(button);
  }
  searchRow.hidden = true;
  count.hidden = true;
  content.replaceChildren(box);
  box.querySelector("button")?.focus();
}

function explain(error) {
  switch (error) {
    case "locked":
      return state("Your vault is locked", "Unlock it in MYLE, then open this again.", "Unlock in MYLE");
    case "noVault":
      return state("No vault yet", "Create your password vault in MYLE first.", "Open MYLE");
    case "notRunning":
      return state("MYLE is not running", "Open it to fill in your logins.", "Open MYLE");
    case "disabled":
      return state("Browser filling is off", "Turn it on in MYLE: Password Manager → ⋯ → Browser filling.", "Open MYLE");
    case "hostMissing":
      return state("This browser cannot reach MYLE yet",
        "Open MYLE once: it connects your browsers by itself. Still nothing? Password Manager → ⋯ → Browser filling shows what is missing.",
        "Try again", retry);
    case "hostForbidden":
      return state("MYLE does not know this copy",
        "Load the extension from the folder MYLE shows in Password Manager → ⋯ → Browser filling.", "Try again", retry);
    case "hostExited":
    case "noHost":
      return state("MYLE could not answer", "Make sure MYLE is installed and up to date, then try again.", "Try again", retry);
    case "insecure":
      return state("Not a secure page", "Logins are filled only on https pages.");
    case "busy":
      return state("Busy", reasons.busy);
    default:
      return state("Something went wrong", "Reload the page and try again.", "Try again", retry);
  }
}

function feedback(text) {
  content.querySelector(".feedback")?.remove();
  const p = document.createElement("p");
  p.className = "feedback";
  p.setAttribute("role", "alert");
  p.textContent = text;
  content.prepend(p);
}

/** A site's colour, the same every time (as in the app). */
function hueOf(text) {
  let hash = 7;
  for (const char of text) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return hash % 360;
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

const copied = {
  username: "User name copied.",
  password: "Password copied. It leaves the clipboard in 30 seconds.",
  totp: "2FA code copied. It leaves the clipboard in 30 seconds.",
};

/** A small button that has MYLE copy one field of the login. */
function copyButton(login, tabId, field, label, shape) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "copy";
  button.title = label;
  button.setAttribute("aria-label", `${label}: ${login.title || login.site || "login"}`);
  button.append(icon(shape));
  button.addEventListener("click", async () => {
    button.disabled = true;
    const result = await send({ type: "copyTab", tabId, frameId: login.frameId, id: login.id, field });
    button.disabled = false;
    if (result?.ok) {
      button.replaceChildren(icon("check"));
      button.classList.add("done");
      note(copied[field]);
      setTimeout(() => {
        button.replaceChildren(icon(shape));
        button.classList.remove("done");
      }, 1600);
    } else {
      note(reasons[result?.error] ?? "MYLE could not copy that. Try again in MYLE.", false);
    }
  });
  return button;
}

function row(login, tabId) {
  const item = document.createElement("div");
  item.className = "login";
  const avatar = avatarFor(login);
  const text = document.createElement("span");
  text.className = "text";
  const title = document.createElement("b");
  title.textContent = login.title || login.site || "Login";
  const user = document.createElement("small");
  user.textContent = login.username || "No user name";
  // Saved for another host of this site, or for a frame from another site.
  const where = [login.exact ? "" : login.site, login.embedded ? `in a frame from ${login.frameSite}` : ""]
    .filter(Boolean).join(", ");
  if (where) {
    const other = document.createElement("span");
    other.className = "other";
    other.textContent = ` · ${where}`;
    user.append(other);
  }
  text.title = [title.textContent, user.textContent].join("\n");
  text.append(title, user);

  const actions = document.createElement("span");
  actions.className = "actions";
  if (login.username) actions.append(copyButton(login, tabId, "username", "Copy the user name", "user"));
  actions.append(copyButton(login, tabId, "password", "Copy the password", "key"));
  if (login.totp) actions.append(copyButton(login, tabId, "totp", "Copy the 2FA code", "code"));

  const fill = document.createElement("button");
  fill.type = "button";
  fill.className = "fill";
  fill.textContent = "Fill";
  fill.title = "Fill this login in the page (Enter)";
  fill.addEventListener("click", async () => {
    fill.disabled = true;
    fill.textContent = "Filling…";
    const result = await send({ type: "fillTab", tabId, frameId: login.frameId, id: login.id });
    if (result?.ok && result.applied) {
      window.close();
    } else {
      feedback(reasons[result?.error] ?? "This login could not be filled. Try the sign-in field in the page.");
      fill.disabled = false;
      fill.textContent = "Fill";
    }
  });
  actions.append(fill);
  item.append(avatar, text, actions);
  item.dataset.search = [login.title, login.username, login.site, login.frameSite].join(" ").toLowerCase();
  return item;
}

function filter() {
  const words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
  let shown = 0;
  for (const item of content.querySelectorAll(".login")) {
    item.hidden = !words.every((word) => item.dataset.search.includes(word));
    if (!item.hidden) shown++;
  }
  content.querySelector(".none")?.remove();
  if (!shown) {
    const none = document.createElement("p");
    none.className = "note none";
    none.textContent = "No login matches.";
    content.append(none);
  }
}

async function load() {
  const [tab] = await ext.tabs.query({ active: true, currentWindow: true });
  let topSite = "";
  try {
    topSite = new URL(tab?.url ?? "").hostname.replace(/^www\./, "");
  } catch {
    // Not a web page.
  }
  site.textContent = topSite || "No website";
  if (!Number.isInteger(tab?.id) || !tab?.url || !/^https:\/\/|^http:\/\/(?:localhost|127\.0\.0\.1)(?::\d+)?(?:\/|$)/.test(tab.url)) {
    return state("Nothing to fill here", "Open a sign-in page to fill in a login.");
  }
  const answer = await send({ type: "tabLogins", tabId: tab.id });
  if (!answer?.ok) return explain(answer?.error);
  if (!answer.logins.length) {
    return state(
      "No saved login",
      `Nothing is saved for ${topSite} yet. Sign in as usual and MYLE offers to keep it, or add it in MYLE.`,
      "Add it in MYLE",
    );
  }
  count.textContent = String(answer.logins.length);
  count.title = `${answer.logins.length} saved ${answer.logins.length === 1 ? "login" : "logins"} for this page`;
  count.hidden = false;
  content.replaceChildren(...answer.logins.map((login) => row(login, tab.id)));
  if (answer.logins.length > 3) {
    searchRow.hidden = false;
    search.focus();
  } else {
    content.querySelector(".login .fill")?.focus();
  }
}

/** The rows still shown, in order. */
const shownRows = () => [...content.querySelectorAll(".login:not([hidden])")];

search.addEventListener("input", filter);
search.addEventListener("keydown", (event) => {
  const first = shownRows()[0];
  if (event.key === "Enter" && first) first.querySelector(".fill").click();
  if (event.key === "ArrowDown" && first) {
    event.preventDefault();
    first.querySelector(".fill").focus();
  }
  if (event.key === "Escape" && search.value) {
    // The first Esc clears the search; the next closes the popup.
    event.preventDefault();
    search.value = "";
    filter();
  }
});
content.addEventListener("keydown", (event) => {
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  const rows = shownRows();
  const at = rows.findIndex((item) => item.contains(document.activeElement));
  if (at < 0) return;
  event.preventDefault();
  const next = at + (event.key === "ArrowDown" ? 1 : -1);
  if (next < 0 && !searchRow.hidden) search.focus();
  else {
    const target = rows[Math.max(0, Math.min(next, rows.length - 1))];
    (target.querySelector(".fill") ?? target.querySelector("button"))?.focus();
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key === "/" && !searchRow.hidden && document.activeElement !== search) {
    event.preventDefault();
    search.focus();
  }
});
document.getElementById("open").addEventListener("click", openApp);

// --- 2FA from a QR code on the page ---------------------------------------------

async function scanQr() {
  const [tab] = await ext.tabs.query({ active: true, currentWindow: true });
  if (!Number.isInteger(tab?.id)) return;
  searchRow.hidden = true;
  count.hidden = true;
  content.replaceChildren(Object.assign(document.createElement("p"), { className: "note", textContent: "Looking for a QR code…" }));
  const found = await send({ type: "scanTab", tabId: tab.id });
  if (!found?.ok || typeof found.link !== "string") {
    if (found?.error === "noQr") {
      return state("No 2FA QR code found", "Show the site's QR code on screen, whole and not covered, then try again.");
    }
    if (found?.error === "noCapture" || found?.error === "insecure") {
      return state("This page cannot be read", "2FA QR codes are read only on secure (https) pages.");
    }
    return explain(found?.error);
  }
  const answer = await send({ type: "tabLogins", tabId: tab.id });
  if (!answer?.ok) return explain(answer?.error);
  const info = found.info ?? {};
  const what = [info.issuer, info.account].filter(Boolean).join(" · ") || "this site";
  const intro = document.createElement("p");
  intro.className = "found";
  intro.append("A 2FA key for ", Object.assign(document.createElement("b"), { textContent: what }), ". Keep it with:");
  const rows = answer.logins.filter((login) => login.frameId === 0).map((login) => {
    const item = document.createElement("div");
    item.className = "login";
    const text = document.createElement("span");
    text.className = "text";
    text.append(
      Object.assign(document.createElement("b"), { textContent: login.title || login.site || "Login" }),
      Object.assign(document.createElement("small"), { textContent: (login.username || "No user name") + (login.totp ? " · has a key" : "") }),
    );
    const save = document.createElement("button");
    save.type = "button";
    save.textContent = "Save";
    save.addEventListener("click", () => keep(tab.id, login.id, found.link, save));
    item.append(avatarFor(login), text, save);
    return item;
  });
  const fresh = document.createElement("button");
  fresh.type = "button";
  fresh.className = "new-login";
  fresh.textContent = "Save as a new login";
  fresh.addEventListener("click", () => keep(tab.id, null, found.link, fresh));
  content.replaceChildren(intro, ...rows, fresh);
}

async function keep(tabId, id, link, button) {
  button.disabled = true;
  const saved = await send({ type: "saveTotpTab", tabId, id, secret: link });
  if (saved?.ok) {
    return state("2FA key saved", "MYLE fills this site's codes from now on: click the code field when it asks.");
  }
  button.disabled = false;
  feedback(reasons[saved?.error] ?? "That key could not be kept. Add it to the login in MYLE instead.");
}

document.getElementById("scan").addEventListener("click", () => void scanQr().catch(() => state("Could not read this tab", "Reload the page and try again.")));

function retry() {
  content.replaceChildren();
  void load().catch(() => state("Could not read this tab", "Reload the page and try again."));
}

retry();

// The version loaded in this browser, and the debug log (read by the
// extension's part in each page, which writes to that page's console).
document.getElementById("version").textContent = `MYLE Passwords ${ext.runtime.getManifest().version}`;
const debugBox = document.getElementById("debug");
void ext.storage.local.get("debug").then((found) => (debugBox.checked = found.debug === true));
debugBox.addEventListener("change", () => {
  void ext.storage.local.set({ debug: debugBox.checked });
  note(debugBox.checked ? "Debug log on: reload the page, then open its console (F12)." : "Debug log off.");
});
