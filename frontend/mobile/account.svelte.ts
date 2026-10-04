// The account on the phone: only who is signed in. MYLE's settings are not
// synced here; the vault syncs through the Password Manager's own state.
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { accountApi, type Profile, type Provider } from "../app/account/api";
import { passwords } from "../app/pages/password-manager/state.svelte";
import { confirm } from "../lib/confirm.svelte";
import { toast } from "../lib/toast.svelte";

const PROVIDER_NAMES: Record<Provider, string> = { discord: "Discord", google: "Google" };

function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** In a plain browser (`npx vite --mode mobile`): a pretend account. */
const preview = !isTauri();
const previewProfile: Profile = { id: "preview", name: "Thomas", email: "thomas@example.com", avatarUrl: null, provider: "discord" };

class MobileAccount {
  profile = $state<Profile | null>(null);
  loaded = $state(false);
  /** The provider whose browser sign-in is being waited for. */
  signingIn = $state<Provider | null>(null);

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
    if (this.signingIn) return;
    this.signingIn = provider;
    try {
      const profile = preview ? await new Promise<Profile>((resolve) => setTimeout(() => resolve(previewProfile), 800)) : await accountApi.signIn(provider);
      await this.#signedIn(profile, provider);
    } catch (error) {
      const text = message(error);
      if (!/cancelled/i.test(text)) toast.error(`${PROVIDER_NAMES[provider]} sign-in failed: ${text}`);
    } finally {
      this.signingIn = null;
    }
  }

  async #signedIn(profile: Profile, provider: Provider | null) {
    this.profile = profile;
    this.signingIn = null;
    const how = provider ? ` with ${PROVIDER_NAMES[provider]}` : "";
    toast.success(`Signed in${how} as ${profile.name ?? profile.email ?? "you"}.`);
    // Brings the account's vault here, or sends this phone's up.
    await passwords.syncNow();
  }

  cancelSignIn() {
    if (!preview) void accountApi.cancelSignIn();
    this.signingIn = null;
  }

  async signOut() {
    const ok = await confirm({
      title: "Sign out?",
      message: "Your vault stays on this phone, locked as before, but it no longer syncs with your PC until you sign in again.",
      confirmLabel: "Sign out",
    });
    if (!ok) return;
    try {
      if (!preview) await accountApi.signOut();
    } catch (error) {
      toast.error(`Could not sign out: ${message(error)}`);
      return;
    }
    this.profile = null;
    passwords.sync = { kind: "signedOut" };
    toast.info("Signed out.");
  }
}

export const account = new MobileAccount();
