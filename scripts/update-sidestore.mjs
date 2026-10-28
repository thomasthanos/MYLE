// Points docs/sidestore.json at a release's IPA, with its notes as SideStore's "what is new" (the same text as the downloads.thomast.uk source). Run by .github/workflows/mobile.yml: VERSION=10.0.2 TAG=v10.0.2 node scripts/update-sidestore.mjs <ipa size in bytes>
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { notesFileFor, plainNotes } from "./mobile-feeds.mjs";
const { VERSION: version, TAG: tag } = process.env;
const file = "docs/sidestore.json";
const source = JSON.parse(readFileSync(file, "utf8"));
const app = source.apps[0];
const old = app.versions[0] || {};
const notesFile = notesFileFor(version, tag).find((f) => existsSync(f));
const notes = (notesFile && plainNotes(readFileSync(notesFile, "utf8"))) || `MYLE Passwords ${version}`;
app.versions = [
  {
    ...old,
    version,
    date: new Date().toISOString().slice(0, 10),
    downloadURL: `https://github.com/thomasthanos/MYLE/releases/download/${tag}/MYLE-Passwords.ipa`,
    size: Number(process.argv[2]),
    minOSVersion: old.minOSVersion || "15.0",
    localizedDescription: notes,
    versionDescription: notes,
  },
];
writeFileSync(file, JSON.stringify(source, null, 2) + "\n");
