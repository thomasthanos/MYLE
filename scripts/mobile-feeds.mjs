// The feeds of a MYLE Passwords release (.github/workflows/mobile.yml):
//   mobile-latest.json  what Android's update check reads (backend/mobile/src/update.rs)
//   sidestore.json      the SideStore / AltStore source an iPhone adds once
//
//   node scripts/mobile-feeds.mjs --version 1.0.0 --tag mobile-v1.0.0 \
//     --base https://downloads.thomast.uk --repo owner/repo \
//     --apk out/MYLE-Passwords.apk --ipa out/MYLE-Passwords.ipa --out out
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

export const APP_ID = "uk.thomast.myle.passwords";
export const MIN_IOS = "15.0";

/** `apk` and `ipa`: { size, sha256 } of the files published next to the feeds. */
export function feeds({ version, tag, base, repo, apk, ipa, date = new Date() }) {
  const notes = `https://github.com/${repo}/releases/tag/${tag}`;
  const day = date.toISOString().slice(0, 10);
  const icon = `${base}/myle-passwords-icon.png`;
  const latest = {
    version,
    notes,
    pubDate: date.toISOString().replace(/\.\d+Z$/, "Z"),
    // This version's own file: no cache hands out an older APK by its name.
    android: { url: `${base}/MYLE-Passwords-${version}.apk`, size: apk.size, sha256: apk.sha256 },
    ios: { url: `${base}/MYLE-Passwords-${version}.ipa`, size: ipa.size, sha256: ipa.sha256, source: `${base}/sidestore.json` },
  };
  const sidestore = {
    name: "MYLE",
    identifier: "uk.thomast.myle.source",
    subtitle: "MYLE Passwords for iPhone",
    description: "MYLE Passwords: the Password Manager of MYLE on your iPhone, in sync with your PC.",
    iconURL: icon,
    website: `https://github.com/${repo}`,
    tintColor: "#6366F1",
    apps: [
      {
        name: "MYLE Passwords",
        bundleIdentifier: APP_ID,
        developerName: "ThomasThanos",
        subtitle: "Your MYLE password vault, in sync with your PC",
        localizedDescription:
          "See, copy and edit the logins in your MYLE password vault, with their 2FA codes. It syncs with MYLE on your PC through your account; the vault is encrypted with your master password before it leaves your devices. Opens with Face ID or Touch ID.",
        iconURL: icon,
        tintColor: "#6366F1",
        category: "utilities",
        screenshotURLs: [],
        versions: [
          {
            version,
            buildVersion: version,
            date: day,
            localizedDescription: `What's new: ${notes}`,
            downloadURL: latest.ios.url,
            size: ipa.size,
            sha256: ipa.sha256,
            minOSVersion: MIN_IOS,
          },
        ],
        appPermissions: {
          entitlements: [],
          privacy: {
            NSCameraUsageDescription: "To read a website's 2FA QR code.",
            NSFaceIDUsageDescription: "To open your password vault with Face ID.",
          },
        },
      },
    ],
    news: [],
  };
  return { latest, sidestore };
}

async function described(path) {
  const bytes = await readFile(path);
  return { size: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { values } = parseArgs({
    options: Object.fromEntries(["version", "tag", "base", "repo", "apk", "ipa", "out"].map((name) => [name, { type: "string" }])),
  });
  for (const name of ["version", "tag", "base", "repo", "apk", "ipa", "out"]) {
    if (!values[name]) throw new Error(`--${name} is missing`);
  }
  const { latest, sidestore } = feeds({
    ...values,
    base: values.base.replace(/\/+$/, ""),
    apk: await described(values.apk),
    ipa: await described(values.ipa),
  });
  await writeFile(join(values.out, "mobile-latest.json"), `${JSON.stringify(latest, null, 2)}\n`);
  await writeFile(join(values.out, "sidestore.json"), `${JSON.stringify(sidestore, null, 2)}\n`);
  console.log(JSON.stringify(latest, null, 2));
}
