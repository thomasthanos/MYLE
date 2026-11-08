import { invoke } from "@tauri-apps/api/core";

export interface Status {
  steamRunning: boolean;
  communitySignedIn: boolean;
  sameAccount: boolean;
}

export const steamAutomationApi = {
  status: () => invoke<Status>("steam_automation_status"),
  signIn: () => invoke<void>("steam_automation_sign_in"),
};
