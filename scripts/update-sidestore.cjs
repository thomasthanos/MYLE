// Points docs/sidestore.json at a release's IPA, with its notes as SideStore's
// "what is new". Run by .github/workflows/mobile.yml:
//   VERSION=10.0.2 TAG=v10.0.2 node scripts/update-sidestore.cjs <ipa size in bytes>
const fs = require("fs");
const file = "docs/sidestore.json";
const source = JSON.parse(fs.readFileSync(file, "utf8"));
const app = source.apps[0];
const old = app.versions[0] || {};
// What is new, as SideStore shows it: the release notes in plain
// text (no markdown or HTML), kept short.
const tag = process.env.TAG;
const candidates = tag.startsWith("mobile-v")
  ? [`docs/release-notes/mobile-${process.env.VERSION}.md`]
  : [`docs/release-notes/mobile-${process.env.VERSION}.md`, `docs/release-notes/${process.env.VERSION}.md`];
const notesFile = candidates.find((f) => fs.existsSync(f));
let notes = notesFile ? fs.readFileSync(notesFile, "utf8") : "";
notes = notes
  .replace(/<[^>]+>/g, "")
  .replace(/`([^`]*)`/g, "$1")
  .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
  .replace(/\*\*|__|\*/g, "")
  .split("\n")
  .map((line) => line.replace(/^#+\s*/, "").replace(/^\s*[-+]\s+/, "• ").trimEnd())
  .filter((line, i, all) => line || (all[i - 1] ?? "") !== "")
  .join("\n")
  .trim();
if (notes.length > 1200) notes = notes.slice(0, 1200).replace(/\s+\S*$/, "") + "…";
if (!notes) notes = `MYLE Passwords ${process.env.VERSION}`;
app.versions = [{
  ...old,
  version: process.env.VERSION,
  date: new Date().toISOString().slice(0, 10),
  downloadURL: `https://github.com/thomasthanos/MYLE/releases/download/${process.env.TAG}/MYLE-Passwords.ipa`,
  size: Number(process.argv[2]),
  minOSVersion: old.minOSVersion || "15.0",
  localizedDescription: notes,
  versionDescription: notes,
}];
fs.writeFileSync(file, JSON.stringify(source, null, 2) + "\n");
