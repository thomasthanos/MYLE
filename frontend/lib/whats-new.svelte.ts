// "What's new" after an update (backend/src/whats_new.rs): asked once at
// startup, and on request from Settings.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { toast } from "./toast.svelte";

export interface VersionNotes {
  version: string;
  /** Markdown; null when no notes could be found. */
  notes: string | null;
  source: "github" | "cache" | "bundled" | "none";
  url: string;
  date: string | null;
}

export interface WhatsNew {
  current: string;
  previous: string | null;
  /** Newest first. */
  versions: VersionNotes[];
  offline: boolean;
}

class WhatsNewState {
  shown = $state<WhatsNew | null>(null);
  loading = $state(false);
  /** Shown because of an update: closing it marks this version seen. */
  #afterUpdate = false;
  #asked = false;

  /** Once per run: the notes of the versions an update brought, if any. */
  async checkAfterStart() {
    if (!isTauri() || this.#asked) return;
    this.#asked = true;
    try {
      const pending = await invoke<WhatsNew | null>("whats_new_pending");
      if (pending && !this.shown) {
        this.#afterUpdate = true;
        this.shown = pending;
      }
    } catch {
      // Not worth a message: the notes are on GitHub, and Settings can show them.
    }
  }

  /** This version's notes (Settings). */
  async showCurrent() {
    if (!isTauri() || this.loading) return;
    this.loading = true;
    try {
      const notes = await invoke<WhatsNew>("whats_new_current");
      this.shown = notes;
    } catch (error) {
      toast.error(`Could not load what's new: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      this.loading = false;
    }
  }

  close() {
    if (this.#afterUpdate) void invoke("whats_new_seen").catch(() => {});
    this.#afterUpdate = false;
    this.shown = null;
  }
}

export const whatsNew = new WhatsNewState();
