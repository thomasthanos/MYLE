import { cloudStorageApi as api, PROVIDERS, type CloudOutcome, type CloudProgress, type CloudProvider, type CloudStatus } from "./api";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const initial = (): CloudProgress => ({ visited: 0, requested: 0, alreadyOnline: 0, pendingUploads: 0, skipped: 0, failed: 0, current: null });

class CloudStorageState {
  providers = $state.raw<CloudStatus[]>(PROVIDERS.map((provider) => ({ ...provider, folders: [], mode: "unavailable", detail: "Cloud folders have not been checked yet." })));
  loading = $state(false);
  loadError = $state<string | null>(null);
  selected = $state<CloudProvider | null>(null);
  running = $state(false);
  cancelling = $state(false);
  requestId = $state<string | null>(null);
  progress = $state.raw<CloudProgress | null>(null);
  result = $state.raw<CloudOutcome | null>(null);
  error = $state<string | null>(null);
  current = $derived(this.providers.find((provider) => provider.provider === this.selected) ?? null);
  #loadSequence = 0;

  async load() {
    if (this.running) return;
    const sequence = ++this.#loadSequence;
    this.loading = true;
    try {
      const status = await api.status();
      if (sequence !== this.#loadSequence) return;
      this.providers = status.providers;
      this.loadError = null;
    } catch (error) {
      if (sequence === this.#loadSequence) this.loadError = message(error);
    } finally {
      if (sequence === this.#loadSequence) this.loading = false;
    }
  }

  choose(provider: CloudProvider) {
    if (this.running) return;
    this.selected = provider;
    this.result = null;
    this.progress = null;
    this.error = null;
  }

  close() {
    if (!this.running) this.selected = null;
  }

  async makeOnlineOnly() {
    const provider = this.current;
    if (this.running || this.loading || this.loadError || !provider || provider.mode !== "native" || !provider.folders.length) return;
    const requestId = crypto.randomUUID();
    const folders = provider.folders.map((folder) => folder.path);
    this.requestId = requestId;
    this.running = true;
    this.cancelling = false;
    this.result = null;
    this.error = null;
    this.progress = initial();
    try {
      const result = await api.onlineOnly(provider.provider, folders, requestId, (progress) => {
        if (this.requestId === requestId && this.running) this.progress = progress;
      });
      if (this.requestId === requestId) this.result = result;
    } catch (error) {
      if (this.requestId === requestId) this.error = message(error);
    } finally {
      if (this.requestId === requestId) {
        this.running = false;
        this.cancelling = false;
        this.requestId = null;
      }
    }
  }

  async cancel() {
    const id = this.requestId;
    if (!id || !this.running || this.cancelling) return;
    this.cancelling = true;
    try {
      await api.cancel(id);
    } catch (error) {
      if (this.requestId === id) {
        this.error = message(error);
        this.cancelling = false;
      }
    }
  }

  async openFolder(path: string) {
    if (!this.current || this.running) return;
    try { await api.openFolder(this.current.provider, path); }
    catch (error) { this.error = message(error); }
  }

  async help() {
    if (!this.current) return;
    try { await api.openHelp(this.current.provider); }
    catch (error) { this.error = message(error); }
  }
}

// Keep an active request and its cancel control when the user changes pages.
export const cloudStorage = new CloudStorageState();
