// The Tools tab: Auto-Logon and the restart to BIOS / UEFI.
import { invoke } from "@tauri-apps/api/core";
export type AutoLogonStatus =
  | "enabled"
  | "disabled"
  | "conflict"
  | "unavailable"
  | "working";
export type AutoLogonAccountType =
  | "local"
  | "microsoft"
  | "domain"
  | "entra"
  | "unknown";
export type AutoLogonOperation = "enable" | "disable";
export type AutoLogonResult =
  | "enabled"
  | "disabled"
  | "cancelled"
  | "needsAdmin"
  | "conflict"
  | "unsupported";
export type FirmwareType = "uefi" | "legacyBios" | "unknown";
export type FirmwareRestartResult =
  | "scheduled"
  | "cancelled"
  | "needsAdmin"
  | "unsupported";
export interface AutoLogonState {
  status: AutoLogonStatus;
  displayName: string;
  accountName: string;
  accountType: AutoLogonAccountType;
  configuredUser: string | null;
  ownedByApp: boolean;
  activeOperation: AutoLogonOperation | null;
  blockedReason: string | null;
}
export interface AutoLogonOutcome {
  result: AutoLogonResult;
  note: string | null;
}
export interface FirmwareRestartState {
  firmwareType: FirmwareType;
  available: boolean;
  blockedReason: string | null;
  active: boolean;
}
export interface FirmwareRestartOutcome {
  result: FirmwareRestartResult;
  note: string | null;
}
export interface WindowsOptimizationSnapshot {
  autoLogon: AutoLogonState;
  firmwareRestart: FirmwareRestartState;
}
export const windowsOptimizationApi = {
  getState: () =>
    invoke<WindowsOptimizationSnapshot>("windows_optimization_get_state"),
  setAutoLogon: (enabled: boolean) =>
    invoke<AutoLogonOutcome>("windows_optimization_set_auto_logon", { enabled }),
  restartToFirmware: () =>
    invoke<FirmwareRestartOutcome>("windows_optimization_restart_to_firmware"),
};
