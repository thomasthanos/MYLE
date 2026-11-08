import { steamAutomationApi, type LibraryGame, type Settings, type Status } from "./api";

const POLL_MS = 5_000;
const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

class SteamAutomationState {
  status = $state<Status | null>(null);
  settings = $state<Settings | null>(null);
  library = $state<LibraryGame[] | null>(null);
  libraryLoading = $state(false);
  libraryError = $state<string | null>(null);
  signingIn = $state(false);
  paused = $state(false);
  error = $state<string | null>(null);
  #timer: ReturnType<typeof setInterval> | undefined;

  start() {
    void this.refresh();
    void this.#loadSettings();
    clearInterval(this.#timer);
    this.#timer = setInterval(() => void this.refresh(), POLL_MS);
  }

  stop() {
    clearInterval(this.#timer);
  }

  async refresh() {
    try {
      this.status = await steamAutomationApi.status();
      this.error = null;
    } catch (error) {
      this.error = messageOf(error);
    }
  }

  async #loadSettings() {
    try {
      this.settings = await steamAutomationApi.settings();
    } catch (error) {
      this.error = messageOf(error);
    }
  }

  async change(change: Partial<Settings>) {
    if (!this.settings) return;
    const next = { ...this.settings, ...change };
    this.settings = next;
    try {
      await steamAutomationApi.setSettings(next);
    } catch (error) {
      this.error = messageOf(error);
    }
  }

  async signIn() {
    this.signingIn = true;
    this.error = null;
    try {
      await steamAutomationApi.signIn();
      await this.refresh();
    } catch (error) {
      this.error = messageOf(error);
    } finally {
      this.signingIn = false;
    }
  }

  async setPaused(paused: boolean) {
    this.paused = paused;
    try {
      await steamAutomationApi.pause(paused);
    } catch (error) {
      this.error = messageOf(error);
    }
  }

  async loadLibrary() {
    if (this.library || this.libraryLoading) return;
    this.libraryLoading = true;
    this.libraryError = null;
    try {
      this.library = await steamAutomationApi.library();
    } catch (error) {
      this.libraryError = messageOf(error);
    } finally {
      this.libraryLoading = false;
    }
  }
}

export const steamAutomationState = new SteamAutomationState();
