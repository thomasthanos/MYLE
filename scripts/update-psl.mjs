// Writes extension/psl.js: the Public Suffix List the extension uses to tell sites apart (a.github.io is not github.io's), the same list the app's `psl` crate carries, so both agree on what one site is. node scripts/update-psl.mjs (from the version in Cargo.lock; run `cargo fetch` first) node scripts/update-psl.mjs rules.txt (from a list you have)
import { readFile, readdir, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { domainToASCII } from "node:url";
async function crateRules() {
  const lock = await readFile(new URL("../backend/Cargo.lock", import.meta.url), "utf8");
  const version = lock.match(/name = "psl"\r?\nversion = "([^"]+)"/)?.[1];
  if (!version) throw new Error("psl is not in backend/Cargo.lock");
  const registry = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "registry", "src");
  for (const index of await readdir(registry)) {
    const path = join(registry, index, `psl-${version}`, "data", "rules.txt");
    try {
      return { text: await readFile(path, "utf8"), from: `psl ${version}` };
    } catch {
      // Another registry index.
    }
  }
  throw new Error(`psl ${version} is not downloaded: run \`cargo fetch\` in backend/`);
}
const { text, from } = process.argv[2]
  ? { text: await readFile(process.argv[2], "utf8"), from: process.argv[2] }
  : await crateRules();
const rules = new Set();
for (const line of text.split(/\r?\n/)) {
  const rule = line.trim().split(/\s/)[0];
  if (!rule || rule.startsWith("//")) continue;
  // Browsers give hosts in their ASCII form (xn--…), so the rules are too.
  const [, mark, name] = rule.match(/^(\*\.|!)?(.*)$/);
  const ascii = domainToASCII(name);
  if (!ascii) throw new Error(`Not a host name: ${rule}`);
  rules.add(`${mark ?? ""}${ascii}`);
}
const out = new URL("../extension/psl.js", import.meta.url);
await writeFile(
  out,
  `// The Public Suffix List (${from}), written by scripts/update-psl.mjs: do not edit.\n` +
    `// https://publicsuffix.org/, Mozilla Public License 2.0.\n` +
    `globalThis.MYLE_PUBLIC_SUFFIXES = new Set(${JSON.stringify([...rules].join(" "))}.split(" "));\n`,
);
console.log(`extension/psl.js: ${rules.size} rules from ${from}`);
