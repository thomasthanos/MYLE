// MYLE Passwords: on the page.
//
// Finds login fields, and when one is clicked shows the logins saved for
// this site; a click on one fills it in. Nothing is filled without a click.
// After a login form is sent, the next page offers to save it.
//
// The page's own scripts share this document, so this assumes they may be
// hostile:
// - only the user's real input counts (event.isTrusted), never events a
//   script makes up;
// - the menu is drawn in a closed shadow root in the browser's top layer,
//   styled so the page cannot hide or restyle it, and a click on it counts
//   only once it has been on screen, still and untouched, for half a second,
//   and (where the browser can tell) while nothing is drawn over it;
// - nothing is filled into hidden fields or into a form that sends over
//   plain http, and only a password the user typed (or picked in the menu)
//   is offered for saving.
(() => {
  const ext = globalThis.browser ?? globalThis.chrome;
  if (window.__myleFill || !(document.documentElement instanceof HTMLHtmlElement)) return;
  window.__myleFill = true;
  // Two copies installed (the store's, and one loaded from a folder): the
  // first one in a page serves it, so nothing shows twice.
  const COPY = "data-myle-passwords";
  if (document.documentElement.hasAttribute(COPY)) return;
  document.documentElement.setAttribute(COPY, "");

  const send = (message) => ext.runtime.sendMessage(message).catch(() => ({ ok: false, error: "noHost" }));
  const LOCAL = ["localhost", "127.0.0.1", "[::1]"];
  const siteName = location.hostname.replace(/^www\./, "");

  // A page can shadow a form's methods and properties with fields named after
  // them (<input name="action">), so these come from the prototypes.
  const getAttr = (el, name) => Element.prototype.getAttribute.call(el, name);
  const queryAll = (scope, selector) => [
    ...(scope instanceof Document ? Document.prototype : Element.prototype).querySelectorAll.call(scope, selector),
  ];

  // --- Fields -------------------------------------------------------------

  /** On screen for the user: not hidden, see-through, tiny or off the page. */
  function visible(el) {
    const r = el.getBoundingClientRect();
    if (r.width <= 20 || r.height <= 10 || r.right + scrollX <= 0 || r.bottom + scrollY <= 0) return false;
    if (typeof el.checkVisibility === "function") {
      return el.checkVisibility({
        opacityProperty: true,
        visibilityProperty: true,
        checkOpacity: true,
        checkVisibilityCSS: true,
      });
    }
    const s = getComputedStyle(el);
    return s.visibility !== "hidden" && s.display !== "none" && Number(s.opacity) > 0;
  }

  const usable = (el) =>
    el instanceof HTMLInputElement && el.isConnected && !el.disabled && !el.readOnly && visible(el);

  const passwordFields = () => queryAll(document, 'input[type="password"]').filter(usable);

  const marked = (el, token) => new RegExp(`\\b${token}\\b`, "i").test(getAttr(el, "autocomplete") ?? "");

  // Some sites ask for the email before showing a password field.
  function isUsernameOnly(field) {
    if (!(field instanceof HTMLInputElement) || !["text", "email", "tel"].includes(field.type)) return false;
    if (!usable(field)) return false;
    const hint = `${field.name} ${field.id} ${field.autocomplete} ${field.placeholder} ${getAttr(field, "aria-label") ?? ""}`;
    return !/search|query|captcha|verification|one.time|otp|code/i.test(hint) &&
      (marked(field, "username") || field.type === "email" || /user|mail|login|account/i.test(hint));
  }

  /** The user name field that goes with a password field: the nearest
   *  visible text or email field before it, in the same form if there is one. */
  function usernameFor(password) {
    const fields = queryAll(password.form ?? document, "input").filter(
      (el) => ["text", "email", "tel"].includes(el.type) && usable(el),
    );
    const before = fields.filter((el) => el.compareDocumentPosition(password) & Node.DOCUMENT_POSITION_FOLLOWING);
    const preferred = before.filter((el) => /user|mail|login|account|name|id/i.test(`${el.name} ${el.id} ${el.autocomplete}`));
    return preferred.at(-1) ?? before.at(-1) ?? null;
  }

  function isLoginField(field) {
    if (!usable(field)) return false;
    const passwords = passwordFields();
    return passwords.includes(field) || passwords.some((p) => usernameFor(p) === field) ||
      (passwords.length === 0 && isUsernameOnly(field));
  }

  /** In a sign-up or password-change form, the fields a new password goes
   *  into (the new one and its repeat), when the clicked field is one of
   *  them. `marked`: the site says so (autocomplete="new-password"). */
  function newPasswordFields(field) {
    const none = { fields: [], marked: false };
    if (!(field instanceof HTMLInputElement) || field.type !== "password") return none;
    const all = passwordFields().filter((el) => el.form === field.form);
    const flagged = all.filter((el) => marked(el, "new-password"));
    if (flagged.length) return flagged.includes(field) ? { fields: flagged, marked: true } : none;
    // Unmarked: new and repeat, or current, new and repeat.
    const fields = all.length === 2 ? all : all.length === 3 ? all.slice(1) : [];
    return fields.includes(field) ? { fields, marked: false } : none;
  }

  // A 2FA code field: the site says so, or its name says so, or it is a row
  // of one-digit boxes.
  const CODE_WORDS = /one.?time|otp|2fa|mfa|two.?factor|authenticator|verification.?code|security.?code|auth.?code|passcode/i;

  /** The row of one-character boxes a code is split into, around `field`. */
  function codeBoxes(field) {
    if (field.maxLength !== 1) return [];
    let scope = field.parentElement;
    for (let depth = 0; scope && depth < 3; depth++, scope = scope.parentElement) {
      const boxes = queryAll(scope, "input").filter((el) => el.maxLength === 1 && usable(el));
      if (boxes.length > 8) return [];
      if (boxes.length >= 4) return boxes;
    }
    return [];
  }

  function isCodeField(field) {
    if (!(field instanceof HTMLInputElement) || !usable(field)) return false;
    if (marked(field, "one-time-code")) return true;
    if (!["text", "tel", "number"].includes(field.type)) return false;
    if (codeBoxes(field).length) return true;
    const hint = `${field.name} ${field.id} ${field.placeholder} ${getAttr(field, "aria-label") ?? ""}`;
    if (CODE_WORDS.test(hint)) return true;
    const numeric = field.inputMode === "numeric" || field.type !== "text" || /\\d|0-9/.test(getAttr(field, "pattern") ?? "");
    return numeric && field.maxLength >= 6 && field.maxLength <= 8 && /code|pin/i.test(hint);
  }

  /** Types a 2FA code into its field, or digit by digit into its boxes. */
  function fillCode(field, code) {
    const boxes = codeBoxes(field);
    if (boxes.length >= code.length) {
      boxes.slice(0, code.length).forEach((box, i) => setValue(box, code[i]));
      return true;
    }
    if (!usable(field)) return false;
    setValue(field, code);
    return true;
  }

  /** A form that would send what is typed in it over plain http. */
  function insecureForm(field) {
    const form = field?.form;
    if (!form) return false;
    const targets = [getAttr(form, "action"), ...queryAll(form, "[formaction]").map((el) => getAttr(el, "formaction"))];
    return targets.some((target) => {
      if (!target) return false;
      try {
        const url = new URL(target, document.baseURI);
        return url.protocol === "http:" && !LOCAL.includes(url.hostname);
      } catch {
        return false;
      }
    });
  }

  /** Sets a value the way typing does, so the page's own code notices. */
  function setValue(input, value) {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    input.focus();
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function fill(credentials, chosen) {
    const passwords = passwordFields();
    const selected = [chosen, document.activeElement].find(usable);
    const inForm = selected?.form ? passwords.filter((field) => field.form === selected.form) : [];
    const afterSelected = selected && !selected.form ? passwords.filter(
      (field) => selected.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING,
    ) : [];
    const password = selected?.type === "password" && passwords.includes(selected) ? selected :
      selected?.form ? (inForm.length === 1 ? inForm[0] : null) :
        afterSelected.length === 1 ? afterSelected[0] :
          passwords.length === 1 ? passwords[0] : null;
    const username = password ? usernameFor(password) :
      (selected && isUsernameOnly(selected) ? selected : (() => {
        const candidates = queryAll(document, "input").filter(isUsernameOnly);
        return candidates.length === 1 ? candidates[0] : null;
      })());
    // An email-only sign-in step can coexist with an unrelated password form.
    // A clicked email field in its own form is still an unambiguous target.
    const emailOnlyStep = selected?.form && isUsernameOnly(selected) && inForm.length === 0;
    // Other multi-password pages may be registration or password-change
    // forms. Filling only the user name there would falsely report success.
    if (passwords.length > 0 && credentials.password && !password && !emailOnlyStep)
      return { applied: false, error: "ambiguousFields" };
    if (insecureForm(password ?? username)) return { applied: false, error: "insecureForm" };
    let applied = false;
    if (username && credentials.username) {
      setValue(username, credentials.username);
      applied = true;
    }
    if (password && credentials.password) {
      setValue(password, credentials.password);
      applied = true;
    }
    return { applied, error: applied ? undefined : "noFields" };
  }

  // --- The menu ---------------------------------------------------------------

  /** How long the menu must be on screen, still, before a click on it counts. */
  const ARM_MS = 500;
  /** MYLE's mark: a shield with a keyhole on the app's violet. */
  const MARK = '<svg class="mark" viewBox="0 0 24 24" aria-hidden="true"><defs>' +
    '<linearGradient id="myle-mark" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#a3acff"/>' +
    '<stop offset="1" stop-color="#6b5cf2"/></linearGradient></defs>' +
    '<rect x="1" y="1" width="22" height="22" rx="7" fill="url(#myle-mark)"/>' +
    '<path d="M12 5.2 17.4 7.3v4.3c0 3.3-2.2 5.8-5.4 7.1-3.2-1.3-5.4-3.8-5.4-7.1V7.3z" fill="#fff" fill-opacity=".96"/>' +
    '<circle cx="12" cy="10.7" r="1.7" fill="#6b5cf2"/><path d="M11.2 11.9h1.6l.45 2.7h-2.5z" fill="#6b5cf2"/></svg>';
  /** Line icons for the menu's own items. */
  const GLYPHS = {
    lock: '<rect x="5" y="11" width="14" height="10" rx="2.5"/><path d="M8 11V7.5a4 4 0 0 1 8 0V11"/>',
    open: '<path d="M14 4h6v6"/><path d="m20 4-8.5 8.5"/><path d="M18 14.5V19a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h4.5"/>',
    sparkle: '<path d="M12 3.5 13.9 9 19.5 11 13.9 13 12 18.5 10.1 13 4.5 11 10.1 9z"/><path d="M19 3v4M17 5h4"/>',
    more: '<path d="m6 9 6 6 6-6"/>',
    key: '<circle cx="8" cy="15" r="4"/><path d="m10.8 12.2 8.7-8.7"/><path d="m17 6 2.5 2.5"/><path d="m14.5 8.5 2.5 2.5"/>',
  };
  const glyph = (name) =>
    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" ` +
    `stroke-linejoin="round" aria-hidden="true">${GLYPHS[name]}</svg>`;
  const STYLE = `
    :host {
      all: initial !important; display: block !important; position: fixed !important;
      inset: 0 auto auto 0 !important; width: 0 !important; height: 0 !important;
      min-width: 0 !important; min-height: 0 !important; margin: 0 !important; padding: 0 !important;
      border: 0 !important; overflow: visible !important; z-index: 2147483647 !important;
      opacity: 1 !important; visibility: visible !important; transform: none !important;
      filter: none !important; clip-path: none !important; mask: none !important;
      mix-blend-mode: normal !important; transition: none !important; animation: none !important;
      background: none !important;
    }
    :host(:not(:popover-open)) { display: none !important; }
    [hidden] { display: none !important; }
    .panel, .bar { position: fixed; box-sizing: border-box; color: #e8ebf7; background: #171b27;
      font: 13px/1.35 system-ui, "Segoe UI", sans-serif; border: 1px solid rgba(255,255,255,.1);
      box-shadow: 0 18px 44px -14px rgba(0,0,0,.75); color-scheme: dark; }
    .panel { width: 304px; max-width: calc(100vw - 16px); max-height: min(360px, calc(100vh - 16px));
      overflow: auto; padding: 5px; border-radius: 12px; }
    .mark { width: 16px; height: 16px; flex: none; }
    .head { display: flex; align-items: center; gap: 7px; padding: 6px 8px 7px; color: #c4c9ec;
      font-size: 11.5px; font-weight: 650; letter-spacing: .02em; }
    .head span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
      color: #6f7799; font-weight: 500; }
    button { all: unset; box-sizing: border-box; cursor: pointer; }
    .item { display: flex; width: 100%; align-items: center; gap: 10px; padding: 8px 9px; border-radius: 8px; }
    .item:hover, .item:focus-visible { background: rgba(132,142,222,.16); }
    .item:focus-visible { box-shadow: inset 0 0 0 2px rgba(132,142,222,.55); }
    .avatar { --hue: 235; flex: none; display: grid; place-items: center; width: 28px; height: 28px;
      overflow: hidden; border-radius: 8px; border: 1px solid hsl(var(--hue) 55% 70% / .22);
      background: linear-gradient(145deg, hsl(var(--hue) 55% 62% / .38), hsl(var(--hue) 55% 50% / .12));
      color: hsl(var(--hue) 75% 88%); font-weight: 650; font-size: 12.5px; }
    .avatar.image { border-color: rgba(255,255,255,.16); background: linear-gradient(160deg, #fbfbfe, #e9ebf3); }
    .avatar img { width: 64%; height: 64%; object-fit: contain; }
    .avatar svg { width: 15px; height: 15px; }
    .avatar.glyph { color: #cdd1ff; }
    .suggest .avatar { --hue: 150; color: #bff2d8; }
    .item.more { justify-content: center; gap: 6px; min-height: 0; padding: 7px 9px; color: #9aa3c7; font-size: 12px; }
    .item.more svg { width: 14px; height: 14px; }
    .label { padding: 4px 9px 2px; color: #7d86ab; font-size: 10.5px; font-weight: 650;
      letter-spacing: .06em; text-transform: uppercase; }
    .text { display: grid; min-width: 0; }
    .text b, .text small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .text b { font-weight: 600; }
    .text small { color: #8d95b8; font-size: 11.5px; }
    .text .other { color: #f2c27a; }
    .note { padding: 8px 9px 9px; color: #aab2d4; font-size: 12px; }
    .bar { right: 16px; top: 16px; width: 332px; max-width: calc(100vw - 32px); padding: 14px;
      border-radius: 14px; line-height: 1.4; }
    .bar .title { display: flex; align-items: center; gap: 8px; font-weight: 600; }
    .bar p { margin: 6px 0 12px; color: #aab2d4; overflow-wrap: anywhere; }
    .bar .row { display: flex; gap: 8px; justify-content: flex-end; }
    .bar button { padding: 7px 12px; border-radius: 8px; background: rgba(255,255,255,.07); }
    .bar button:hover { background: rgba(255,255,255,.12); }
    .bar button:focus-visible { box-shadow: 0 0 0 2px rgba(132,142,222,.6); }
    .bar button.primary { background: #848ede; color: #fff; font-weight: 600; }
    .bar button:disabled { opacity: .6; cursor: default; }
    .bar .keys { display: grid; gap: 4px; margin: -4px 0 12px; }
    .bar .keys .item { padding: 7px 8px; background: rgba(255,255,255,.04); color: inherit; font-weight: 400; }
    .bar .keys .item:hover { background: rgba(132,142,222,.16); }`;

  // A random tag name: a page cannot define it before the menu is made.
  const host = document.createElement(`myle-${crypto.getRandomValues(new Uint32Array(2)).join("-")}`);
  const topLayer = typeof host.showPopover === "function";
  if (topLayer) host.setAttribute("popover", "manual");
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `<style>${STYLE}</style><div class="panel" role="menu" hidden></div>` +
    '<div class="bar" role="dialog" aria-label="MYLE Passwords" hidden></div>';
  const panel = root.querySelector(".panel");
  const bar = root.querySelector(".bar");
  /** When each part last appeared, moved or changed. */
  const since = new Map([[panel, 0], [bar, 0]]);
  /** The parts the browser last saw fully visible, nothing over them
   *  (Chromium's IntersectionObserver v2; other browsers cannot tell). */
  const unobscured = new Set();
  const tracker = "isVisible" in (globalThis.IntersectionObserverEntry?.prototype ?? {}) ?
    new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isVisible) unobscured.add(entry.target);
        else unobscured.delete(entry.target);
      }
    }, { trackVisibility: true, delay: 100 }) :
    null;
  let anchor = null;
  let spot = "";

  function mount(field) {
    // A login form in a modal dialog: the menu goes inside the dialog, or the
    // dialog would leave it unclickable.
    let parent = document.documentElement;
    try {
      const dialog = field?.closest("dialog");
      if (dialog?.matches(":modal")) parent = dialog;
    } catch {
      // No :modal in this browser.
    }
    if (host.parentNode !== parent) {
      closeLayer();
      parent.append(host);
    }
  }

  function openLayer() {
    if (!topLayer) return;
    try {
      // Shown again, it goes back above anything the page put in the top layer.
      if (host.matches(":popover-open")) host.hidePopover();
      host.showPopover();
    } catch {
      // Not connected, or the page cancelled it: the menu stays hidden.
    }
  }

  function closeLayer() {
    if (!topLayer) return;
    try {
      if (host.matches(":popover-open")) host.hidePopover();
    } catch {
      // Already closed.
    }
  }

  function show(part) {
    part.hidden = false;
    since.set(part, performance.now());
    tracker?.observe(part);
    openLayer();
  }

  function hide(part) {
    part.hidden = true;
    tracker?.unobserve(part);
    unobscured.delete(part);
    if (panel.hidden && bar.hidden) closeLayer();
  }

  function hidePanel() {
    hide(panel);
    anchor = null;
    spot = "";
  }

  // The page changing the menu's element (its style, or its place in the top
  // layer) closes the menu, and the element is put back as it was.
  const guard = new MutationObserver(() => {
    guard.disconnect();
    for (const name of host.getAttributeNames()) if (name !== "popover") host.removeAttribute(name);
    if (topLayer) host.setAttribute("popover", "manual");
    hidePanel();
    hide(bar);
    guard.observe(host, { attributes: true });
  });
  guard.observe(host, { attributes: true });
  host.addEventListener("toggle", (event) => {
    // Closed by the page rather than by us.
    if (event.isTrusted && topLayer && !host.matches(":popover-open") && !(panel.hidden && bar.hidden)) {
      hidePanel();
      hide(bar);
    }
  });

  function place(field) {
    if (!field?.isConnected) return hidePanel();
    const r = field.getBoundingClientRect();
    if (r.bottom < 0 || r.top > innerHeight) return hidePanel();
    const width = panel.offsetWidth;
    const height = panel.offsetHeight;
    const viewport = document.documentElement.clientWidth || innerWidth;
    const left = Math.round(Math.max(8, Math.min(r.left, viewport - width - 8)));
    const below = r.bottom + 6;
    const top = Math.round(below + height > innerHeight - 8 ? Math.max(8, r.top - 6 - height) : below);
    panel.style.left = `${left}px`;
    panel.style.top = `${top}px`;
    // A menu that moved (the page scrolled, or moved the field) waits again.
    if (`${left},${top}` !== spot) {
      spot = `${left},${top}`;
      since.set(panel, performance.now());
    }
  }

  /** Why a click (or Enter) on the menu does not count, or "" when it is the
   *  user's own, on the menu as they could see it: not made up by a script,
   *  not the second click of a double click, not on a menu that just
   *  appeared or moved under the pointer, and not through something drawn
   *  over it ("covered"). */
  function refusal(event, part, button) {
    if (!event.isTrusted || event.detail > 1) return "fake";
    if (part.hidden || performance.now() - since.get(part) < ARM_MS) return "early";
    if (!host.isConnected || (topLayer && !host.matches(":popover-open"))) return "fake";
    if (tracker && !unobscured.has(part)) return "covered";
    if (!topLayer) {
      const page = getComputedStyle(document.documentElement);
      if (Number(page.opacity) < 1 || page.visibility !== "visible") return "covered";
    }
    // Enter or Space on the focused item, or a pointer on the menu itself.
    if (event.detail === 0) return root.activeElement === button ? "" : "fake";
    return document.elementFromPoint(event.clientX, event.clientY) === host ? "" : "covered";
  }

  /** Whether a click on the menu counts; if something covered the menu, says
   *  so (shown again, the menu comes back on top). */
  function genuine(event, part, button) {
    const why = refusal(event, part, button);
    if (why === "covered" && part === panel && anchor) {
      showNote(anchor, "Something on this page covered MYLE's menu, so nothing was filled. " +
        "Click the field again, or use the MYLE button in the toolbar.");
    }
    return !why;
  }

  /** A site's colour, the same every time (as in the app). */
  function hueOf(text) {
    let hash = 7;
    for (const char of text) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
    return hash % 360;
  }

  /** A login's avatar: its website's icon when the app has one, else its
   *  first letter on its site's colour. */
  function avatarFor(login, title) {
    const avatar = document.createElement("span");
    avatar.className = "avatar";
    avatar.style.setProperty("--hue", String(hueOf(login.site || title.toLowerCase())));
    const letter = (title.match(/[\p{L}\p{N}]/u)?.[0] ?? "?").toUpperCase();
    if (typeof login.icon === "string" && login.icon.startsWith("data:image/")) {
      const img = document.createElement("img");
      img.alt = "";
      img.draggable = false;
      // A page whose rules forbid such images: the letter instead.
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

  function glyphAvatar(name) {
    const avatar = document.createElement("span");
    avatar.className = "avatar glyph";
    avatar.innerHTML = glyph(name);
    return avatar;
  }

  function item(title, sub, avatar, other) {
    const button = document.createElement("button");
    button.className = "item";
    button.setAttribute("role", "menuitem");
    const text = document.createElement("span");
    text.className = "text";
    const name = document.createElement("b");
    name.textContent = title;
    const small = document.createElement("small");
    small.textContent = sub;
    if (other) {
      // Saved for another host of this site: say which.
      const where = document.createElement("span");
      where.className = "other";
      where.textContent = ` · ${other}`;
      small.append(where);
    }
    text.append(name, small);
    button.append(avatar, text);
    button.title = [title, sub, other].filter(Boolean).join(" · ");
    return button;
  }

  function header() {
    const head = document.createElement("div");
    head.className = "head";
    head.innerHTML = MARK;
    const site = document.createElement("span");
    site.textContent = siteName;
    head.append("MYLE", site);
    return head;
  }

  function note(text) {
    const box = document.createElement("div");
    box.className = "note";
    box.textContent = text;
    return box;
  }

  function render(field, children) {
    anchor = field;
    mount(field);
    panel.replaceChildren(header(), ...children);
    show(panel);
    place(field);
  }

  function showNote(field, text, fade) {
    if (!field?.isConnected) return;
    render(field, [note(text)]);
    const shown = panel.lastChild;
    if (fade) setTimeout(() => panel.lastChild === shown && anchor === field && hidePanel(), 6000);
  }

  const problems = {
    ambiguousFields: "This page has several password fields. Click the one to fill, then pick the login again.",
    insecureForm: "This form would send your password over plain http, so MYLE does not fill it.",
    locked: "Your vault is locked. Unlock it in MYLE, then try again.",
    busy: "Too many requests just now. Try again in a minute.",
    wrongSite: "That login is saved for another website.",
    notFound: "That login is no longer in your vault.",
    notRunning: "MYLE is not running. Open it, then try again.",
    noTotp: "That login has no 2FA key in MYLE any more.",
    badKey: "MYLE could not read that 2FA key.",
  };

  function entryButton(login) {
    const title = login.title || login.site || "Login";
    const button = item(title, login.username || "No user name", avatarFor(login, title),
      login.exact ? "" : login.site);
    button.addEventListener("click", async (event) => {
      if (!genuine(event, panel, button)) return;
      const field = anchor;
      hidePanel();
      // Checked before the password is even asked for.
      if (insecureForm(field)) return showNote(field, problems.insecureForm);
      const answer = await send({ type: "fill", id: login.id });
      const result = answer?.ok ? fill(answer, field) : { applied: false, error: answer?.error };
      if (!result.applied) {
        showNote(field, problems[result.error] ?? (answer?.ok ?
          "That sign-in field changed. Click it again and retry." :
          "Could not fill this login. Check its website in MYLE."));
      }
    });
    return button;
  }

  function codeButton(login) {
    const title = login.title || login.site || "Login";
    const button = item(title, `2FA code · ${login.username || "No user name"}`, avatarFor(login, title),
      login.exact ? "" : login.site);
    button.addEventListener("click", async (event) => {
      if (!genuine(event, panel, button)) return;
      const field = anchor;
      hidePanel();
      const answer = await send({ type: "totp", id: login.id });
      if (!answer?.ok || typeof answer.code !== "string" || !/^\d{6,8}$/.test(answer.code)) {
        return showNote(field, problems[answer?.error] ?? "MYLE could not make the code just now.");
      }
      if (!fillCode(field, answer.code)) showNote(field, "The code field changed. Click it again and retry.");
    });
    return button;
  }

  /** On a 2FA code field: the codes of this site's logins that have a key.
   *  Nothing at all when none has (the code may come by SMS or email). */
  async function showCodesFor(field) {
    anchor = field;
    const answer = await send({ type: "logins" });
    if (anchor !== field || !field.isConnected) return;
    if (!answer?.ok) {
      return answer?.error === "locked" ? render(field, [openButton("Unlock your vault in MYLE for the 2FA code", "lock")]) :
        hidePanel();
    }
    const withCodes = (answer.logins ?? []).filter((login) => login.totp === true);
    if (!withCodes.length) return hidePanel();
    // The account the page already names comes first.
    const named = accountsOnPage(withCodes, field);
    const ordered = [...named, ...withCodes.filter((login) => !named.includes(login))];
    render(field, ordered.slice(0, 6).map(codeButton));
  }

  function suggestButton(fields) {
    const button = item("Suggest a strong password", "20 characters, made by MYLE", glyphAvatar("sparkle"));
    button.classList.add("suggest");
    button.addEventListener("click", async (event) => {
      if (!genuine(event, panel, button)) return;
      const field = anchor;
      hidePanel();
      if (insecureForm(fields[0])) return showNote(field, problems.insecureForm);
      const answer = await send({ type: "generate" });
      if (!answer?.ok || typeof answer.password !== "string") {
        return showNote(field, problems[answer?.error] ?? "MYLE could not make a password just now.");
      }
      if (!fields.every(usable)) return showNote(field, "The form changed. Click the password field again.");
      for (const target of fields) {
        setValue(target, answer.password);
        approved.set(target, answer.password);
      }
      showNote(field, "A strong password is filled in. When you send the form, MYLE offers to save it.", true);
    });
    return button;
  }

  function openButton(label, glyphName) {
    const button = item(label, "Opens MYLE on this PC", glyphAvatar(glyphName));
    button.addEventListener("click", (event) => {
      if (!genuine(event, panel, button)) return;
      hidePanel();
      void send({ type: "open" });
    });
    return button;
  }

  /** The logins whose user name this page already shows: on a password step
   *  (Google's, say) the account chosen before, from a user name field,
   *  visible or hidden, or the page's own text. Empty when none, or all. */
  function accountsOnPage(logins, field) {
    const typed = queryAll(document, "input")
      .filter((el) => el !== field && el.type !== "password" && el.value &&
        (marked(el, "username") || marked(el, "email") || /user|mail|login|identifier|account/i.test(`${el.name} ${el.id}`)))
      .map((el) => el.value.trim().toLowerCase());
    const text = (document.body?.innerText ?? "").slice(0, 60_000).toLowerCase();
    const named = logins.filter((login) => {
      const name = (login.username ?? "").trim().toLowerCase();
      if (name.length < 3) return false;
      if (typed.includes(name)) return true;
      // A whole word of the page, not part of a longer one.
      const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return new RegExp(`(^|[^\\p{L}\\p{N}._@-])${escaped}($|[^\\p{L}\\p{N}._@-])`, "u").test(text);
    });
    return named.length < logins.length ? named : [];
  }

  function moreButton(count, reveal) {
    const button = document.createElement("button");
    button.className = "item more";
    button.innerHTML = glyph("more");
    button.append(`${count} other ${count === 1 ? "login" : "logins"} for this site`);
    button.addEventListener("click", (event) => {
      if (event.isTrusted) reveal();
    });
    return button;
  }

  async function showFor(field) {
    anchor = field;
    const fresh = newPasswordFields(field);
    const [answer, passkeys] = await Promise.all([send({ type: "logins" }), passkeyChoices()]);
    if (anchor !== field || !field.isConnected) return;
    const items = [...passkeys];
    if (answer?.ok) {
      const logins = answer.logins ?? [];
      // A new password only while the vault is open, so it can be saved.
      const suggestion = fresh.fields.length ? suggestButton(fresh.fields) : null;
      if (suggestion && fresh.marked) items.push(suggestion);
      // The account the page already names comes alone; the rest on request.
      const named = accountsOnPage(logins, field);
      if (named.length) {
        items.push(...named.map(entryButton));
        items.push(moreButton(logins.length - named.length, () => {
          if (anchor !== field) return;
          const rest = logins.filter((login) => !named.includes(login));
          render(field, [...named, ...rest].slice(0, 8).map(entryButton));
        }));
      } else {
        items.push(...logins.slice(0, 6).map(entryButton));
        if (logins.length > 6) items.push(note(`${logins.length - 6} more: use the MYLE button in the toolbar.`));
      }
      if (suggestion && !fresh.marked) items.push(suggestion);
      if (!items.length) items.push(note(`No saved login for ${siteName}. Add this website to a login in MYLE.`));
    } else if (answer?.error === "locked") {
      items.push(openButton("Unlock your vault in MYLE", "lock"));
    } else if (answer?.error === "noVault") {
      items.push(openButton("Create your vault in MYLE", "lock"));
    } else if (answer?.error === "notRunning") {
      items.push(openButton("Open MYLE to fill in", "open"));
    } else if (answer?.error === "disabled") {
      items.push(note("Browser filling is off in MYLE. Turn it on: Password Manager → ⋯ → Browser filling."));
    } else if (answer?.error === "hostMissing") {
      items.push(note("This browser cannot reach MYLE yet. Open MYLE once: it connects your browsers by itself."));
    } else if (answer?.error === "hostForbidden") {
      items.push(note("MYLE does not know this copy of the extension. Load the one from MYLE: Password Manager → ⋯ → Browser filling."));
    } else if (answer?.error === "hostExited" || answer?.error === "noHost") {
      items.push(note("MYLE could not answer this browser. Make sure MYLE is installed and up to date."));
    } else if (answer?.error === "embedded") {
      items.push(note(`This sign-in form comes from ${answer.site}, inside ${answer.top}. ` +
        "To fill it, use the MYLE button in the browser's toolbar."));
    } else if (answer?.error === "busy") {
      items.push(note(problems.busy));
    } else if (!items.length) {
      return hidePanel();
    }
    render(field, items);
  }

  // A click (not mere focus, which pages move around by themselves) opens it.
  document.addEventListener(
    "mousedown",
    (event) => {
      if (!event.isTrusted || event.button !== 0) return;
      const path = event.composedPath();
      if (path.includes(host)) return;
      const field = path.find((el) => el instanceof HTMLInputElement);
      if (field && isLoginField(field)) {
        // Clicked again while the menu shows only a note: look again.
        if (anchor !== field || panel.hidden || !panel.querySelector(".item")) void showFor(field);
      } else if (field && isCodeField(field)) {
        if (anchor !== field || panel.hidden || !panel.querySelector(".item")) void showCodesFor(field);
      } else {
        hidePanel();
      }
    },
    true,
  );

  document.addEventListener(
    "keydown",
    (event) => {
      if (!event.isTrusted) return;
      const target = event.composedPath()[0];
      if (target === host) return; // Keys inside the menu: below.
      if (event.key === "Escape" && !panel.hidden) {
        hidePanel();
      } else if (event.key === "ArrowDown" && !panel.hidden && target === anchor) {
        // From the field into the menu.
        const first = panel.querySelector(".item");
        if (first) {
          event.preventDefault();
          first.focus();
        }
      } else if (event.key === "Enter" && target instanceof HTMLInputElement && target.type === "password") {
        captureFrom(target.form);
      }
    },
    true,
  );

  root.addEventListener("keydown", (event) => {
    if (!event.isTrusted || panel.hidden) return;
    const items = [...panel.querySelectorAll(".item")];
    const at = items.indexOf(root.activeElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      if (!items.length) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = at < 0 ? (step > 0 ? 0 : items.length - 1) : (at + step + items.length) % items.length;
      items[next].focus();
    } else if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      event.stopPropagation();
      const field = anchor;
      hidePanel();
      field?.focus();
    }
  });

  addEventListener("scroll", () => !panel.hidden && anchor && place(anchor), true);
  addEventListener("resize", () => !panel.hidden && anchor && place(anchor));
  addEventListener("pagehide", () => {
    hidePanel();
    hide(bar);
  });

  // The popup's "Fill" lands here.
  ext.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (sender.id !== ext.runtime.id) return;
    if (message?.type === "inspect") {
      const passwords = passwordFields();
      const usernames = queryAll(document, "input").filter(isUsernameOnly);
      const fields = [...passwords, ...usernames];
      const active = document.activeElement;
      sendResponse({
        fields: fields.length > 0,
        password: passwords.length > 0,
        focused: fields.includes(active),
        // Every sign-in field here sends over plain http: not worth a password.
        insecure: fields.length > 0 && fields.every(insecureForm),
      });
    } else if (message?.type === "apply") {
      sendResponse(message.origin === location.origin ?
        fill(message, null) : { applied: false, error: "pageChanged" });
    }
  });

  // --- Passkeys --------------------------------------------------------------
  //
  // The site's navigator.credentials calls come from passkeys.js, in the
  // page's world. The user decides here, in MYLE's own prompt; the app answers
  // for the address the browser reports, never one the page names.

  const FROM_PAGE = "myle-passkeys/page";
  const TO_PAGE = "myle-passkeys/extension";
  /** A sign-in form's standing request for a passkey picked from the menu. */
  let waitingPasskey = null;
  /** The request MYLE's prompt is for. */
  let prompted = null;

  const answerPage = (id, answer) => window.postMessage({ source: TO_PAGE, id, ...answer }, location.origin);

  const passkeyErrors = {
    notVerified: ["NotAllowedError", "Windows Hello did not confirm it is you."],
    excluded: ["InvalidStateError", "This account already has a passkey in MYLE."],
    rpMismatch: ["SecurityError", "This page may not use that site's passkeys."],
    notFound: ["NotAllowedError", "That passkey is no longer in your vault."],
    locked: ["NotAllowedError", "Your vault locked meanwhile. Unlock it in MYLE and try again."],
  };

  function refuse(id, error) {
    const [name, message] = passkeyErrors[error] ?? ["NotAllowedError", "The operation either timed out or was not allowed."];
    answerPage(id, { result: "error", name, message });
  }

  function closePrompt() {
    prompted = null;
    hide(bar);
  }

  /** MYLE's prompt: a title, a line, what goes between, then the buttons
   *  ([label, primary, run]). Returns the line, to say what happens. */
  function prompt(id, title, line, buttons, between = []) {
    prompted = id;
    mount(null);
    const head = document.createElement("div");
    head.className = "title";
    head.innerHTML = MARK;
    head.append(title);
    const text = document.createElement("p");
    text.textContent = line;
    const row = document.createElement("div");
    row.className = "row";
    for (const [label, primary, run] of buttons) {
      const button = document.createElement("button");
      button.textContent = label;
      if (primary) button.className = "primary";
      button.addEventListener("click", (event) => {
        if (prompted !== id || button.disabled || !genuine(event, bar, button)) return;
        void run(button, text);
      });
      row.append(button);
    }
    bar.replaceChildren(head, text, ...between, row);
    show(bar);
    return text;
  }

  const anotherDevice = (id) => ["Another device", false, () => {
    closePrompt();
    answerPage(id, { result: "native" });
  }];
  const cancel = (id) => ["Cancel", false, () => {
    closePrompt();
    refuse(id, "cancelled");
  }];

  /** Says what went wrong for a moment; the page hears it at once. */
  function failed(id, text, error) {
    refuse(id, error);
    text.textContent = passkeyErrors[error]?.[1] ?? problems[error] ?? "MYLE could not do that just now.";
    setTimeout(() => prompted === id && closePrompt(), 2600);
  }

  /** Whether MYLE takes part for this site; else the browser's own passkeys. */
  async function passkeysHere(id, options, retry) {
    const listed = await send({ type: "passkeyList", rpId: options.rpId, allow: options.allow ?? [] });
    if (prompted !== id) return null;
    if (listed?.ok && Array.isArray(listed.passkeys)) return listed;
    if (listed?.error === "locked") {
      prompt(id, "Your MYLE vault is locked", "Unlock it in MYLE, then press Try again, to use the passkeys saved there.", [
        anotherDevice(id),
        ["Open MYLE", false, () => send({ type: "open" })],
        ["Try again", true, () => retry()],
      ]);
    } else {
      prompted = null;
      if (listed?.error === "rpMismatch") refuse(id, "rpMismatch");
      else answerPage(id, { result: "native" });
    }
    return null;
  }

  const busyLine = "If Windows Hello asks, confirm it is you.";

  async function offerNewPasskey(id, options) {
    const listed = await passkeysHere(id, options, () => offerNewPasskey(id, options));
    if (!listed) return;
    const who = options.userName || options.userDisplayName || "your account";
    prompt(id, `Save a passkey for ${listed.rpId}?`,
      `For ${who}, in your MYLE vault: it signs you in on every PC where you use MYLE.`, [
        cancel(id),
        anotherDevice(id),
        ["Save in MYLE", true, async (button, text) => {
          button.disabled = true;
          text.textContent = `Saving… ${busyLine}`;
          const answer = await send({
            type: "passkeyCreate",
            rpId: options.rpId,
            rpName: options.rpName,
            userId: options.userId,
            userName: options.userName,
            userDisplayName: options.userDisplayName,
            challenge: options.challenge,
            algorithms: options.algorithms,
            exclude: options.exclude,
            userVerification: options.userVerification,
          });
          if (prompted !== id) return;
          if (answer?.ok && answer.credential) {
            closePrompt();
            answerPage(id, { result: "credential", credential: answer.credential });
          } else if (answer?.error === "notSupported") {
            // The site takes no key MYLE makes: the browser's own then.
            closePrompt();
            answerPage(id, { result: "native" });
          } else {
            failed(id, text, answer?.error);
          }
        }],
      ]);
  }

  const usePasskey = (options, credentialId) => send({
    type: "passkeyGet",
    rpId: options.rpId,
    challenge: options.challenge,
    credentialId,
    userVerification: options.userVerification,
  });

  async function offerSignIn(id, options) {
    const listed = await passkeysHere(id, options, () => offerSignIn(id, options));
    if (!listed) return;
    if (!listed.passkeys.length) {
      prompted = null;
      return answerPage(id, { result: "native" });
    }
    const keys = document.createElement("div");
    keys.className = "keys";
    const text = prompt(id, `Sign in to ${listed.rpId}`, "With a passkey saved in MYLE:", [cancel(id), anotherDevice(id)], [keys]);
    for (const key of listed.passkeys.slice(0, 6)) {
      const button = item(key.userName || key.userDisplayName || "Passkey", key.title || listed.rpId, glyphAvatar("key"));
      button.addEventListener("click", async (event) => {
        if (prompted !== id || button.disabled || !genuine(event, bar, button)) return;
        for (const other of keys.querySelectorAll("button")) other.disabled = true;
        text.textContent = `Signing in… ${busyLine}`;
        const answer = await usePasskey(options, key.credentialId);
        if (prompted !== id) return;
        if (answer?.ok && answer.credential) {
          closePrompt();
          answerPage(id, { result: "credential", credential: answer.credential });
        } else {
          failed(id, text, answer?.error);
        }
      });
      keys.append(button);
    }
  }

  /** For the menu on a sign-in field: the passkeys a waiting sign-in form
   *  (mediation: "conditional") can use. */
  async function passkeyChoices() {
    const waiting = waitingPasskey;
    if (!waiting) return [];
    const listed = await send({ type: "passkeyList", rpId: waiting.options.rpId, allow: waiting.options.allow ?? [] });
    if (!listed?.ok || !Array.isArray(listed.passkeys) || waitingPasskey !== waiting) return [];
    return listed.passkeys.slice(0, 4).map((key) => {
      const button = item(key.userName || key.userDisplayName || "Passkey", `Passkey · ${listed.rpId}`, glyphAvatar("key"));
      button.addEventListener("click", async (event) => {
        if (!genuine(event, panel, button)) return;
        const field = anchor;
        hidePanel();
        if (waitingPasskey !== waiting) return showNote(field, "The sign-in form changed. Click the field again.");
        const answer = await usePasskey(waiting.options, key.credentialId);
        if (answer?.ok && answer.credential && waitingPasskey === waiting) {
          waitingPasskey = null;
          answerPage(waiting.id, { result: "credential", credential: answer.credential });
        } else {
          showNote(field, passkeyErrors[answer?.error]?.[1] ?? problems[answer?.error] ?? "MYLE could not sign in with that passkey.");
        }
      });
      return button;
    });
  }

  if (window === window.top) {
    addEventListener("message", (event) => {
      if (event.source !== window || event.data?.source !== FROM_PAGE) return;
      const { id, kind, options } = event.data;
      if (typeof id !== "string" || id.length > 80) return;
      if (kind === "abort") {
        if (waitingPasskey?.id === id) waitingPasskey = null;
        if (prompted === id) closePrompt();
        return;
      }
      if (!options || typeof options !== "object") return answerPage(id, { result: "native" });
      if (kind === "conditional") {
        waitingPasskey = { id, options };
        return;
      }
      // A newer request takes the place of the one on screen.
      if (prompted) refuse(prompted, "cancelled");
      prompted = id;
      if (kind === "create") void offerNewPasskey(id, options);
      else if (kind === "get") void offerSignIn(id, options);
      else {
        prompted = null;
        answerPage(id, { result: "native" });
      }
    });
  }

  // --- Offering to save -------------------------------------------------------

  /** What the user typed (or picked in the menu) into each password field. */
  const approved = new WeakMap();
  document.addEventListener(
    "input",
    (event) => {
      const field = event.composedPath()[0];
      if (event.isTrusted && field instanceof HTMLInputElement && field.type === "password") {
        approved.set(field, field.value);
      }
    },
    true,
  );

  /** In a sign-up or password-change form, the new password; else the one. */
  function sentPassword(fields) {
    const flagged = fields.find((el) => marked(el, "new-password"));
    if (flagged) return flagged;
    const [last, before] = [fields.at(-1), fields.at(-2)];
    return before && before.value === last.value ? last : fields[0];
  }

  let lastSent = "";

  function captureFrom(form) {
    const fields = queryAll(form ?? document, 'input[type="password"]').filter((el) => el.value);
    if (!fields.length) return;
    const password = sentPassword(fields);
    // Only what the user typed: a value a script put there is never offered.
    if (approved.get(password) !== password.value) return;
    const username = usernameFor(fields[0])?.value ?? "";
    // The click on the button and the form's submit are one sending.
    const key = `${username}\n${password.value}`;
    if (key === lastSent) return;
    lastSent = key;
    setTimeout(() => lastSent === key && (lastSent = ""), 5000);
    const sentFrom = location.href;
    void send({ type: "submitted", username, password: password.value }).then(() => {
      // A sign-in that stays on this page (a web app changing its own view):
      // once the form is gone or the address changed, the offer shows here.
      if (window !== window.top) return;
      setTimeout(() => {
        if (location.href !== sentFrom || !password.isConnected || !visible(password)) void offerSave();
      }, 1500);
    });
  }

  document.addEventListener(
    "submit",
    (event) => event.isTrusted && event.target instanceof HTMLFormElement && captureFrom(event.target),
    true,
  );
  // Many sign-in pages send with a script, not a form: a click on a submit
  // button, or Enter in the password field, counts too.
  document.addEventListener(
    "click",
    (event) => {
      if (!event.isTrusted) return;
      const button = event.composedPath().find(
        (el) => el instanceof HTMLButtonElement || (el instanceof HTMLInputElement && el.type === "submit"),
      );
      if (button && (button.type === "submit" || /log ?in|sign ?in|continue|next|σύνδεση|είσοδος/i.test(button.textContent ?? "")))
        captureFrom(button.form);
    },
    true,
  );

  let offerShown = "";

  async function offerSave() {
    const pending = await send({ type: "pending" });
    if (typeof pending?.nonce !== "string" || offerShown === pending.nonce) return;
    offerShown = pending.nonce;
    mount(null);
    const title = document.createElement("div");
    title.className = "title";
    title.innerHTML = MARK;
    title.append(pending.update ? "Update the saved password?" : "Save this login?");
    const text = document.createElement("p");
    text.textContent = `${pending.username || "A login without a user name"} for ${pending.site}, in your MYLE vault.` +
      (pending.locked ? " Your vault is locked: Save opens MYLE for you to unlock it." : "");
    const row = document.createElement("div");
    row.className = "row";
    const later = document.createElement("button");
    later.textContent = "Not now";
    later.addEventListener("click", (event) => {
      if (!event.isTrusted) return;
      hide(bar);
      void send({ type: "dismiss", nonce: pending.nonce });
    });
    const save = document.createElement("button");
    save.className = "primary";
    save.textContent = pending.update ? "Update" : "Save";
    save.addEventListener("click", async (event) => {
      if (save.disabled || !genuine(event, bar, save)) return;
      save.disabled = true;
      if (pending.locked) text.textContent = "Unlock your vault in MYLE: the login is saved as soon as it opens.";
      const answer = await send({ type: "save", nonce: pending.nonce });
      if (answer?.ok) {
        text.textContent = answer.already ? "It was already in your vault." :
          answer.updated ? "The saved password is updated." : "Saved to your vault.";
        row.remove();
        setTimeout(() => hide(bar), 2200);
      } else if (answer?.error === "expired") {
        text.textContent = "This offer has expired. Add the login in MYLE instead.";
        row.remove();
      } else {
        save.disabled = false;
        text.textContent = answer?.error === "locked" ?
          "Your vault is still locked. Unlock it in MYLE, then press Save again." :
          "It could not be saved. Try again, or add it in MYLE.";
      }
    });
    row.append(later, save);
    bar.replaceChildren(title, text, row);
    show(bar);
  }

  // The offer shows once, in the page itself, not in its frames.
  if (window === window.top) void offerSave();

  // --- 2FA keys on a setup page -----------------------------------------------
  //
  // A site turning on 2FA shows its key: a QR code, and mostly the key as text
  // too ("Can't scan it?"). MYLE offers to keep the key with the site's login,
  // so it fills the codes from then on. A QR code alone is read from the
  // toolbar popup.

  const SETUP_WORDS = /authenticator|two.?factor|two.?step|2fa|mfa|setup key|secret key|can.?t scan|scan (?:the|this) (?:qr|code)|enter (?:this|the) (?:key|code)|one.?time|totp/i;
  /** Words that go with a QR code to scan, in the key's own part of the page. */
  const SCAN_WORDS = /scan|authenticator|setup key|secret key|can.?t scan|manual|2fa|two.?factor|totp/i;
  const offeredKeys = new Set();
  let keyTimer = 0;
  let keyScans = 0;
  let keyOffers = 0;

  /** A 2FA key in `text`: an otpauth:// link, or the key written out. */
  function keyIn(text) {
    const link = text.match(/otpauth:\/\/totp\/[^\s"'<>]+/i)?.[0];
    if (link) return link;
    const plain = text.trim();
    if (plain.length < 16 || plain.length > 160) return null;
    const bare = plain.replace(/[\s-]/g, "").replace(/=+$/, "");
    if (bare.length < 16 || bare.length > 128 || !/^[A-Z2-7]+$/i.test(bare)) return null;
    // Letters alone and short: more likely a word or an id than a key; and
    // keys are written in one case.
    if (!/[2-7]/.test(bare) && bare.length < 26) return null;
    if (bare !== bare.toUpperCase() && bare !== bare.toLowerCase()) return null;
    return plain;
  }

  /** A picture shaped like a QR code: square, 90–600 px, and drawn (a
   *  canvas or svg) or named so (data: image, "qr" in its name). */
  function qrLike(el) {
    const r = el.getBoundingClientRect();
    if (r.width < 90 || r.width > 600 || Math.abs(r.width - r.height) > r.width * 0.12 || !visible(el)) return false;
    if (!(el instanceof HTMLImageElement)) return true;
    const names = `${getAttr(el, "src") ?? ""} ${el.alt} ${el.className} ${el.id}`.slice(0, 400);
    return /^data:image\//i.test(getAttr(el, "src") ?? "") || /qr|totp|2fa|authenticator|otp/i.test(names);
  }

  /** A 2FA setup is a QR code and its key in one part of the page, with
   *  words about scanning it; a key alone in a page about 2FA is not. */
  function besideQr(el) {
    let scope = el.parentElement;
    for (let depth = 0; scope && scope !== document.documentElement && depth < 6; depth++, scope = scope.parentElement) {
      const pictures = queryAll(scope, "img, canvas, svg");
      if (pictures.length > 40) return false;
      if (pictures.some(qrLike)) return SCAN_WORDS.test((scope.textContent ?? "").slice(0, 6000));
    }
    return false;
  }

  function findSetupKey() {
    if (!SETUP_WORDS.test((document.body?.textContent ?? "").slice(0, 200_000))) return null;
    // A QR code's own link, given as a link: nothing else needed.
    for (const link of queryAll(document, 'a[href^="otpauth:" i]')) {
      const key = keyIn(getAttr(link, "href") ?? "");
      if (key) return key;
    }
    for (const input of queryAll(document, "input, textarea")) {
      if ((input.readOnly || input.disabled) && visible(input)) {
        const key = keyIn(input.value);
        if (key && besideQr(input)) return key;
      }
    }
    // Written out, maybe a span per group of four.
    const candidates = queryAll(document, "code, kbd, samp, pre, strong, b, span, p, div, td, li, dd");
    for (const el of candidates.slice(0, 4000)) {
      if (el.childElementCount > 16) continue;
      const text = el.textContent ?? "";
      if (text.length < 16 || text.length > 160) continue;
      const key = keyIn(text);
      if (key && visible(el) && (key.startsWith("otpauth:") || besideQr(el))) return key;
    }
    return null;
  }

  /** "Not now": this key is not offered again on this site. */
  const notNow = (secret) => () => {
    closePrompt();
    void send({ type: "keyDismiss", secret });
  };

  /** Says how it went, then closes. */
  function settleKey(text, words) {
    text.textContent = words;
    setTimeout(closePrompt, 3200);
  }

  async function offerKey(secret) {
    const id = `totp-${++keyOffers}`;
    // Turned down before, or kept already: not asked again.
    const worth = await send({ type: "keyWorth", secret });
    if (worth?.offer === false) return;
    const answer = await send({ type: "logins" });
    if (prompted) return;
    if (!answer?.ok) {
      if (answer?.error === "locked") {
        prompt(id, "Keep this 2FA key in MYLE?", "Your vault is locked. Unlock it in MYLE, then press Try again.", [
          ["Not now", false, notNow(secret)],
          ["Open MYLE", false, () => send({ type: "open" })],
          ["Try again", true, () => {
            closePrompt();
            void offerKey(secret);
          }],
        ]);
      }
      return;
    }
    const logins = answer.logins ?? [];
    const named = accountsOnPage(logins, null);
    const exact = logins.filter((login) => login.exact);
    const choices = (named.length ? named : exact.length ? exact : logins).slice(0, 4);
    const save = async (login, text) => {
      text.textContent = "Saving…";
      const saved = await send({ type: "saveTotp", id: login?.id ?? null, secret });
      if (prompted !== id) return;
      if (saved?.ok) {
        settleKey(text, "Saved. When the site asks for a code, click its field: MYLE fills it.");
      } else {
        settleKey(text, problems[saved?.error] ?? "MYLE could not keep that key. Add it to the login in MYLE instead.");
      }
    };
    const why = "MYLE keeps it in your vault and fills this site's 2FA codes for you.";
    if (choices.length === 1) {
      const [login] = choices;
      prompt(id, "Use MYLE for the 2FA codes?",
        `${why} It goes with ${login.title || siteName} (${login.username || "no user name"})` +
          (login.totp ? ", in place of the key it has now." : "."), [
          ["Not now", false, notNow(secret)],
          ["Save", true, (button, text) => {
            button.disabled = true;
            return save(login, text);
          }],
        ]);
    } else if (choices.length > 1) {
      const list = document.createElement("div");
      list.className = "keys";
      const text = prompt(id, "Use MYLE for the 2FA codes?", `${why} Which login is it for?`, [["Not now", false, notNow(secret)]], [list]);
      for (const login of choices) {
        const title = login.title || login.site || "Login";
        const button = item(title, login.username || "No user name", avatarFor(login, title), login.totp ? "has a key" : "");
        button.addEventListener("click", (event) => {
          if (prompted !== id || button.disabled || !genuine(event, bar, button)) return;
          for (const other of list.querySelectorAll("button")) other.disabled = true;
          void save(login, text);
        });
        list.append(button);
      }
    } else {
      prompt(id, "Use MYLE for the 2FA codes?", `${why} No login for ${siteName} is in MYLE yet: the key is kept as a new one.`, [
        ["Not now", false, notNow(secret)],
        ["Save", true, (button, text) => {
          button.disabled = true;
          return save(null, text);
        }],
      ]);
    }
  }

  function scanForKey() {
    clearTimeout(keyTimer);
    keyTimer = setTimeout(() => {
      // A page that keeps changing is looked at a limited number of times.
      if (++keyScans > 80 || prompted || !bar.hidden) return;
      const key = findSetupKey();
      if (!key || offeredKeys.has(key)) return;
      offeredKeys.add(key);
      void offerKey(key);
    }, 1000);
  }

  if (window === window.top) {
    new MutationObserver(scanForKey).observe(document.documentElement, { childList: true, subtree: true, characterData: true });
    scanForKey();
  }
})();
