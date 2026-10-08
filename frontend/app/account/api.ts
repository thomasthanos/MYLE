// Typed bridge to the account commands in backend/src/account/.
import { invoke } from "@tauri-apps/api/core";

export type Provider = "discord" | "google";

/** Sent by the backend when the owner-only pages open or close. */
export const ACCESS_EVENT = "account-access";

export interface Profile {
  id: string;
  name: string | null;
  email: string | null;
  avatarUrl: string | null;
  /** "discord" or "google", as Supabase reports it. */
  provider: string | null;
}

export const accountApi = {
  profile: () => invoke<Profile | null>("account_profile"),
  /** Whether the owner-only pages are open, by the backend's check with the
   *  account server (re-checked when `recheck` or a few minutes old). */
  access: (recheck = false) => invoke<boolean>("account_access", { recheck }),
  /** Resolves once the browser returns; rejects on cancel, timeout or refusal. */
  signIn: (provider: Provider) => invoke<Profile>("account_sign_in", { provider }),
  cancelSignIn: () => invoke<void>("account_cancel_sign_in"),
  signOut: () => invoke<void>("account_sign_out"),
  pull: () => invoke<unknown>("account_pull"),
  push: (settings: unknown) => invoke<void>("account_push", { settings }),
};
