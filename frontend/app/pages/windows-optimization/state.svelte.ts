// The Tools tab's state: Auto-Logon and the restart to BIOS / UEFI.
import { isTauri } from "@tauri-apps/api/core";
import { tick } from "svelte";
import { confirm } from "../../../lib/confirm.svelte";
import { operationGate } from "../../../lib/operation-gate.svelte";
import { toast } from "../../../lib/toast.svelte";
import {
  windowsOptimizationApi,
  type AutoLogonOutcome,
  type AutoLogonOperation,
  type AutoLogonState,
  type FirmwareRestartOutcome,
  type WindowsOptimizationSnapshot,
} from "./api";
const POLL_MS = 1500;
function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
class WindowsOptimizationState {
  snapshot = $state<WindowsOptimizationSnapshot | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  autoLogonRequest = $state<AutoLogonOperation | null>(null);
  firmwareRestartRequest = $state(false);
  biosDialogOpen = $state(false);
  #pollTimer: ReturnType<typeof setTimeout> | null = null;
  readonly autoLogonBusy = $derived(
    this.autoLogonRequest !== null || isAutoLogonActive(this.snapshot?.autoLogon),
  );
  readonly firmwareRestartBusy = $derived(
    this.firmwareRestartRequest || this.snapshot?.firmwareRestart.active === true,
  );
  readonly ownBusy = $derived(this.autoLogonBusy || this.firmwareRestartBusy);
  readonly externallyLocked = $derived(operationGate.lockedFor("windows-optimization"));
  readonly locked = $derived(this.ownBusy || this.externallyLocked);
  async load() {
    if (!isTauri() || this.loading) return;
    this.loading = true;
    try {
      this.applySnapshot(await windowsOptimizationApi.getState());
      this.error = null;
    } catch (error) {
      this.error = message(error);
    } finally {
      this.loading = false;
    }
  }
  autoLogonStatus(): string {
    if (this.autoLogonRequest === "enable") return "Enabling";
    if (this.autoLogonRequest === "disable") return "Disabling";
    const state = this.snapshot?.autoLogon;
    if (!state) return this.loading ? "Checking" : "Unavailable";
    if (state.activeOperation === "enable") return "Enabling";
    if (state.activeOperation === "disable") return "Disabling";
    return state.status.charAt(0).toUpperCase() + state.status.slice(1);
  }
  firmwareRestartStatus(): "Ready" | "Unavailable" | "Processing" {
    if (this.firmwareRestartBusy) return "Processing";
    return this.snapshot?.firmwareRestart.available ? "Ready" : "Unavailable";
  }
  openBiosDialog() {
    if (this.locked || !this.snapshot?.firmwareRestart.available) return;
    this.biosDialogOpen = true;
  }
  dismissBiosDialog() {
    if (!this.firmwareRestartBusy) this.biosDialogOpen = false;
  }
  async restartToFirmware() {
    if (this.locked || !this.snapshot?.firmwareRestart.available) return;
    if (!operationGate.begin("windows-optimization")) {
      toast.info("Finish the current app task before restarting to BIOS / UEFI.");
      return;
    }
    this.firmwareRestartRequest = true;
    this.error = null;
    toast.info("Save your work now. Windows will request administrator approval next.");
    try {
      await tick();
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      const outcome = await windowsOptimizationApi.restartToFirmware();
      this.reportFirmwareRestartOutcome(outcome);
      if (outcome.result === "scheduled") this.biosDialogOpen = false;
    } catch (error) {
      toast.error(`Restart to BIOS / UEFI: ${message(error)}`);
    } finally {
      this.firmwareRestartRequest = false;
      operationGate.end("windows-optimization");
      await this.load();
    }
  }
  async setAutoLogon(enabled: boolean) {
    if (this.locked) return;
    const state = this.snapshot?.autoLogon;
    if (!state) return;
    const approved = await confirm(
      enabled
        ? {
            title: "Enable automatic Windows sign-in?",
            message:
              "Windows will sign in to this account automatically after startup. Anyone with physical access to this PC may be able to access the account. Windows will request the account password securely; a PIN or Windows Hello cannot be used.",
            confirmLabel: "Enable Auto-Logon",
            cancelLabel: "Cancel",
            danger: true,
          }
        : state.status === "conflict"
          ? {
              title: "Disable another Auto-Logon configuration?",
              message:
                `Auto-Logon is configured for ${state.configuredUser ?? "another Windows account"}. Disable that configuration and remove its stored sign-in secret?`,
              confirmLabel: "Disable Auto-Logon",
              cancelLabel: "Keep enabled",
              danger: true,
            }
          : {
              title: "Disable automatic Windows sign-in?",
              message:
                "Windows will require a normal sign-in again after startup. The stored Auto-Logon secret will be removed.",
              confirmLabel: "Disable Auto-Logon",
              cancelLabel: "Keep enabled",
            },
    );
    if (!approved || this.locked) return;
    if (!operationGate.begin("windows-optimization")) {
      toast.info("Finish the current app task before changing Auto-Logon.");
      return;
    }
    this.autoLogonRequest = enabled ? "enable" : "disable";
    this.error = null;
    try {
      const outcome = await windowsOptimizationApi.setAutoLogon(enabled);
      this.reportAutoLogonOutcome(outcome);
    } catch (error) {
      toast.error(`Windows Auto-Logon: ${message(error)}`);
    } finally {
      this.autoLogonRequest = null;
      operationGate.end("windows-optimization");
      await this.load();
    }
  }
  private applySnapshot(snapshot: WindowsOptimizationSnapshot) {
    this.snapshot = snapshot;
    if (isAutoLogonActive(snapshot.autoLogon) || snapshot.firmwareRestart.active) {
      operationGate.begin("windows-optimization");
      this.watchBackendOperation();
    } else if (!this.autoLogonRequest && !this.firmwareRestartRequest) {
      operationGate.end("windows-optimization");
    }
  }
  private watchBackendOperation() {
    if (this.#pollTimer) return;
    const poll = async () => {
      this.#pollTimer = null;
      if (
        (!isAutoLogonActive(this.snapshot?.autoLogon) && !this.snapshot?.firmwareRestart.active) ||
        this.autoLogonRequest ||
        this.firmwareRestartRequest
      ) return;
      try {
        const snapshot = await windowsOptimizationApi.getState();
        this.snapshot = snapshot;
        if (isAutoLogonActive(snapshot.autoLogon) || snapshot.firmwareRestart.active) {
          this.#pollTimer = setTimeout(poll, POLL_MS);
        } else {
          operationGate.end("windows-optimization");
        }
      } catch {
        this.#pollTimer = setTimeout(poll, POLL_MS);
      }
    };
    this.#pollTimer = setTimeout(poll, POLL_MS);
  }
  private reportAutoLogonOutcome(outcome: AutoLogonOutcome) {
    const suffix = outcome.note ? ` ${outcome.note}` : "";
    if (outcome.result === "enabled") {
      toast.success(`Windows Auto-Logon is enabled.${suffix}`);
    } else if (outcome.result === "disabled") {
      toast.success(`Windows Auto-Logon is disabled.${suffix}`);
    } else if (outcome.result === "cancelled") {
      toast.info("The Auto-Logon change was cancelled.");
    } else if (outcome.result === "needsAdmin") {
      toast.info("Administrator approval is required to change Auto-Logon.");
    } else if (outcome.result === "conflict") {
      toast.error(`A different Auto-Logon configuration is active.${suffix}`);
    } else {
      toast.error(`Auto-Logon is not available for this account or Windows configuration.${suffix}`);
    }
  }
  private reportFirmwareRestartOutcome(outcome: FirmwareRestartOutcome) {
    const suffix = outcome.note ? ` ${outcome.note}` : "";
    if (outcome.result === "scheduled") {
      toast.info(`Windows accepted the BIOS / UEFI restart request.${suffix}`);
    } else if (outcome.result === "cancelled") {
      toast.error(`Administrator approval was cancelled. Your PC was not restarted.${suffix}`);
    } else if (outcome.result === "needsAdmin") {
      toast.error(`Administrator approval is required to restart to BIOS / UEFI.${suffix}`);
    } else {
      toast.error(`Restart to BIOS / UEFI is unavailable on this system.${suffix}`);
    }
  }
}
function isAutoLogonActive(state: AutoLogonState | null | undefined): boolean {
  return state?.status === "working" || state?.activeOperation != null;
}
export const windowsOptimizationState = new WindowsOptimizationState();
