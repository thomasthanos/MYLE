// The debloater's page state. It lives outside the components so switching
// tabs or pages keeps the choices and a run's progress.
//
// Nothing changes on the PC while the user chooses: a switch or a profile
// only records what they want (`desired`, `removing`), and "Review & apply"
// does it all at once, after a restore point.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { confirm } from "../../../../lib/confirm.svelte";
import { operationGate } from "../../../../lib/operation-gate.svelte";
import { toast } from "../../../../lib/toast.svelte";
import { debloatApi as api, type AppStatus, type DebloatEvent, type DebloatOutcome, type DebloatStatus, type StartMenuUpdate, type Step, type TweakStatus } from "./api";
import { pinCatalog } from "./catalog";
import { appPresetIds, fromLegacy, inProfile, isApplied, matchingProfile, pendingOf, prune, withProfile, type Profile } from "./selection";
const PENDING_KEY = "myle.debloat.pending";
/** Kept by older versions: the apps ticked for the one-click run, and the tweaks left out of it. */
const LEGACY_APPS_KEY = "myle.debloat.apps";
const LEGACY_SKIPPED_KEY = "myle.debloat.skipped";
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
interface SavedChoices {
  tweaks: Record<string, boolean>;
  apps: string[];
}
function loadChoices() {
  try {
    const raw = localStorage.getItem(PENDING_KEY);
    if (raw) {
      const saved = JSON.parse(raw) as Partial<SavedChoices>;
      const tweaks = Object.entries(saved.tweaks ?? {}).filter((entry): entry is [string, boolean] => typeof entry[1] === "boolean");
      const apps = Array.isArray(saved.apps) ? saved.apps.filter((id): id is string => typeof id === "string") : [];
      return { desired: new Map(tweaks), apps: new Set(apps) };
    }
    const legacy = localStorage.getItem(LEGACY_APPS_KEY);
    const value: unknown = legacy ? JSON.parse(legacy) : null;
    return fromLegacy(Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : null);
  } catch {
    return fromLegacy(null);
  }
}
function saveChoices(desired: ReadonlyMap<string, boolean>, apps: ReadonlySet<string>) {
  try {
    localStorage.setItem(PENDING_KEY, JSON.stringify({ tweaks: Object.fromEntries(desired), apps: [...apps] } satisfies SavedChoices));
    localStorage.removeItem(LEGACY_APPS_KEY);
    localStorage.removeItem(LEGACY_SKIPPED_KEY);
  } catch {
    // A preference only.
  }
}
/** Two runs' outcomes as one. */
function merged(a: DebloatOutcome, b: DebloatOutcome): DebloatOutcome {
  return { changed: a.changed + b.changed, failed: [...a.failed, ...b.failed], reboot: a.reboot || b.reboot, needsAdmin: a.needsAdmin || b.needsAdmin };
}
const nothing: DebloatOutcome = { changed: 0, failed: [], reboot: false, needsAdmin: false };
export type Tab = "quick" | "settings" | "apps" | "startMenu" | "tools";
const saved = loadChoices();
class DebloatState {
  tab = $state<Tab>("quick");
  status = $state<DebloatStatus | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  /** Tweaks to turn on (true) or off (false): only where that differs from now. */
  readonly desired = new SvelteMap<string, boolean>(saved.desired);
  /** Installed apps chosen for removal. */
  readonly removing = new SvelteSet<string>(saved.apps);
  readonly selectedPins = new SvelteSet<string>(pinCatalog.filter((item) => item.essential).map((item) => item.id));
  views = $state({
    settings: { query: "", filter: "all" },
    apps: { query: "", filter: "all" },
  });
  /** Settings groups the user opened (true) or folded (false); unset follows what is left to do. */
  opened = $state<Record<string, boolean>>({});
  /** The review of the pending changes is open. */
  reviewing = $state(false);
  busy = $state(false);
  startMenuBusy = $state(false);
  /** What the current (or last) run did, step by step. */
  steps = $state<Step[]>([]);
  /** How many steps the current run has, known before they start. */
  expected = $state(0);
  outcome = $state<DebloatOutcome | null>(null);
  /** The phase of a run: for the button and the progress header. */
  phase = $state<"idle" | "restorePoint" | "running" | "done">("idle");
  readonly locked = $derived(this.busy || this.startMenuBusy || operationGate.lockedFor("windows-optimization"));
  readonly tweaks = $derived(this.status?.tweaks ?? []);
  readonly installedApps = $derived((this.status?.apps ?? []).filter((app) => app.packages.length > 0));
  /** What "Review & apply" would do. */
  readonly pending = $derived(pendingOf(this.tweaks, this.desired));
  readonly pendingApps = $derived(this.installedApps.filter((app) => this.removing.has(app.id)));
  readonly pendingCount = $derived(this.pending.on.length + this.pending.off.length + this.pendingApps.length);
  /** The Quick setup profile the choices are exactly, if any. */
  readonly profile = $derived(matchingProfile(this.tweaks, this.status?.apps ?? [], this.desired, this.removing));
  /** Everything MYLE changed and can put back as it was. */
  readonly undoable = $derived(this.tweaks.filter((tweak) => tweak.canUndo));
  async load() {
    this.loading = true;
    try {
      this.status = await api.status();
      this.error = null;
      this.#tidy();
    } catch (error) {
      this.error = message(error);
    } finally {
      this.loading = false;
    }
  }
  /** Drops the choices that no longer change anything. */
  #tidy() {
    if (!this.status) return;
    const kept = prune(this.status.tweaks, this.status.apps, this.desired, this.removing);
    this.#replace(kept.desired, kept.apps);
  }
  #replace(desired: ReadonlyMap<string, boolean>, apps: ReadonlySet<string>) {
    this.desired.clear();
    for (const [id, on] of desired) this.desired.set(id, on);
    this.removing.clear();
    for (const id of apps) this.removing.add(id);
    saveChoices(this.desired, this.removing);
  }
  /** Whether a tweak's switch is on: the choice if there is one, else how it is now. */
  isOn(tweak: TweakStatus) {
    return this.desired.get(tweak.id) ?? isApplied(tweak);
  }
  /** A choice that is not how things are now. */
  isPending(tweak: TweakStatus) {
    return this.desired.has(tweak.id) && this.desired.get(tweak.id) !== isApplied(tweak);
  }
  setTweak(tweak: TweakStatus, on: boolean) {
    if (this.locked || tweak.state === "unavailable") return;
    if (on === isApplied(tweak)) this.desired.delete(tweak.id);
    else this.desired.set(tweak.id, on);
    saveChoices(this.desired, this.removing);
  }
  /** Turns on the recommended tweaks of one group, leaving the rest as chosen. */
  recommend(tweaks: readonly TweakStatus[]) {
    if (this.locked) return;
    for (const tweak of tweaks) {
      if (inProfile(tweak.level, "recommended") && !isApplied(tweak) && tweak.state !== "unavailable") this.desired.set(tweak.id, true);
    }
    saveChoices(this.desired, this.removing);
  }
  chooseProfile(profile: Profile) {
    if (this.locked || !this.status) return;
    const next = withProfile(this.status.tweaks, this.status.apps, this.desired, this.removing, profile);
    this.#replace(next.desired, next.apps);
  }
  /** Leaves these out of the choices (unticked in the review). */
  forget(tweakIds: readonly string[], appIds: readonly string[]) {
    for (const id of tweakIds) this.desired.delete(id);
    for (const id of appIds) this.removing.delete(id);
    saveChoices(this.desired, this.removing);
  }
  clearChoices() {
    if (this.locked) return;
    this.#replace(new Map(), new Set());
    this.reviewing = false;
  }
  isRemoving(id: string) {
    return this.removing.has(id);
  }
  setApp(id: string, on: boolean) {
    if (this.locked || !this.installedApps.some((app) => app.id === id)) return;
    if (on) this.removing.add(id);
    else this.removing.delete(id);
    saveChoices(this.desired, this.removing);
  }
  selectApps(which: "recommended" | "all" | "none") {
    if (this.locked || !this.status) return;
    this.removing.clear();
    for (const id of appPresetIds(this.status.apps, which)) this.removing.add(id);
    saveChoices(this.desired, this.removing);
  }
  togglePin(id: string) {
    if (this.locked || !this.status?.startMenu.supported) return;
    if (this.selectedPins.has(id)) this.selectedPins.delete(id);
    else this.selectedPins.add(id);
  }
  selectPins(preset: "essential" | "all" | "none") {
    if (this.locked || !this.status?.startMenu.supported) return;
    this.selectedPins.clear();
    for (const item of pinCatalog) {
      if (preset === "all" || (preset === "essential" && item.essential)) this.selectedPins.add(item.id);
    }
  }
  #onStep = (step: Step) => {
    const at = this.steps.findIndex((known) => known.id === step.id);
    if (at >= 0) this.steps[at] = step;
    else this.steps.push(step);
  };
  #onEvent = (event: DebloatEvent) => this.#onStep(event.data);
  /** Asks for a restore point first; false when the run should stop. */
  async #restorePoint(): Promise<boolean> {
    this.phase = "restorePoint";
    this.#onStep({ id: "restore-point", label: "Create a restore point", state: "running", detail: null });
    let result;
    try {
      result = await api.restorePoint(false);
      if (result.result === "protectionOff") {
        this.#onStep({ id: "restore-point", label: "Create a restore point", state: "running", detail: "System Protection is off" });
        if (
          await confirm({
            title: "System Protection is off",
            message:
              "Windows keeps no restore points for this PC. Turn System Protection on for the Windows drive, so a restore point can be created now? It uses a little disk space for restore points.",
            confirmLabel: "Turn it on",
            cancelLabel: "Not now",
          })
        ) {
          result = await api.restorePoint(true);
        }
      }
    } catch (error) {
      const text = message(error);
      this.#onStep({ id: "restore-point", label: "Create a restore point", state: "failed", detail: text });
      if (text.includes("declined")) {
        toast.error("Administrator approval was declined, so nothing was changed.");
        return false;
      }
      result = { result: "failed" as const, message: text };
    }
    if (result.result === "created") {
      this.#onStep({ id: "restore-point", label: "Create a restore point", state: "done", detail: null });
      return true;
    }
    const why = result.result === "failed" ? result.message : "System Protection is off.";
    this.#onStep({ id: "restore-point", label: "Create a restore point", state: "failed", detail: why });
    return confirm({
      title: "Continue without a restore point?",
      message: `The restore point could not be created: ${why}\n\nMYLE can still undo its own changes from the Settings tab.`,
      confirmLabel: "Continue",
      cancelLabel: "Stop",
    });
  }
  async #perform(work: () => Promise<DebloatOutcome>, withRestorePoint: boolean, steps: number) {
    if (this.locked || !operationGate.begin("windows-optimization")) {
      toast.error("Another app task is running. Try again when it finishes.");
      return;
    }
    this.busy = true;
    this.steps = [];
    this.outcome = null;
    this.expected = steps + (withRestorePoint ? 1 : 0);
    try {
      if (withRestorePoint && !(await this.#restorePoint())) {
        this.phase = "idle";
        return;
      }
      this.phase = "running";
      const outcome = await work();
      this.outcome = outcome;
      this.phase = "done";
      if (outcome.needsAdmin) toast.error("Administrator approval was declined, so machine-wide changes were not made.");
      else if (outcome.failed.length) toast.error(`${outcome.failed.length} step${outcome.failed.length === 1 ? "" : "s"} could not be completed.`);
      else toast.success(outcome.reboot ? "Done. Restart Windows to finish." : "Done.");
    } catch (error) {
      this.phase = "idle";
      toast.error(message(error));
    } finally {
      this.busy = false;
      operationGate.end("windows-optimization");
      await this.load();
    }
  }
  /** Asks about the tweaks that ask again (Edge); returns those still wanted. */
  async #confirmEach(tweaks: TweakStatus[]): Promise<TweakStatus[] | null> {
    const wanted: TweakStatus[] = [];
    for (const tweak of tweaks) {
      if (tweak.confirm && !(await confirm({ title: tweak.title, message: tweak.confirm, confirmLabel: "Yes, go ahead", cancelLabel: "Skip it", danger: true }))) {
        continue;
      }
      wanted.push(tweak);
    }
    return wanted;
  }
  /** Does every pending change: one restore point, then the tweaks and the
   *  apps, then the tweaks to turn off. */
  async applyPending() {
    if (this.locked || !this.pendingCount) return;
    const { on, off } = this.pending;
    const apps = this.pendingApps;
    this.reviewing = false;
    const wanted = await this.#confirmEach(on);
    if (!wanted) return;
    // Declined one by one (Edge): no longer chosen.
    for (const tweak of on) if (!wanted.includes(tweak)) this.desired.delete(tweak.id);
    saveChoices(this.desired, this.removing);
    const onIds = wanted.map((tweak) => tweak.id);
    const offIds = off.map((tweak) => tweak.id);
    const appIds = apps.map((app) => app.id);
    if (!onIds.length && !offIds.length && !appIds.length) return;
    const changesPc = onIds.length > 0 || appIds.length > 0;
    await this.#perform(
      async () => {
        let outcome = nothing;
        if (changesPc) outcome = merged(outcome, await api.run(onIds, appIds, this.#onEvent));
        if (offIds.length) outcome = merged(outcome, await api.undo(offIds, this.#onEvent));
        return outcome;
      },
      changesPc,
      onIds.length + offIds.length + appIds.length,
    );
  }
  async undo(tweaks: TweakStatus[]): Promise<boolean> {
    if (this.locked || !tweaks.length) return false;
    const single = tweaks.length === 1 ? tweaks[0] : null;
    const isUndo = single ? single.canUndo : true;
    const ok = await confirm({
      title: single ? `${isUndo ? "Undo" : "Turn off"} “${single.title}”?` : `Undo ${tweaks.length} changes?`,
      message: single
        ? isUndo
          ? "What MYLE changed is put back exactly as it was before."
          : "This setting will be switched back to the Windows default."
        : `Everything MYLE changed is put back exactly as it was before:\n${tweaks.map((tweak) => `• ${tweak.title}`).join("\n")}`,
      confirmLabel: isUndo ? "Undo" : "Turn off",
    });
    if (!ok) return false;
    await this.#perform(() => api.undo(tweaks.map((tweak) => tweak.id), this.#onEvent), false, tweaks.length);
    return true;
  }
  async openStore(app: AppStatus) {
    try {
      await api.openStore(app.id);
    } catch (error) {
      toast.error(message(error));
    }
  }
  async setStartMenu(update: StartMenuUpdate) {
    if (this.locked || !this.status?.startMenu.supported || !operationGate.begin("windows-optimization")) return;
    this.startMenuBusy = true;
    try {
      const next = await api.startMenuSet(update);
      if (this.status) this.status.startMenu = next;
    } catch (error) {
      toast.error(message(error));
    } finally {
      this.startMenuBusy = false;
      operationGate.end("windows-optimization");
    }
  }
  async setHideRecommended(hide: boolean) {
    if (this.locked || !this.status?.startMenu.supported || !operationGate.begin("windows-optimization")) return;
    this.startMenuBusy = true;
    try {
      const next = await api.startMenuHideRecommended(hide);
      if (this.status) this.status.startMenu = next;
      toast.success(hide ? "Recommended section hidden." : "Recommended section restored.");
    } catch (error) {
      toast.error(message(error));
    } finally {
      this.startMenuBusy = false;
      operationGate.end("windows-optimization");
    }
  }
  async applyStartPins(pins: string[], label: string) {
    if (this.locked || !this.status?.startMenu.supported) return;
    const ok = await confirm({
      title: `${label}?`,
      message:
        pins.length === 0
          ? "Your current Start Menu pins will be backed up first, then all pinned apps will be cleared from the Start Menu."
          : "Your current Start Menu pins will be backed up first, then the Start Menu will be updated with your selected apps.",
      confirmLabel: "Apply",
    });
    if (!ok) return;
    if (this.locked || !operationGate.begin("windows-optimization")) return;
    this.startMenuBusy = true;
    try {
      const next = await api.startMenuApplyPins(pins);
      if (this.status) this.status.startMenu = next;
      toast.success("Start Menu pinned apps updated.");
    } catch (error) {
      toast.error(message(error));
    } finally {
      this.startMenuBusy = false;
      operationGate.end("windows-optimization");
    }
  }
  async restoreStartPins() {
    if (this.locked || !this.status?.startMenu.supported || !operationGate.begin("windows-optimization")) return;
    this.startMenuBusy = true;
    try {
      const next = await api.startMenuRestorePins();
      if (this.status) this.status.startMenu = next;
      toast.success("Previous Start Menu pins restored.");
    } catch (error) {
      toast.error(message(error));
    } finally {
      this.startMenuBusy = false;
      operationGate.end("windows-optimization");
    }
  }
}
export const debloat = new DebloatState();
