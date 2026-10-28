/* Orders two versions by Semantic Versioning precedence: build metadata (`+…`) is ignored, and a pre-release (`9.0.0-beta.1`) comes before its release. Negative when `a` is older, positive when newer, 0 when equal. */
export function compareVersions(a: string, b: string): number {
  const [x, y] = [parse(a), parse(b)];
  for (let i = 0; i < Math.max(x.core.length, y.core.length); i++) {
    const diff = (x.core[i] ?? 0) - (y.core[i] ?? 0);
    if (diff) return Math.sign(diff);
  }
  // A release is newer than any of its pre-releases.
  if (!x.pre.length || !y.pre.length) return Math.sign(y.pre.length - x.pre.length);
  for (let i = 0; i < Math.min(x.pre.length, y.pre.length); i++) {
    const [p, q] = [x.pre[i], y.pre[i]];
    const [pNumber, qNumber] = [/^\d+$/.test(p), /^\d+$/.test(q)];
    if (pNumber && qNumber) {
      const diff = Number(p) - Number(q);
      if (diff) return Math.sign(diff);
    } else if (pNumber !== qNumber) {
      // Numbers come before words.
      return pNumber ? -1 : 1;
    } else if (p !== q) {
      return p < q ? -1 : 1;
    }
  }
  return Math.sign(x.pre.length - y.pre.length);
}
function parse(version: string) {
  const plain = version.trim().replace(/^v/i, "").split("+")[0];
  const dash = plain.indexOf("-");
  const core = (dash < 0 ? plain : plain.slice(0, dash)).split(".").map((part) => Number.parseInt(part, 10) || 0);
  const pre = dash < 0 ? [] : plain.slice(dash + 1).split(".").filter(Boolean);
  return { core, pre };
}
