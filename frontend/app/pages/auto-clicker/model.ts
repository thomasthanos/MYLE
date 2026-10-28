import type { Config, Profile, Status, Step } from "./api";
export const MAX_DELAY = 86_400_000;
export function newStep(): Step {
  return { id: crypto.randomUUID(), x: 0, y: 0, clicks: 50, button: "left", clickType: "single", intervalMs: 100, delayMs: 500 };
}
export function newProfile(name = "My clicks"): Profile {
  return { id: crypto.randomUUID(), name, config: { mode: "simple", button: "left", clickType: "single", intervalMs: 100, steps: [newStep()], loops: 50 } };
}
export function intervalInMs(value: number, unit: "ms" | "seconds"): number | null {
  const ms = value * (unit === "seconds" ? 1000 : 1);
  // Permit floating-point roundoff from decimal seconds, never round an actual fractional ms.
  const rounded = Math.round(ms);
  return Number.isFinite(ms) && Math.abs(ms - rounded) < 1e-7 ? rounded : null;
}
const integer = (n: number, low: number, high: number) => Number.isInteger(n) && n >= low && n <= high;
export function validateConfig(config: Config): string | null {
  if (!integer(config.intervalMs, 10, MAX_DELAY)) return "Set an interval between 10 ms and 24 hours.";
  if (config.loops !== null && !integer(config.loops, 1, 1_000_000)) return "Set a repeat count between 1 and 1,000,000.";
  if (config.steps.length > 128 || (config.mode === "sequence" && !config.steps.length)) return "Add at least one step (up to 128).";
  const ids = new Set<string>();
  for (const [i, step] of config.steps.entries()) {
    if (ids.has(step.id)) return "Each step needs a unique ID.";
    ids.add(step.id);
    if (!integer(step.x, -2_147_483_648, 2_147_483_647) || !integer(step.y, -2_147_483_648, 2_147_483_647)) return `Step ${i + 1}: choose whole-number screen coordinates.`;
    if (!integer(step.clicks, 1, 1_000_000)) return `Step ${i + 1}: set between 1 and 1,000,000 clicks.`;
    if (!integer(step.intervalMs, 10, MAX_DELAY)) return `Step ${i + 1}: set an interval between 10 ms and 24 hours.`;
    if (!integer(step.delayMs, 0, MAX_DELAY)) return `Step ${i + 1}: set a delay between 0 and 24 hours.`;
  }
  return null;
}
export function moveStep(steps: Step[], index: number, direction: number): Step[] {
  const next = index + direction;
  if (index < 0 || index >= steps.length || next < 0 || next >= steps.length) return steps;
  const result = [...steps];
  [result[index], result[next]] = [result[next], result[index]];
  return result;
}
export function progressText(status: Status): string {
  switch (status.phase) {
    case "countdown": return `Starting in ${status.countdown}…`;
    case "picking": return "Click a location on your screen";
    case "stopping": return "Stopping…";
    case "completed": return "Finished";
    case "stopped": return "Stopped";
    case "error": return "Could not continue";
    case "running": return status.mode === "sequence"
      ? `Step ${status.step || 1} of ${status.steps}: Click ${status.click}/${status.clicks}`
      : `${status.completed.toLocaleString()} ${status.completed === 1 ? "click" : "clicks"}${status.total === null ? "" : ` of ${status.total.toLocaleString()}`}`;
    default: return "Ready when you are";
  }
}
export function progressShare(status: Status): number | null {
  return status.total ? Math.min(1, Math.max(0, status.completed / status.total)) : null;
}
export function isBusy(status: Status | null): boolean {
  return !!status && ["countdown", "running", "stopping", "picking"].includes(status.phase);
}
