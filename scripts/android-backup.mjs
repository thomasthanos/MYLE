// Keeps MYLE Passwords' data out of Android backups AND device-to-device
// transfers (.github/workflows/mobile.yml, after `tauri android init`).
// `allowBackup="false"` stops cloud backup, but from Android 12 not a
// "copy apps & data to a new phone" transfer: only data extraction rules do.
// The app folder holds the vault, the account's session tokens and the
// biometric unlock file. The rules go on the app's own manifest, which wins
// over the plugins' (the biometrics plugin ships rules that miss its store).
//   node scripts/android-backup.mjs backend/mobile/gen/android/app/src/main/AndroidManifest.xml
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const TOOLS = "http://schemas.android.com/tools";
const DOMAINS = ["root", "file", "database", "sharedpref", "external"];
const excludes = (indent) => DOMAINS.map((d) => `${indent}<exclude domain="${d}" path="." />`).join("\n");

/** res/xml/myle_backup.xml: nothing goes to a cloud backup or a new phone. */
export const DATA_EXTRACTION_RULES = `<?xml version="1.0" encoding="utf-8"?>
<data-extraction-rules>
  <cloud-backup>
${excludes("    ")}
  </cloud-backup>
  <device-transfer>
${excludes("    ")}
  </device-transfer>
</data-extraction-rules>
`;
/** res/xml/myle_full_backup.xml: the same for Android 11 and older. */
export const FULL_BACKUP_CONTENT = `<?xml version="1.0" encoding="utf-8"?>
<full-backup-content>
${excludes("  ")}
</full-backup-content>
`;

const OURS = {
  "android:allowBackup": "false",
  "android:dataExtractionRules": "@xml/myle_backup",
  "android:fullBackupContent": "@xml/myle_full_backup",
};

/** The manifest with the backup rules on `<application>` and the tools namespace declared. */
export function patchManifest(text) {
  let out = text.replace(/<manifest\b[^>]*>/, (tag) =>
    /\sxmlns:tools=/.test(tag) ? tag : tag.replace(/<manifest\b/, `<manifest xmlns:tools="${TOOLS}"`),
  );
  let found = false;
  out = out.replace(/<application\b([^>]*?)(\/?)>/, (_tag, attributes, selfClosing) => {
    found = true;
    let rest = attributes;
    for (const name of Object.keys(OURS)) rest = rest.replace(new RegExp(`\\s+${name}="[^"]*"`), "");
    // tools:replace keeps whatever else it named, plus ours.
    const replaced = new Set(["android:dataExtractionRules", "android:fullBackupContent", "android:allowBackup"]);
    rest = rest.replace(/\s+tools:replace="([^"]*)"/, (_m, list) => {
      for (const item of list.split(",").map((s) => s.trim()).filter(Boolean)) replaced.add(item);
      return "";
    });
    const added = Object.entries(OURS).map(([k, v]) => `\n        ${k}="${v}"`).join("");
    return `<application${rest}${added}\n        tools:replace="${[...replaced].join(",")}"${selfClosing}>`;
  });
  if (!found) throw new Error("The manifest has no <application> element.");
  return out;
}

async function main(manifest) {
  if (!manifest) throw new Error("usage: node scripts/android-backup.mjs <AndroidManifest.xml>");
  await writeFile(manifest, patchManifest(await readFile(manifest, "utf8")));
  const xml = join(dirname(manifest), "res", "xml");
  await mkdir(xml, { recursive: true });
  await writeFile(join(xml, "myle_backup.xml"), DATA_EXTRACTION_RULES);
  await writeFile(join(xml, "myle_full_backup.xml"), FULL_BACKUP_CONTENT);
  console.log(`Backup and transfer rules set in ${manifest}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main(process.argv[2]).catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
