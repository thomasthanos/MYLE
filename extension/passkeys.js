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
  const nativeToString = Function.prototype.toString;
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
      for (const message of earlyTraces.splice(0)) post({ ...message, event: `${message.event} (early)` }, origin);
      keepTraces = false;
      for (const [id, message] of early) {
        if (waiting.has(id)) post(message, origin);
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
    const resolve = waiting.get(event.data.id);
    if (resolve) {
      waiting.delete(event.data.id);
      resolve(event.data);
    }
  });

  /** Asks MYLE's part of the page; `signal` withdraws the question. */
  function ask(kind, options, signal) {
    return new Promise((resolve, reject) => {
      if (signal?.aborted) return reject(aborted(signal));
      const id = `${++next}-${Math.random().toString(36).slice(2)}`;
      waiting.set(id, resolve);
      const message = { source: PAGE, id, kind, options };
      if (!bridged) early.set(id, message);
      post(message, origin);
      listen.call(signal ?? new EventTarget(), "abort", () => {
        if (!waiting.delete(id)) return;
        post({ source: PAGE, id, kind: "abort" }, origin);
        reject(aborted(signal));
      }, { once: true });
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
        const details = kind === "create" ? forCreate(publicKey) : forGet(publicKey);
        trace(kind === "get" && options.mediation === "conditional" ? "conditional" : kind, {
          rpId: details.rpId,
          allow: details.allow?.length ?? 0,
          userVerification: details.userVerification,
          mediation: options.mediation ?? "",
        });
        if (kind === "get" && options.mediation === "conditional") return conditional(options, callNative);
        return ask(kind, details, options.signal).then((answer) => {
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
