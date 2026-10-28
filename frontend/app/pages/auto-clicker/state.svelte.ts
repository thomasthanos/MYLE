import { clickerApi, type Config, type Profiles, type Status, type Step } from "./api";
import { isBusy, moveStep, newProfile, newStep, validateConfig } from "./model";
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
export class AutoClickerState {
  profiles = $state.raw<Profiles | null>(null);
  status = $state.raw<Status | null>(null);
  loading = $state(false);
  loaded = $state(false);
  saving = $state(false);
  dirty = $state(false);
  pendingStart = $state(false);
  pickingId = $state<string | null>(null);
  error = $state<string | null>(null);
  saveError = $state<string | null>(null);
  #api: typeof clickerApi;
  #load: Promise<void> | null = null;
  #subscribed = false;
  #revision = 0;
  #configuration = Promise.resolve();
  #writes = Promise.resolve();
  #saveTimer: ReturnType<typeof setTimeout> | undefined;
  #pickGeneration = 0;
  #pickRequest: string | undefined;
  #startGeneration = 0;
  constructor(api = clickerApi) { this.#api = api; }
  get active() { return this.profiles?.items.find(p => p.id === this.profiles?.activeId) ?? null; }
  get busy() { return isBusy(this.status) || this.pendingStart || this.pickingId !== null; }
  get validation() { return this.active ? validateConfig(this.active.config) : null; }
  accept(status: Status) {
    if (this.status && status.revision < this.status.revision) return;
    this.status = status;
    if (status.phase === "stopping" && this.pickingId !== null) {
      this.#pickGeneration++;
      this.pickingId = null;
    }
  }
  load(): Promise<void> {
    if (this.loaded) return Promise.resolve();
    if (this.#load) return this.#load;
    this.loading = true;
    this.error = null;
    this.#load = (async () => {
      try {
        if (!this.#subscribed) { await this.#api.listen(s => this.accept(s)); this.#subscribed = true; }
        const [profiles, status] = await Promise.all([this.#api.load(), this.#api.status()]);
        this.profiles = profiles;
        this.accept(status);
        this.loaded = true;
        if (!this.busy) this.#prepare();
      } catch (error) { this.error = message(error); }
      finally { this.loading = false; this.#load = null; }
    })();
    return this.#load;
  }
  #prepare() {
    const config = this.active && !this.validation ? copy(this.active.config) : null;
    const revision = this.#revision;
    this.#configuration = this.#configuration.then(() => this.#api.configure(config)).catch(error => {
      if (revision === this.#revision && !this.busy) this.error = message(error);
    });
  }
  configured() { return this.#configuration; }
  #changed(profiles: Profiles) {
    this.profiles = profiles;
    this.#revision++;
    this.dirty = true;
    this.error = null;
    this.#prepare();
    clearTimeout(this.#saveTimer);
    if (!this.validation) this.#saveTimer = setTimeout(() => { void this.save(); }, 500);
  }
  edit(change: Partial<Config>) {
    if (!this.active || !this.profiles || this.busy) return;
    const id = this.active.id;
    this.#changed({ ...this.profiles, items: this.profiles.items.map(p => p.id === id ? { ...p, config: { ...p.config, ...change } } : p) });
  }
  updateStep(id: string, change: Partial<Step>) {
    if (!this.active) return;
    this.edit({ steps: this.active.config.steps.map(s => s.id === id ? { ...s, ...change, id: s.id } : s) });
  }
  addStep() { if (this.active && this.active.config.steps.length < 128) this.edit({ steps: [...this.active.config.steps, newStep()] }); }
  removeStep(id: string) { if (this.active) this.edit({ steps: this.active.config.steps.filter(s => s.id !== id) }); }
  moveStep(index: number, direction: number) { if (this.active) this.edit({ steps: moveStep(this.active.config.steps, index, direction) }); }
  selectProfile(id: string) {
    if (!this.profiles || this.busy || this.validation || !this.profiles.items.some(p => p.id === id)) return;
    this.#changed({ ...this.profiles, activeId: id });
  }
  addProfile() {
    if (!this.profiles || this.busy || this.validation || this.profiles.items.length >= 100) return;
    const profile = newProfile(`Profile ${this.profiles.items.length + 1}`);
    this.#changed({ ...this.profiles, activeId: profile.id, items: [...this.profiles.items, profile] });
  }
  renameProfile(name: string) {
    const trimmed = name.trim();
    if (!this.active || !this.profiles || this.busy || !trimmed || trimmed.length > 80 || /[\u0000-\u001f\u007f-\u009f]/u.test(trimmed)) return;
    this.#changed({ ...this.profiles, items: this.profiles.items.map(p => p.id === this.active?.id ? { ...p, name: trimmed } : p) });
  }
  removeProfile() {
    if (!this.active || !this.profiles || this.busy || this.profiles.items.length <= 1) return;
    const items = this.profiles.items.filter(p => p.id !== this.active?.id);
    this.#changed({ ...this.profiles, activeId: items[0].id, items });
  }
  async save() {
    clearTimeout(this.#saveTimer);
    if (!this.profiles || this.validation || !this.dirty) return;
    const snapshot = copy(this.profiles), revision = this.#revision;
    this.saving = true;
    this.#writes = this.#writes.then(async () => {
      try {
        await this.#api.save(snapshot);
        if (revision === this.#revision) { this.dirty = false; this.saveError = null; }
      } catch (error) { this.saveError = `Profiles were not saved: ${message(error)}`; }
    });
    await this.#writes;
    this.saving = false;
  }
  async start() {
    if (!this.active || this.busy || this.validation) return;
    const config = copy(this.active.config);
    const generation = ++this.#startGeneration;
    this.pendingStart = true;
    this.error = null;
    try {
      await this.configured();
      if (generation !== this.#startGeneration) return;
      const status = await this.#api.start(config);
      // Stop may have arrived while the Start command was in flight. Stop again
      // after its acknowledgement so command scheduling cannot restart the worker.
      this.accept(generation === this.#startGeneration ? status : await this.#api.stop());
    }
    catch (error) { this.error = message(error); }
    finally { this.pendingStart = false; }
  }
  async stop() {
    const pickRequest = this.#pickRequest;
    this.#pickRequest = undefined;
    this.#startGeneration++;
    this.#pickGeneration++;
    this.pickingId = null;
    try { this.accept(await this.#api.stop(pickRequest)); }
    catch (error) { this.error = message(error); }
  }
  async pick(id: string) {
    if (!this.active || this.busy) return;
    const profileId = this.active.id, generation = ++this.#pickGeneration;
    const requestId = crypto.randomUUID();
    this.#pickRequest = requestId;
    this.pickingId = id;
    this.error = null;
    try {
      const point = await this.#api.pick(requestId);
      if (point && generation === this.#pickGeneration && this.active?.id === profileId && this.profiles) {
        this.#changed({ ...this.profiles, items: this.profiles.items.map(p => p.id === profileId
          ? { ...p, config: { ...p.config, steps: p.config.steps.map(step => step.id === id ? { ...step, ...point } : step) } } : p) });
      }
    } catch (error) { if (generation === this.#pickGeneration) this.error = message(error); }
    finally {
      if (generation === this.#pickGeneration) this.pickingId = null;
      if (this.#pickRequest === requestId) this.#pickRequest = undefined;
    }
  }
}
export const clickerState = new AutoClickerState();
