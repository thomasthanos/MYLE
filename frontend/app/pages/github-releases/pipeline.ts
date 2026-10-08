// How far a build got through the steps its script declares.

/**
 * Whether a build stage ("App 9.7.2") is the pipeline step `title` ("App",
 * from `Invoke-Step "App $version"`): the same words, maybe followed by a
 * version or number, not by more words ("Setup window" is not "Setup").
 */
export function stageIsStep(stage: string, title: string): boolean {
  const a = stage.trim().toLowerCase();
  const b = title.trim().toLowerCase();
  if (!b) return false;
  if (a === b) return true;
  return a.startsWith(`${b} `) && !/^[a-z]/.test(a.slice(b.length + 1));
}

/** How far a build got through `titles`: the index of the last step reached, or -1. */
export function stepReached(stages: string[], titles: string[]): number {
  let at = -1;
  for (const stage of stages) {
    for (let j = Math.max(at, 0); j < titles.length; j++) {
      if (stageIsStep(stage, titles[j])) {
        at = j;
        break;
      }
    }
  }
  return at;
}
