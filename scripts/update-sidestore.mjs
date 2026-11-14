// Points docs/sidestore.json at a release's IPA: the version entry of the source this release published to downloads.thomast.uk (scripts/mobile-feeds.mjs), so the copy SideStore reads from the repository is the same as the one it reads from R2 (the IPA's own versioned address there, with its size and SHA-256). Run by .github/workflows/mobile.yml: node scripts/update-sidestore.mjs out/sidestore.json
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
/* The repository's source with the generated one's newest version as its only one; everything else about the source is the repository's own. */
export function updatedSource(current, generated) {
  const fresh = generated?.apps?.[0]?.versions?.[0];
  if (!fresh?.version || !/^https:\/\//.test(fresh.downloadURL ?? "") || !/^[0-9a-f]{64}$/.test(fresh.sha256 ?? "") || !Number.isInteger(fresh.size) || fresh.size <= 0) {
    throw new Error("The generated SideStore source has no usable version (https address, size and SHA-256).");
  }
  const app = current?.apps?.[0];
  if (!app) throw new Error("docs/sidestore.json has no app.");
  return { ...current, apps: [{ ...app, versions: [{ ...fresh }] }, ...current.apps.slice(1)] };
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const generatedFile = process.argv[2];
  if (!generatedFile) throw new Error("Usage: node scripts/update-sidestore.mjs out/sidestore.json");
  const file = "docs/sidestore.json";
  const source = updatedSource(JSON.parse(readFileSync(file, "utf8")), JSON.parse(readFileSync(generatedFile, "utf8")));
  writeFileSync(file, JSON.stringify(source, null, 2) + "\n");
}
