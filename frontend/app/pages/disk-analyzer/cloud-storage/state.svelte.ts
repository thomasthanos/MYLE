import { cloudStorageApi as api, PROVIDERS, type CloudOutcome, type CloudProgress, type CloudStatus } from "./api";
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const initial = (): CloudProgress => ({ visited: 0, requested: 0, alreadyOnline: 0, pendingUploads: 0, skipped: 0, failed: 0, foldersRequested: 0, foldersAlreadyOnline: 0, current: null, waitingForDrive: false });
class CloudStorageState {
  providers = $state.raw<CloudStatus[]>(PROVIDERS.map((provider) => ({ ...provider, folders: [], mode: "unavailable", detail: "Cloud folders have not been checked yet." })));
  loading = $state(false);
  loadError = $state<string | null>(null);
  running = $state(false);
  cancelling = $state(false);
  requestId = $state<string | null>(null);
  progress = $state.raw<CloudProgress | null>(null);
  result = $state.raw<CloudOutcome | null>(null);
  error = $state<string | null>(null);
  /** The apps whose files the button makes online-only. */
  ready = $derived(this.providers.filter((provider) => provider.mode === "native" && provider.folders.length));
  /** The apps on this PC that need their own controls. */
  left = $derived(this.providers.filter((provider) => provider.mode === "guided"));
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
  /* One request for every ready app, with the folders checked last: the backend refuses it if they changed meanwhile. */
  async makeOnlineOnly() {
    const ready = this.ready;
    if (this.running || this.loading || this.loadError || !ready.length) return;
    const clouds = ready.map((provider) => provider.provider);
    const folders = ready.flatMap((provider) => provider.folders.map((folder) => folder.path));
    const requestId = crypto.randomUUID();
    this.requestId = requestId;
    this.running = true;
    this.cancelling = false;
    this.result = null;
    this.error = null;
    this.progress = initial();
    try {
      const result = await api.onlineOnly(clouds, folders, requestId, (progress) => {
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
}
// Keep an active request and its Stop control when the user changes pages.
export const cloudStorage = new CloudStorageState();
