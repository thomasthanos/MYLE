import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";

// The real popup.js, run against a small DOM of its own: elements, text,
// attributes, classes, events (with bubbling) and focus, nothing more.
const source = readFileSync(new URL("../../extension/popup.js", import.meta.url), "utf8");
const html = readFileSync(new URL("../../extension/popup.html", import.meta.url), "utf8");
const css = readFileSync(new URL("../../extension/popup.css", import.meta.url), "utf8");

class Node {
  constructor(tag, document) {
    this.tagName = tag.toUpperCase();
    this.ownerDocument = document;
    this.childNodes = [];
    this.parent = null;
    this.attributes = new Map();
    this.listeners = new Map();
    this.classes = new Set();
    this.dataset = {};
    this.style = { props: {}, setProperty(key, value) { this.props[key] = value; } };
    this.hidden = false;
    this.value = "";
    this.checked = false;
    const node = this;
    this.classList = {
      add: (...names) => names.forEach((name) => node.classes.add(name)),
      remove: (...names) => names.forEach((name) => node.classes.delete(name)),
      contains: (name) => node.classes.has(name),
      toggle: (name, force = !node.classes.has(name)) => (force ? node.classes.add(name) : node.classes.delete(name), force),
    };
  }
  get className() { return [...this.classes].join(" "); }
  set className(value) { this.classes = new Set(String(value).split(/\s+/).filter(Boolean)); }
  get children() { return this.childNodes.filter((child) => typeof child !== "string"); }
  get textContent() { return this.childNodes.map((child) => (typeof child === "string" ? child : child.textContent)).join(""); }
  set textContent(value) { this.replaceChildren(String(value)); }
  append(...nodes) {
    for (const node of nodes) {
      if (node === null || node === undefined) continue;
      if (typeof node === "string") this.childNodes.push(node);
      else {
        node.remove();
        node.parent = this;
        this.childNodes.push(node);
      }
    }
  }
  prepend(...nodes) {
    const rest = this.childNodes;
    this.childNodes = [];
    this.append(...nodes);
    this.childNodes.push(...rest);
  }
  replaceChildren(...nodes) {
    for (const child of this.children) child.parent = null;
    this.childNodes = [];
    this.append(...nodes);
  }
  remove() {
    if (!this.parent) return;
    this.parent.childNodes = this.parent.childNodes.filter((child) => child !== this);
    this.parent = null;
  }
  setAttribute(key, value) { this.attributes.set(key, String(value)); }
  getAttribute(key) { return this.attributes.has(key) ? this.attributes.get(key) : null; }
  removeAttribute(key) { this.attributes.delete(key); }
  addEventListener(type, listener) {
    if (!this.listeners.has(type)) this.listeners.set(type, []);
    this.listeners.get(type).push(listener);
  }
  dispatch(type, init = {}) {
    const event = { type, target: this, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...init };
    for (let node = this; node; node = node.parent) {
      for (const listener of node.listeners.get(type) ?? []) listener(event);
    }
    for (const listener of this.ownerDocument.listeners.get(type) ?? []) listener(event);
    return event;
  }
  click() { return this.dispatch("click"); }
  focus() { this.ownerDocument.activeElement = this; }
  contains(node) {
    for (let at = node; at; at = at.parent) if (at === this) return true;
    return false;
  }
  descendants() {
    return this.children.flatMap((child) => [child, ...child.descendants()]);
  }
  querySelector(selector) {
    return this.descendants().find((node) => node.tagName === selector.toUpperCase()) ?? null;
  }
  /** Test helper: the buttons inside, by their text or label. */
  buttons() {
    return this.descendants().filter((node) => node.tagName === "BUTTON");
  }
}

function fakeDocument() {
  const document = {
    activeElement: null,
    listeners: new Map(),
    ids: new Map(),
    createElement: (tag) => new Node(tag, document),
    createElementNS: (_ns, tag) => new Node(tag, document),
    getElementById: (id) => document.ids.get(id) ?? null,
    addEventListener(type, listener) {
      if (!document.listeners.has(type)) document.listeners.set(type, []);
      document.listeners.get(type).push(listener);
    },
  };
  const element = (id, tag, hidden = false) => {
    const node = new Node(tag, document);
    node.hidden = hidden;
    document.ids.set(id, node);
    return node;
  };
  element("content", "main");
  element("site", "span");
  const chip = element("chip", "button", true);
  chip.append(element("chip-text", "span"));
  const searchRow = element("search-row", "div", true);
  searchRow.append(element("search", "input"), element("count", "span", true));
  element("live", "p");
  element("toast", "p", true);
  element("scan", "button");
  element("open", "button");
  element("version", "span");
  element("app-version", "span", true);
  element("debug", "input");
  return document;
}

function deferred() {
  let resolve;
  const promise = new Promise((done) => (resolve = done));
  return { promise, resolve };
}

const settle = async () => {
  for (let i = 0; i < 6; i++) await new Promise((done) => setImmediate(done));
};

/** Opens the popup on `url`; `answer(message)` answers what it sends. */
function popup(url, answer = () => ({ ok: true, enabled: true, state: "unlocked" })) {
  const document = fakeDocument();
  const sent = [];
  const timers = new Map();
  let nextTimer = 1;
  const window = { closed: false, close() { this.closed = true; } };
  const chrome = {
    runtime: {
      sendMessage: (message) => {
        sent.push(message);
        return Promise.resolve(answer(message));
      },
      getManifest: () => ({ version: "1.7.0" }),
    },
    tabs: { query: async () => [{ id: 7, url }] },
    storage: { local: { get: async () => ({}), set: async () => {} } },
  };
  const context = vm.createContext({
    chrome,
    document,
    window,
    URL,
    console,
    setTimeout: (run, ms) => {
      const id = nextTimer++;
      timers.set(id, { run, ms });
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  });
  vm.runInContext(source, context);
  const id = (name) => document.getElementById(name);
  const text = () => id("content").textContent;
  const runTimers = () => {
    for (const [key, timer] of [...timers]) {
      timers.delete(key);
      timer.run();
    }
  };
  return { context, document, window, sent, timers, id, text, runTimers };
}

const login = (extra = {}) => ({
  id: "a", title: "Example", username: "ana@example.com", site: "example.com", exact: true,
  totp: false, frameId: 0, frameSite: "example.com", embedded: false, ...extra,
});

test("MYLE's errors are told in words that say what to do", () => {
  const { context } = popup("chrome://newtab/");
  assert.match(context.errorView("hostMissing").text, /Browser filling is turned off/);
  assert.deepEqual([...context.errorView("hostMissing").actions.map(([label]) => label)], ["Open MYLE", "Try again"]);
  assert.match(context.errorView("hostForbidden").text, /Update MYLE to the latest version/);
  assert.equal(context.errorView("reloaded").title, "Reload the page");
  assert.equal(context.errorView("vaultError").text, "MYLE could not read your vault just now. Try again in a moment.");
  assert.match(context.reasonFor("noContent"), /reload it/);
});

test("the chip says whether MYLE is connected and the vault unlocked", () => {
  const { context } = popup("chrome://newtab/");
  const chip = (answer, old) => {
    const { tone, text, action } = context.chipFor(answer, old);
    return [tone, text, action];
  };
  assert.deepEqual(chip({ ok: true, enabled: true, state: "unlocked" }), ["ok", "Connected · Unlocked", null]);
  assert.deepEqual(chip({ ok: true, enabled: true, state: "locked" }), ["warn", "Locked", "open"]);
  assert.deepEqual(chip({ ok: true, enabled: true, state: "new" }), ["warn", "No vault", "open"]);
  assert.deepEqual(chip({ ok: true, enabled: false, state: "unlocked" }), ["warn", "Filling off", "open"]);
  assert.deepEqual(chip({ ok: false, error: "notRunning" }), ["idle", "MYLE not running", "open"]);
  assert.deepEqual(chip({ ok: false, error: "hostMissing" }), ["bad", "Not connected", "open"]);
  assert.deepEqual(chip({ ok: true, enabled: true, state: "unlocked" }, true), ["warn", "Update MYLE", "open"]);
});

test("a browser page shows the chip and asks MYLE for nothing else", async () => {
  const opened = popup("chrome://newtab/");
  await settle();
  assert.deepEqual(opened.sent.map((message) => message.type), ["status"]);
  assert.match(opened.text(), /Not a web page/);
  assert.equal(opened.id("site").textContent, "Not a web page");
  assert.equal(opened.id("chip").hidden, false);
  assert.equal(opened.id("chip-text").textContent, "Connected · Unlocked");
});

test("a plain http page is not a secure page and its logins are not asked for", async () => {
  const opened = popup("http://example.com/login");
  await settle();
  assert.deepEqual(opened.sent.map((message) => message.type), ["status"]);
  assert.match(opened.text(), /Not a secure page/);
});

test("the status and the logins are asked for side by side", async () => {
  const opened = popup("https://example.com/login", (message) =>
    message.type === "tabLogins" ? { ok: true, logins: [login(), login({ id: "b", title: "Work" })] } : { ok: true, enabled: true, state: "unlocked", appVersion: "9.18.0" });
  await settle();
  assert.deepEqual(opened.sent.map((message) => message.type).sort(), ["status", "tabLogins"]);
  assert.equal(opened.id("search-row").hidden, false, "two logins get a search box");
  assert.equal(opened.id("count").textContent, "2");
  assert.equal(opened.id("app-version").textContent, "MYLE 9.18.0");
  assert.equal(opened.id("live").textContent, "2 logins for example.com");
  // Typing narrows the list; the count says how many are shown.
  opened.id("search").value = "work";
  opened.id("search").dispatch("input");
  assert.equal(opened.id("count").textContent, "1/2");
});

test("a late list never covers a scanned 2FA key", async () => {
  const first = deferred();
  let lists = 0;
  const opened = popup("https://example.com/login", (message) => {
    if (message.type === "tabLogins") return ++lists === 1 ? first.promise : { ok: true, logins: [login()] };
    if (message.type === "scanTab") return { ok: true, link: "otpauth://totp/Example:ana?secret=JBSWY3DPEHPK3PXP", info: { issuer: "Example", account: "ana" } };
    return { ok: true, enabled: true, state: "unlocked" };
  });
  await settle();
  opened.id("scan").click();
  await settle();
  assert.match(opened.text(), /Found a 2FA key for Example · ana/);
  // The first list answers only now: it is dropped.
  first.resolve({ ok: true, logins: [login(), login({ id: "b" }), login({ id: "c" }), login({ id: "d" })] });
  await settle();
  assert.match(opened.text(), /Found a 2FA key for Example · ana/);
  assert.equal(opened.id("search-row").hidden, true);
  assert.doesNotThrow(() => opened.id("search").dispatch("input"));
  assert.doesNotThrow(() => opened.id("search").dispatch("keydown", { key: "Enter" }));
  assert.ok(!opened.text().includes("JBSWY3DPEHPK3PXP"), "the key is never in the page");
});

test("a copy that fails right after one that worked shows no check mark", async () => {
  let copies = 0;
  const opened = popup("https://example.com/login", (message) => {
    if (message.type === "tabLogins") return { ok: true, logins: [login()] };
    if (message.type === "copyTab") return ++copies === 1 ? { ok: true } : { ok: false, error: "locked" };
    return { ok: true, enabled: true, state: "unlocked" };
  });
  await settle();
  const copy = opened.id("content").buttons().find((node) => node.getAttribute("aria-label")?.startsWith("Copy the password"));
  copy.click();
  await settle();
  assert.ok(copy.classList.contains("done"));
  assert.equal(opened.id("toast").textContent, "Password copied · clears in 30 s");
  copy.click();
  await settle();
  assert.ok(!copy.classList.contains("done"), "the failure clears the earlier check at once");
  assert.equal(copy.getAttribute("aria-busy"), null);
  assert.match(opened.id("toast").textContent, /Your vault locked meanwhile/);
  assert.equal(opened.id("chip-text").textContent, "Locked", "the chip learns the vault locked");
  opened.runTimers();
  assert.ok(!copy.classList.contains("done"));
});

test("a login without a password offers no Copy password", async () => {
  const opened = popup("https://example.com/login", (message) =>
    message.type === "tabLogins" ? { ok: true, logins: [login({ hasPassword: false, totp: true })] } : { ok: true, enabled: true, state: "unlocked" });
  await settle();
  const labels = opened.id("content").buttons().map((node) => node.getAttribute("aria-label"));
  assert.deepEqual(labels, ["Copy the user name: Example", "Copy the 2FA code: Example", "Fill the user name of Example"]);
});

test("each Fill names its login, and the list is not one live region", async () => {
  const opened = popup("https://example.com/login", (message) =>
    message.type === "tabLogins" ? { ok: true, logins: [login()] } : { ok: true, enabled: true, state: "unlocked" });
  await settle();
  const fill = opened.id("content").buttons().find((node) => node.textContent === "Fill");
  assert.equal(fill.getAttribute("aria-label"), "Fill Example");
  assert.equal(opened.document.activeElement, fill, "one login: its Fill has the focus");
  assert.doesNotMatch(html, /id="content"[^>]*aria-live/);
  assert.match(html, /id="live" role="status"/);
});

test("replacing a login's 2FA key takes a second, explicit click", async () => {
  const opened = popup("https://example.com/login", (message) => {
    if (message.type === "tabLogins") return { ok: true, logins: [login({ totp: true })] };
    if (message.type === "scanTab") return { ok: true, link: "otpauth://totp/x?secret=ABC", info: {} };
    if (message.type === "saveTotpTab") return { ok: true, replaced: true };
    return { ok: true, enabled: true, state: "unlocked" };
  });
  await settle();
  opened.id("scan").click();
  await settle();
  const buttons = opened.id("content").buttons();
  assert.equal(buttons[0].textContent, "Save as a new login", "a new login comes first");
  const replace = buttons.find((node) => node.textContent === "Replace key…");
  replace.click();
  await settle();
  assert.ok(!opened.sent.some((message) => message.type === "saveTotpTab"), "the first click saves nothing");
  assert.equal(replace.textContent, "Replace the key");
  assert.ok(replace.classList.contains("danger"));
  replace.click();
  await settle();
  const saved = opened.sent.find((message) => message.type === "saveTotpTab");
  assert.deepEqual({ id: saved.id, secret: saved.secret }, { id: "a", secret: "otpauth://totp/x?secret=ABC" });
  assert.match(opened.text(), /Key replaced/);
});

test("Open MYLE closes the popup only when MYLE answered", async () => {
  let reply = { ok: false, error: "hostMissing" };
  const opened = popup("chrome://newtab/", (message) => (message.type === "open" ? reply : { ok: false, error: "notRunning" }));
  await settle();
  opened.id("open").click();
  await settle();
  assert.equal(opened.window.closed, false);
  assert.match(opened.id("toast").textContent, /Browser filling is turned off/);
  reply = { ok: true };
  opened.id("open").click();
  await settle();
  assert.equal(opened.window.closed, true);
});

test("white text on the primary and danger buttons reaches WCAG AA", () => {
  const luminance = (r, g, b) => {
    const linear = (value) => {
      const c = value / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
  };
  const contrast = (rgb) => 1.05 / (luminance(...rgb) + 0.05);
  const variable = (name) => {
    const found = css.match(new RegExp(`${name}:\\s*rgb\\((\\d+) (\\d+) (\\d+)\\)`));
    assert.ok(found, `${name} is a plain rgb() colour`);
    return found.slice(1).map(Number);
  };
  for (const name of ["--btn-primary", "--btn-primary-hover", "--btn-primary-press"]) {
    assert.ok(contrast(variable(name)) >= 4.5, `${name}: ${contrast(variable(name)).toFixed(2)}:1`);
  }
  const danger = css.match(/\.btn\.danger \{[^}]*background: rgb\((\d+) (\d+) (\d+)\)/).slice(1).map(Number);
  assert.ok(contrast(danger) >= 4.5);
});
