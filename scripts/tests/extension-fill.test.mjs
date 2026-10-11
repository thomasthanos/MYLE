// How the page script (content.js) finds fields, fills them and offers its
// menu: its functions, each run on its own with what it needs stubbed.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = (await readFile(new URL("../../extension/content.js", import.meta.url), "utf8")).replace(/\r\n/g, "\n");

/** One function of content.js, by name, with `context` as its world. */
function pick(name, context = {}) {
  let start = source.indexOf(`  function ${name}(`);
  if (start < 0) start = source.indexOf(`  async function ${name}(`);
  assert.ok(start >= 0, `${name} is in content.js`);
  const end = source.indexOf("\n  }\n", start);
  return vm.runInNewContext(`(${source.slice(start, end + 4).trim()})`, context);
}

/** A constant of content.js, by name (a regular expression, say). */
function constant(name) {
  const start = source.indexOf(`  const ${name} = `);
  assert.ok(start >= 0, `${name} is in content.js`);
  const end = source.indexOf(";\n", start);
  return vm.runInNewContext(source.slice(start + `  const ${name} = `.length, end));
}

test("a fill types the way a person does: key, beforeinput, the browser's setter, input, key, change", () => {
  const seen = [];
  class Event {
    constructor(type, init = {}) {
      Object.assign(this, { type, ...init, kind: "Event" });
    }
  }
  class KeyboardEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.kind = "KeyboardEvent";
    }
  }
  class InputEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.kind = "InputEvent";
    }
  }
  const input = {
    own: "",
    focused: false,
    focus() { this.focused = true; },
    dispatchEvent(event) { seen.push({ ...event, value: this.own }); },
  };
  // The prototype's setter: what React's value tracking does not intercept.
  const HTMLInputElement = { prototype: {} };
  Object.defineProperty(HTMLInputElement.prototype, "value", { set(value) { this.own = value; }, configurable: true });
  const setValue = pick("setValue", { HTMLInputElement, Event, KeyboardEvent, InputEvent, Object });
  setValue(input, "s3cret");
  assert.deepEqual(seen.map((event) => `${event.type}:${event.kind}`), [
    "keydown:KeyboardEvent", "beforeinput:InputEvent", "input:InputEvent", "keyup:KeyboardEvent", "change:Event",
  ]);
  assert.equal(seen[1].value, "", "beforeinput comes before the value changes");
  assert.equal(seen[2].value, "s3cret", "input comes after");
  for (const event of seen.slice(0, 4)) assert.equal(event.composed, true, `${event.type} crosses web components`);
  assert.equal(seen[1].inputType, "insertReplacementText");
  assert.equal(seen[1].cancelable, undefined, "beforeinput cannot be cancelled away");
  assert.equal(seen[0].key, "Unidentified");
  assert.ok(input.focused, "the field keeps the focus (no blur)");
});

test("the site's search box is never taken for a user name", () => {
  const getAttr = (el, name) => el.attrs?.[name] ?? null;
  const isSearch = pick("isSearch", { getAttr });
  const field = (extra) => ({ type: "text", id: "", name: "", placeholder: "", ...extra });
  assert.ok(isSearch(field({ type: "search" })));
  assert.ok(isSearch(field({ name: "q" })));
  assert.ok(isSearch(field({ attrs: { role: "searchbox" } })));
  assert.ok(isSearch(field({ placeholder: "Search the site" })));
  assert.ok(!isSearch(field({ name: "username" })));
  assert.ok(!isSearch(field({ type: "email", placeholder: "Email" })));
});

test("2FA setup words are whole words: not inside a blob, nor a one-time purchase", () => {
  const words = constant("SETUP_WORDS");
  assert.ok(words.test("Scan this QR code with your authenticator app"));
  assert.ok(words.test("Turn on 2FA for your account"));
  assert.ok(words.test("Enter the one-time code from your app"));
  assert.ok(!words.test("data:image/png;base64,iVBORw0KGgoa2fa9MFAx"));
  assert.ok(!words.test("sha256-3f2fa81c0d9e5b7a"));
  assert.ok(!words.test("A one-time purchase, no subscription"));
});

test("a code about to change is waited for, and asked for once more", async () => {
  const asked = [];
  const waits = [];
  let remaining = 2;
  const send = async (message) => {
    asked.push(message.type);
    return { ok: true, code: String(asked.length).padStart(6, "0"), remaining };
  };
  const setTimeout = (run, ms) => {
    waits.push(ms);
    run();
  };
  const freshCode = pick("freshCode", { send, setTimeout, CODE_ENDING: 3, Number, Math, Promise });
  const code = await freshCode("a");
  assert.deepEqual(asked, ["totp", "totp"]);
  assert.equal(code.code, "000002", "the new code");
  assert.equal(waits[0], 2400, "until the next one, and a little more");
  asked.length = 0;
  remaining = 20;
  assert.equal((await freshCode("a")).code, "000001");
  assert.deepEqual(asked, ["totp"], "a fresh code is used as it is");
});

test("a menu with only a note shows once per field, unless asked from the badge", () => {
  const rendered = [];
  let hidden = 0;
  const offer = pick("offer", {
    noted: new WeakSet(),
    render: (field, items) => rendered.push(items.length),
    hidePanel: () => hidden++,
  });
  const field = {};
  const note = { classList: { contains: (name) => name === "note" } };
  const login = { classList: { contains: () => false } };
  offer(field, [note], false);
  offer(field, [note], false);
  assert.deepEqual(rendered, [1]);
  assert.equal(hidden, 1, "the second click shows nothing");
  offer(field, [note], true);
  assert.deepEqual(rendered, [1, 1], "the badge shows it again");
  offer(field, [login], false);
  assert.equal(rendered.length, 3, "a menu with logins always shows");
});

test("the menu explains what stands in the way, also on a 2FA field", () => {
  const made = [];
  const problemItems = pick("problemItems", {
    openButton: (label, glyph, field) => made.push(["open", label, !!field]) && { label },
    note: (text) => made.push(["note", text]) && { text },
    problems: { busy: "Too many requests", reloaded: "Reload", vaultError: "Vault" },
  });
  problemItems({ ok: false, error: "notRunning" }, {});
  problemItems({ ok: false, error: "locked" }, {}, false, "Unlock your vault in MYLE for the 2FA code");
  problemItems({ ok: false, error: "hostMissing" }, {});
  problemItems({ ok: false, error: "reloaded" }, {});
  assert.deepEqual(made[0], ["open", "Open MYLE to fill in", true], "the menu comes back once MYLE is open");
  assert.deepEqual(made[1], ["open", "Unlock your vault in MYLE for the 2FA code", true]);
  assert.match(made[2][1], /Browser filling is off/);
  assert.deepEqual(made[3], ["note", "Reload"]);
  assert.deepEqual([...problemItems({ ok: false, error: "somethingNew" }, {})], []);
});

test("the badge sits at the field's right end, or just left of the page's own icon there", () => {
  let padding = "10px";
  const badgeSpot = pick("badgeSpot", {
    BADGE: 22,
    innerHeight: 800,
    innerWidth: 1200,
    getComputedStyle: () => ({ paddingRight: padding }),
    parseFloat,
    Math,
  });
  const field = { getBoundingClientRect: () => ({ left: 100, right: 400, top: 50, bottom: 86, width: 300, height: 36 }) };
  assert.deepEqual({ ...badgeSpot(field) }, { left: 372, top: 57 });
  padding = "40px";
  assert.deepEqual({ ...badgeSpot(field) }, { left: 336, top: 57 }, "clear of a show-password icon");
  const tiny = { getBoundingClientRect: () => ({ left: 0, right: 40, top: 0, bottom: 40, width: 40, height: 40 }) };
  assert.equal(badgeSpot(tiny), null, "no room in a code box");
});

test("no save offer while the sign-in form is still shown (the password was refused)", async () => {
  const sent = [];
  const offerSave = pick("offerSave", {
    prompted: null,
    passwordFields: () => [{}],
    send: async (message) => sent.push(message.type),
  });
  await offerSave();
  assert.deepEqual(sent, [], "not even asked: the login waits for the next page");
});
