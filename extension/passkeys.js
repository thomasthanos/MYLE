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
  if (window !== window.top) {
    // A frame of the page's own origin (an empty helper frame a sign-in page
    // makes, as Google's does) asks the page itself, so MYLE answers there
    // too. Frames of any other origin keep the browser's own passkeys.
    delegateToTop();
    return;
  }
  // Two copies installed (the store's, and one from a folder): one wraps.
  if (Object.prototype.hasOwnProperty.call(window, "__mylePasskeys")) return;
  Object.defineProperty(window, "__mylePasskeys", { value: true });
  const PAGE = "myle-passkeys/page";
  const EXTENSION = "myle-passkeys/extension";
  const proto = CredentialsContainer.prototype;
  const nativeCreate = proto.create;
  const nativeGet = proto.get;
  // Kept before the page can replace them.
  const post = window.postMessage.bind(window);
  const listen = EventTarget.prototype.addEventListener;
  const { origin } = location;
  const waiting = new Map();
  let next = 0;

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
      post({ source: PAGE, id, kind, options }, origin);
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

  function wrap(name, native, kind) {
    const wrapped = {
      [name](options) {
        const publicKey = options?.publicKey;
        if (this !== navigator.credentials || !publicKey || typeof publicKey !== "object") {
          return native.call(this, options);
        }
        const callNative = (given = options) => native.call(this, given);
        if (kind === "get" && options.mediation === "conditional") return conditional(options, callNative);
        const details = kind === "create" ? forCreate(publicKey) : forGet(publicKey);
        return ask(kind, details, options.signal).then((answer) =>
          settle(answer, kind, details, callNative));
      },
    }[name];
    Object.defineProperty(proto, name, { value: wrapped, writable: true, configurable: true, enumerable: true });
  }

  wrap("create", nativeCreate, "create");
  wrap("get", nativeGet, "get");

  /** In a same-origin frame: navigator.credentials calls with publicKey go
   *  to the top page's (which MYLE wraps); everything else stays native. */
  function delegateToTop() {
    let top;
    try {
      top = window.top;
      // Exactly the same origin: not one merely reachable by document.domain.
      if (self.origin === "null" || self.origin !== top.origin || !top.navigator.credentials) return;
    } catch {
      return;
    }
    if (Object.prototype.hasOwnProperty.call(window, "__mylePasskeys")) return;
    Object.defineProperty(window, "__mylePasskeys", { value: true });
    const proto = CredentialsContainer.prototype;
    for (const name of ["create", "get"]) {
      const native = proto[name];
      const wrapped = {
        [name](options) {
          if (this !== navigator.credentials || !options?.publicKey || typeof options.publicKey !== "object") {
            return native.call(this, options);
          }
          const container = top.navigator.credentials;
          return container[name](options);
        },
      }[name];
      Object.defineProperty(proto, name, { value: wrapped, writable: true, configurable: true, enumerable: true });
    }
  }
})();
