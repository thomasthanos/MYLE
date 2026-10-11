// The account on the phone: only who is signed in. MYLE's settings are not
// synced here; the vault syncs through the Password Manager's own state.
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { accountApi, type Profile, type Provider } from "../app/account/api";
import { passwords } from "../app/pages/password-manager/state.svelte";
import { confirm } from "../lib/confirm.svelte";
import { toast } from "../lib/toast.svelte";
import { shell } from "./shell.svelte";
const PROVIDER_NAMES: Record<Provider, string> = { discord: "Discord", google: "Google" };
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
/** In a plain browser (`npx vite --mode mobile`): a pretend account. */
const preview = !isTauri();
/** Android shows the sign-in page in a Custom Tab over the app; iOS in a
 *  sheet that answers by itself. */
const android = /Android/i.test(navigator.userAgent);
/** Back in the app this long without the page coming back: it was closed. */
const CLOSED_AFTER_MS = 1500;
const previewProfile: Profile = { id: "preview", name: "Thomas", email: "thomas@example.com", avatarUrl: null, provider: "discord" };
class MobileAccount {
  #authEpoch = 0;
  profile = $state<Profile | null>(null);
  loaded = $state(false);
  /** The provider whose browser sign-in is being waited for. */
  signingIn = $state<Provider | null>(null);
  signingOut = $state(false);
  /** Tells the shell to return to the welcome screen after local cleanup. */
  resetVersion = $state(0);
  async init() {
    if (preview) {
      this.profile = new URLSearchParams(location.search).has("signed-out") ? null : previewProfile;
      this.loaded = true;
      return;
    }
    // The browser's return started the app again: the sign-in finished
    // without anyone waiting for it.
    await listen<Profile>("account-signed-in", (event) => void this.#signedIn(event.payload, null)).catch(() => {});
    this.profile = await accountApi.profile().catch(() => null);
    this.loaded = true;
  }
  async signIn(provider: Provider) {
    if (this.signingIn || this.signingOut) return;
    const epoch = ++this.#authEpoch;
    this.signingIn = provider;
    try {
      const profile = preview ? await new Promise<Profile>((resolve) => setTimeout(() => resolve(previewProfile), 800)) : await accountApi.signIn(provider);
      if (epoch !== this.#authEpoch) return;
      await this.#signedIn(profile, provider);
    } catch (error) {
      if (epoch !== this.#authEpoch) return;
      const text = message(error);
      if (!/cancelled/i.test(text)) toast.error(`${PROVIDER_NAMES[provider]} sign-in failed: ${text}`);
    } finally {
      if (epoch === this.#authEpoch) this.signingIn = null;
    }
  }
  async #signedIn(profile: Profile, provider: Provider | null) {
    if (this.signingOut) return;
    if (provider === null && !preview) {
      const epoch = this.#authEpoch;
      const saved = await accountApi.profile().catch(() => null);
      if (epoch !== this.#authEpoch || this.signingOut || saved?.id !== profile.id) return;
    }
    this.profile = profile;
    this.signingIn = null;
    const how = provider ? ` with ${PROVIDER_NAMES[provider]}` : "";
    toast.success(`Signed in${how} as ${profile.name ?? profile.email ?? "you"}.`);
    // Brings the account's vault here, or sends this phone's up.
    await passwords.syncNow();
  }
  cancelSignIn() {
    this.#authEpoch++;
    if (!preview) void accountApi.cancelSignIn();
    this.signingIn = null;
  }
  /** The app is in front again. On Android, back from the sign-in tab with
   *  no answer means the user closed it: the sign-in ends as cancelled. */
  resumed() {
    if (!android || !this.signingIn) return;
    const waiting = this.signingIn;
    setTimeout(() => {
      if (this.signingIn === waiting && document.visibilityState === "visible") this.cancelSignIn();
    }, CLOSED_AFTER_MS);
  }
  async signOut() {
    if (this.signingOut) return;
    this.signingOut = true;
    try {
      const ok = await confirm({
        title: "Sign out and clear this phone?",
        message: "Removes this phone's vault, saved account, website icons and biometric unlock. Your synced vault stays in your account. Changes that have not synced will be lost.",
        confirmLabel: "Sign out & clear", danger: true,
      });
      if (!ok) return;
      this.#authEpoch++;
      await passwords.lock();
      if (!preview) await accountApi.signOut();
      passwords.resetLocal();
      try { localStorage.clear(); } catch { /* Storage may be unavailable. */ }
      try { sessionStorage.clear(); } catch { /* Storage may be unavailable. */ }
      this.profile = null;
      this.signingIn = null;
      shell.accountOpen = false;
      this.resetVersion++;
      toast.info("Signed out. This phone's local data was cleared.");
    } catch (error) {
      toast.error(`Could not sign out: ${message(error)}`);
    } finally {
      this.signingOut = false;
    }
  }
}
export const account = new MobileAccount();
