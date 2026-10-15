// MYLE Passwords: passkeys. This part runs in the page's own world, before
// the page's scripts, so the site's navigator.credentials calls come here.
// They go on to MYLE's part of the page (content.js), where the user decides
// in MYLE's prompt; MYLE (the app) makes the answer for this page's own
// address. "Another device", or MYLE having nothing for this site, leaves
// the call to the browser as before (Windows Hello, a phone, a security key).
//
// The page's scripts can reach everything here, so nothing here is trusted:
// a page can only ask, as it always could; the user's click on MYLE's prompt
// and the address the browser reports decide the rest.
(() => {
  if (typeof CredentialsContainer !== "function" || typeof PublicKeyCredential !== "function") return;
  const PAGE = "myle-passkeys/page";
  const EXTENSION = "myle-passkeys/extension";
  // Kept before the page can replace them.
  const post = window.postMessage.bind(window);
  const listen = EventTarget.prototype.addEventListener;
  const unlisten = EventTarget.prototype.removeEventListener;
  const nativeToString = Function.prototype.toString;
  const later = typeof setTimeout === "function" ? setTimeout : null;
  const cancelLater = typeof clearTimeout === "function" ? clearTimeout : () => {};
  /** Our wrappers, each with the browser's own function it stands for: they
   *  read as that one ("function get() { [native code] }"), as other
   *  password managers' do, so a page that checks for tampering still
   *  hands its call to them. */
  const disguised = new WeakMap();

  /** A step, for MYLE's debug log (shown only with "Debug log" on in the
   *  toolbar popup): never a challenge, a key or a user name. */
  // Steps before MYLE's part of the page started, written once it has.
  const earlyTraces = [];
  let keepTraces = true;
  function trace(event, data = {}) {
    try {
      const message = { source: PAGE, kind: "trace", event, data };
      if (keepTraces && earlyTraces.length < 20) earlyTraces.push(message);
      post(message, location.origin === "null" ? "*" : location.origin);
    } catch {
      // A frame that cannot post: nothing to log.
    }
  }

  function disguise(wrapper, native) {
    disguised.set(wrapper, native);
    return wrapper;
  }

  /** MYLE only answers inputs it implements. Keep the full original request
   *  for the browser when a site needs another authenticator or extension. */
  function supportedRequest(publicKey, kind) {
    if (kind === "create" && String(publicKey.authenticatorSelection?.authenticatorAttachment) === "cross-platform") return false;
    const extensions = publicKey.extensions;
    if (!extensions) return true;
    const supported = kind === "create" ? "credProps" : "appid";
    for (const key of Object.getOwnPropertyNames(extensions)) {
      if (key !== supported && extensions[key] !== undefined) return false;
    }
    for (const key in extensions) {
      if (key !== supported && extensions[key] !== undefined) return false;
    }
    // Web IDL dictionary members can also be non-enumerable and inherited.
    for (const key of ["prf", "largeBlob", "uvm", "appidExclude", "credProps", "appid", "credProtect", "credentialProtectionPolicy", "enforceCredentialProtectionPolicy", "minPinLength", "hmacCreateSecret", "hmacGetSecret", "credBlob", "getCredBlob", "payment"]) {
      if (key !== supported && extensions[key] !== undefined) return false;
    }
    return true;
  }

  /** A frame can borrow the top page only with its own policy permission
   *  and an entirely same-origin ancestor chain. Missing policy data in a
   *  frame is ambiguous, so its original browser method handles the call. */
  function permitted(win, kind) {
    try {
      const framed = win !== win.top;
      if (framed) {
        if (win.origin === "null") return false;
        let ancestor = win;
        while (ancestor !== win.top) {
          const parent = ancestor.parent;
          if (!parent || parent === ancestor || parent.origin !== win.origin) return false;
          ancestor = parent;
        }
      }
      const policy = win.document?.permissionsPolicy ?? win.document?.featurePolicy;
      if (typeof policy?.allowsFeature !== "function") return !framed;
      return policy.allowsFeature(`publickey-credentials-${kind}`) === true;
    } catch {
      return false;
    }
  }

  if (!Object.prototype.hasOwnProperty.call(Function.prototype.toString, "__myle")) {
    const toString = {
      toString() {
        return nativeToString.call(disguised.get(this) ?? this);
      },
    }.toString;
    disguise(toString, nativeToString);
    Object.defineProperty(toString, "__myle", { value: true });
    Object.defineProperty(Function.prototype, "toString", { value: toString, writable: true, configurable: true, enumerable: false });
  }

  if (window !== window.top) {
    // A frame of the page's own origin (an empty helper frame a sign-in page
    // makes, as Google's does) asks the page itself, so MYLE answers there
    // too. Frames of any other origin keep the browser's own passkeys (and
    // only say so in the debug log).
    if (!delegateToTop()) watchOnly();
    return;
  }
  // Two copies installed (the store's, and one from a folder): one wraps.
  if (Object.prototype.hasOwnProperty.call(window, "__mylePasskeys")) return;
  Object.defineProperty(window, "__mylePasskeys", { value: true });
  const proto = CredentialsContainer.prototype;
  const nativeCreate = proto.create;
  const nativeGet = proto.get;
  const { origin } = location;
  const waiting = new Map();
  let next = 0;
  // Questions asked before MYLE's part of the page said hello.
  let bridged = false;
  const early = new Map();
  // MYLE's part of the page says it heard each question (newer copies do):
  // a question it never hears (its extension was updated or turned off) goes
  // to the browser's own passkeys instead of waiting forever.
  let acks = false;
  /** How long MYLE's part of the page has to say it heard a question. */
  const HEARD_WITHIN = 2000;
  /** How long a question asked before MYLE's part of the page started waits
   *  for it to start. */
  const START_WITHIN = 10_000;
  /** A modal request's deadline: the page's own timeout, within sane bounds,
   *  as the browser does. */
  const deadlineOf = (publicKey) => Math.min(Math.max(Number(publicKey.timeout) || 300_000, 30_000), 600_000);

  // Buffers can come from another frame's realm, where instanceof fails.
  const isArrayBuffer = (value) => Object.prototype.toString.call(value) === "[object ArrayBuffer]";
  const isBuffer = (value) => isArrayBuffer(value) || ArrayBuffer.isView(value);

  function b64(value) {
    const bytes = isArrayBuffer(value) ?
      new Uint8Array(value) :
      new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    let text = "";
    for (const byte of bytes) text += String.fromCharCode(byte);
    return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }

  function unb64(text) {
    const plain = atob(text.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(text.length / 4) * 4, "="));
    const bytes = new Uint8Array(plain.length);
    for (let i = 0; i < plain.length; i++) bytes[i] = plain.charCodeAt(i);
    return bytes.buffer;
  }

  const aborted = (signal) => signal.reason ?? new DOMException("The operation was aborted.", "AbortError");

  listen.call(window, "message", (event) => {
    if (event.source !== window || event.data?.source !== EXTENSION) return;
    if (event.data.kind === "hello") {
      // MYLE's part of the page starts only once the page has loaded; a
      // sign-in that asked before that (as Google's passkey step does) is
      // asked again now, instead of the question going unheard.
      bridged = true;
      acks = event.data.acks === true;
      for (const message of earlyTraces.splice(0)) post({ ...message, event: `${message.event} (early)` }, origin);
      keepTraces = false;
      for (const [id, message] of early) {
        if (!waiting.has(id)) continue;
        post(message, origin);
        waiting.get(id).heard();
      }
      trace("bridged", { early: early.size });
      early.clear();
      return;
    }
    if (event.data.kind === "ping") {
      // MYLE's part of the page started: say how things stand here.
      const container = navigator.credentials;
      return trace("ready", {
        wrapped: proto.get === wrappers.get && proto.create === wrappers.create,
        instance: !!container && container.get === wrappers.get,
        url: location.href.split(/[?#]/)[0],
      });
    }
    if (event.data.kind === "ack") {
      waiting.get(event.data.id)?.acked();
      return;
    }
    const question = waiting.get(event.data.id);
    if (question) {
      waiting.delete(event.data.id);
      question.answer(event.data);
    }
  });

  // Put in the back/forward cache, the page cannot see MYLE's prompt any more,
  // and MYLE has cancelled it: a modal request is refused, as the browser's own
  // would be, rather than found still waiting when the user comes back.
  listen.call(window, "pagehide", (event) => {
    if (!event.persisted) return;
    for (const question of [...waiting.values()]) {
      if (question.kind !== "conditional") question.fail(new DOMException("The operation either timed out or was not allowed.", "NotAllowedError"));
    }
  });

  /** Asks MYLE's part of the page; `signal` withdraws the question, and a
   *  modal request ends at `deadline` ms like the browser's own. A question
   *  that is never heard answers "native": the browser's own passkeys. */
  function ask(kind, options, signal, deadline = 0) {
    return new Promise((resolve, reject) => {
      if (signal?.aborted) return reject(aborted(signal));
      const id = `${++next}-${Math.random().toString(36).slice(2)}`;
      let heardTimer = null;
      let deadlineTimer = null;
      const stop = (timer) => timer !== null && cancelLater(timer);
      const done = () => {
        stop(heardTimer);
        stop(deadlineTimer);
        heardTimer = deadlineTimer = null;
        early.delete(id);
        if (signal) unlisten.call(signal, "abort", withdrawn);
      };
      /** Withdrawn by the page, or out of time: MYLE's prompt closes too. */
      const end = (error) => {
        if (!waiting.delete(id)) return;
        done();
        post({ source: PAGE, id, kind: "abort" }, origin);
        reject(error);
      };
      const withdrawn = () => end(aborted(signal));
      const unheard = () => {
        if (!waiting.delete(id)) return;
        done();
        trace("unheard", { kind });
        resolve({ result: "native" });
      };
      waiting.set(id, {
        kind,
        answer(data) {
          done();
          resolve(data);
        },
        fail: end,
        // Sent to a copy that says when it heard a question: it has a moment.
        heard() {
          stop(heardTimer);
          heardTimer = acks && later ? later(unheard, HEARD_WITHIN) : null;
        },
        acked() {
          stop(heardTimer);
          heardTimer = null;
        },
      });
      const message = { source: PAGE, id, kind, options };
      if (bridged) {
        post(message, origin);
        waiting.get(id).heard();
      } else {
        early.set(id, message);
        post(message, origin);
        // MYLE's part of the page never starts (turned off, or this is a
        // page it does not run in): the browser's own passkeys answer.
        if (later) heardTimer = later(() => !bridged && unheard(), START_WITHIN);
      }
      if (deadline && later) {
        deadlineTimer = later(() => end(new DOMException("The operation either timed out or was not allowed.", "NotAllowedError")), deadline);
      }
      if (signal) listen.call(signal, "abort", withdrawn, { once: true });
    });
  }

  /** What MYLE needs of `navigator.credentials.create()`'s options. */
  function forCreate(publicKey) {
    return {
      rpId: typeof publicKey.rp?.id === "string" ? publicKey.rp.id : null,
      rpName: String(publicKey.rp?.name ?? ""),
      userId: isBuffer(publicKey.user?.id) ? b64(publicKey.user.id) : "",
      userName: String(publicKey.user?.name ?? ""),
      userDisplayName: String(publicKey.user?.displayName ?? ""),
      challenge: isBuffer(publicKey.challenge) ? b64(publicKey.challenge) : "",
      algorithms: Array.isArray(publicKey.pubKeyCredParams) ?
        publicKey.pubKeyCredParams.map((param) => Number(param?.alg)).filter(Number.isFinite) : [],
      exclude: Array.isArray(publicKey.excludeCredentials) ?
        publicKey.excludeCredentials.filter((item) => isBuffer(item?.id)).map((item) => b64(item.id)) : [],
      userVerification: String(publicKey.authenticatorSelection?.userVerification ?? "preferred"),
      credProps: publicKey.extensions?.credProps === true,
    };
  }

  /** What MYLE needs of `navigator.credentials.get()`'s options. */
  function forGet(publicKey) {
    return {
      rpId: typeof publicKey.rpId === "string" ? publicKey.rpId : null,
      challenge: isBuffer(publicKey.challenge) ? b64(publicKey.challenge) : "",
      allow: Array.isArray(publicKey.allowCredentials) ?
        publicKey.allowCredentials.filter((item) => isBuffer(item?.id)).map((item) => b64(item.id)) : [],
      userVerification: String(publicKey.userVerification ?? "preferred"),
      // A site that still knows its old U2F keys asks for them by "appid":
      // a MYLE passkey never is one, which the answer says, as browsers do.
      appid: typeof publicKey.extensions?.appid === "string",
    };
  }

  const value = (data) => ({ value: data, enumerable: true });
  const method = (fn) => ({ value: fn });

  /** A PublicKeyCredential the page can use like the browser's own. */
  function credential(made, kind, extensionsAsked) {
    const r = made.response;
    const clientDataJSON = unb64(r.clientDataJSON);
    const authenticatorData = unb64(r.authenticatorData);
    let response;
    if (kind === "create") {
      const publicKey = unb64(r.publicKey);
      const transports = [...r.transports];
      response = Object.create(AuthenticatorAttestationResponse.prototype, {
        clientDataJSON: value(clientDataJSON),
        attestationObject: value(unb64(r.attestationObject)),
        getAuthenticatorData: method(() => authenticatorData.slice(0)),
        getPublicKey: method(() => publicKey.slice(0)),
        getPublicKeyAlgorithm: method(() => r.publicKeyAlgorithm),
        getTransports: method(() => [...transports]),
        toJSON: method(() => ({
          clientDataJSON: r.clientDataJSON,
          attestationObject: r.attestationObject,
          authenticatorData: r.authenticatorData,
          publicKey: r.publicKey,
          publicKeyAlgorithm: r.publicKeyAlgorithm,
          transports: [...transports],
        })),
      });
    } else {
      response = Object.create(AuthenticatorAssertionResponse.prototype, {
        clientDataJSON: value(clientDataJSON),
        authenticatorData: value(authenticatorData),
        signature: value(unb64(r.signature)),
        userHandle: value(r.userHandle ? unb64(r.userHandle) : null),
        toJSON: method(() => ({
          clientDataJSON: r.clientDataJSON,
          authenticatorData: r.authenticatorData,
          signature: r.signature,
          userHandle: r.userHandle ?? null,
        })),
      });
    }
    const extensions = {};
    if (kind === "create" && extensionsAsked?.credProps) extensions.credProps = { rk: true };
    if (kind === "get" && extensionsAsked?.appid) extensions.appid = false;
    return Object.create(PublicKeyCredential.prototype, {
      id: value(made.id),
      rawId: value(unb64(made.rawId)),
      type: value("public-key"),
      authenticatorAttachment: value(made.authenticatorAttachment ?? "platform"),
      response: value(response),
      getClientExtensionResults: method(() => ({ ...extensions })),
      toJSON: method(() => ({
        id: made.id,
        rawId: made.rawId,
        type: "public-key",
        authenticatorAttachment: made.authenticatorAttachment ?? "platform",
        response: response.toJSON(),
        clientExtensionResults: { ...extensions },
      })),
    });
  }

  /** MYLE's answer as the page expects it: a credential, an error, or the
   *  browser's own passkeys. */
  function settle(answer, kind, extensionsAsked, native) {
    if (answer?.result === "credential") return credential(answer.credential, kind, extensionsAsked);
    if (answer?.result === "error") {
      throw new DOMException(answer.message || "The operation either timed out or was not allowed.", answer.name || "NotAllowedError");
    }
    return native();
  }

  /** A sign-in form offering passkeys as the user types (mediation:
   *  "conditional"): MYLE's show in its menu on the user name field, next to
   *  the browser's own; whichever the user picks answers. */
  async function conditional(options, callNative) {
    const controller = new AbortController();
    const outer = options.signal;
    if (outer?.aborted) throw aborted(outer);
    listen.call(outer ?? new EventTarget(), "abort", () => controller.abort(outer.reason), { once: true });
    const details = forGet(options.publicKey);
    const ours = ask("conditional", details, controller.signal).then((answer) =>
      answer?.result === "credential" ? credential(answer.credential, "get", details) : new Promise(() => {}));
    // The browser's own: if it cannot offer any, MYLE's still can.
    const theirs = callNative({ ...options, signal: controller.signal }).catch((error) => {
      if (controller.signal.aborted) throw error;
      return new Promise(() => {});
    });
    try {
      return await Promise.race([ours, theirs]);
    } finally {
      controller.abort();
    }
  }

  const wrappers = {};

  function wrap(name, native, kind) {
    const wrapped = {
      [name](options) {
        const publicKey = options?.publicKey;
        // The page's own container, whatever it was called on (as other
        // password managers do): a bound or borrowed method counts too.
        const container = this instanceof CredentialsContainer ? this : navigator.credentials;
        if (!publicKey || typeof publicKey !== "object") return native.call(container, options);
        const callNative = (given = options) => native.call(container, given);
        if (!permitted(window, kind) || !supportedRequest(publicKey, kind)) return callNative();
        const details = kind === "create" ? forCreate(publicKey) : forGet(publicKey);
        trace(kind === "get" && options.mediation === "conditional" ? "conditional" : kind, {
          rpId: details.rpId,
          allow: details.allow?.length ?? 0,
          userVerification: details.userVerification,
          mediation: options.mediation ?? "",
        });
        if (kind === "get" && options.mediation === "conditional") return conditional(options, callNative);
        // Making a passkey without asking (conditional create) is the
        // browser's own business.
        if (kind === "create" && options.mediation === "conditional") return callNative();
        return ask(kind, details, options.signal, deadlineOf(publicKey)).then((answer) => {
          trace("answer", { kind, result: answer?.result ?? "none", name: answer?.name ?? "" });
          return settle(answer, kind, details, callNative);
        });
      },
    }[name];
    disguise(wrapped, native);
    wrappers[name] = wrapped;
    Object.defineProperty(proto, name, { value: wrapped, writable: true, configurable: true, enumerable: true });
  }

  wrap("create", nativeCreate, "create");
  wrap("get", nativeGet, "get");
  // The container itself too, as Bitwarden does: a page that reads the
  // method off navigator.credentials gets ours even if the prototype is
  // swapped later.
  try {
    const container = navigator.credentials;
    for (const name of ["create", "get"]) {
      Object.defineProperty(container, name, { value: wrappers[name], writable: true, configurable: true, enumerable: false });
    }
  } catch {
    // No container (an insecure page): nothing to wrap.
  }

  // MYLE can answer as this PC's own authenticator, so a site that asks
  // whether there is one (before offering "Use your passkey") hears yes.
  const nativeAvailable = PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable;
  if (typeof nativeAvailable === "function") {
    const available = {
      isUserVerifyingPlatformAuthenticatorAvailable() {
        return Promise.resolve(true);
      },
    }.isUserVerifyingPlatformAuthenticatorAvailable;
    disguise(available, nativeAvailable);
    Object.defineProperty(PublicKeyCredential, "isUserVerifyingPlatformAuthenticatorAvailable", {
      value: available, writable: true, configurable: true, enumerable: true,
    });
  }

  // A same-origin frame the page made and reached at once (before its own
  // copy of this script could run there): its passkey calls come here too.
  for (const property of typeof HTMLIFrameElement === "function" ? ["contentWindow", "contentDocument"] : []) {
    const type = HTMLIFrameElement;
    const descriptor = Object.getOwnPropertyDescriptor(type.prototype, property);
    if (!descriptor?.get) continue;
    const getter = {
      get() {
        const value = descriptor.get.call(this);
        try {
          const win = property === "contentWindow" ? value : value?.defaultView;
          if (win) adopt(win);
        } catch {
          // Another origin: left alone.
        }
        return value;
      },
    }.get;
    disguise(getter, descriptor.get);
    Object.defineProperty(type.prototype, property, { ...descriptor, get: getter });
  }

  /** Points a same-origin frame's passkey calls at this page's. */
  function adopt(win) {
    if (win === window || win.origin !== window.origin) return;
    if (Object.prototype.hasOwnProperty.call(win, "__mylePasskeys")) return;
    const frameProto = win.CredentialsContainer?.prototype;
    if (!frameProto) return;
    Object.defineProperty(win, "__mylePasskeys", { value: true });
    for (const name of ["create", "get"]) {
      const native = frameProto[name];
      const wrapped = {
        [name](options) {
          if (!options?.publicKey || typeof options.publicKey !== "object") return native.call(this, options);
          if (!permitted(win, name) || !supportedRequest(options.publicKey, name)) return native.call(this, options);
          trace("frame", { how: "adopted", kind: name });
          return wrappers[name].call(navigator.credentials, options);
        },
      }[name];
      disguise(wrapped, native);
      Object.defineProperty(frameProto, name, { value: wrapped, writable: true, configurable: true, enumerable: true });
    }
  }

  /** In a same-origin frame: navigator.credentials calls with publicKey go
   *  to the top page's (which MYLE wraps); everything else stays native. */
  function delegateToTop() {
    let top;
    try {
      top = window.top;
      // Exactly the same origin: not one merely reachable by document.domain.
      if (self.origin === "null" || self.origin !== top.origin || !top.navigator.credentials) return false;
    } catch {
      return false;
    }
    if (Object.prototype.hasOwnProperty.call(window, "__mylePasskeys")) return true;
    Object.defineProperty(window, "__mylePasskeys", { value: true });
    const proto = CredentialsContainer.prototype;
    for (const name of ["create", "get"]) {
      const native = proto[name];
      const wrapped = {
        [name](options) {
          if (!options?.publicKey || typeof options.publicKey !== "object") return native.call(this, options);
          if (!permitted(window, name) || !supportedRequest(options.publicKey, name)) return native.call(this, options);
          trace("frame", { how: "delegated", kind: name });
          return top.navigator.credentials[name](options);
        },
      }[name];
      disguise(wrapped, native);
      Object.defineProperty(proto, name, { value: wrapped, writable: true, configurable: true, enumerable: true });
    }
    return true;
  }

  /** A frame of another origin: the browser's own passkeys, as always; the
   *  debug log only notes that the site asked from there. */
  function watchOnly() {
    if (Object.prototype.hasOwnProperty.call(window, "__mylePasskeys")) return;
    Object.defineProperty(window, "__mylePasskeys", { value: true });
    const proto = CredentialsContainer.prototype;
    for (const name of ["create", "get"]) {
      const native = proto[name];
      const wrapped = {
        [name](options) {
          if (options?.publicKey) trace("otherOriginFrame", { kind: name, origin: self.origin });
          return native.call(this, options);
        },
      }[name];
      disguise(wrapped, native);
      Object.defineProperty(proto, name, { value: wrapped, writable: true, configurable: true, enumerable: true });
    }
  }
})();
