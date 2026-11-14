// The Android backup and transfer rules: `node --test scripts/tests/`.
import assert from "node:assert/strict";
import test from "node:test";
import { DATA_EXTRACTION_RULES, FULL_BACKUP_CONTENT, patchManifest } from "../android-backup.mjs";

const template = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    <application
        android:icon="@mipmap/ic_launcher"
        android:allowBackup="true"
        android:label="@string/app_name">
        <activity android:name=".MainActivity" />
    </application>
</manifest>
`;

test("the app's manifest gets the rules and overrides the plugins' ones", () => {
  const out = patchManifest(template);
  assert.match(out, /<manifest xmlns:tools="http:\/\/schemas\.android\.com\/tools" xmlns:android=/);
  assert.match(out, /android:allowBackup="false"/);
  assert.doesNotMatch(out, /android:allowBackup="true"/);
  assert.match(out, /android:dataExtractionRules="@xml\/myle_backup"/);
  assert.match(out, /android:fullBackupContent="@xml\/myle_full_backup"/);
  assert.match(out, /tools:replace="android:dataExtractionRules,android:fullBackupContent,android:allowBackup"/);
  // Nothing else on the element is lost.
  assert.match(out, /android:icon="@mipmap\/ic_launcher"/);
  assert.match(out, /<activity android:name="\.MainActivity" \/>/);
});

test("running it twice changes nothing more, and an existing tools:replace is kept", () => {
  const once = patchManifest(template);
  assert.equal(patchManifest(once), once);
  const withReplace = template
    .replace("<manifest ", '<manifest xmlns:tools="http://schemas.android.com/tools" ')
    .replace('android:label="@string/app_name"', 'android:label="@string/app_name" tools:replace="android:label"');
  const out = patchManifest(withReplace);
  assert.equal(out.match(/xmlns:tools=/g).length, 1);
  assert.match(out, /tools:replace="android:dataExtractionRules,android:fullBackupContent,android:allowBackup,android:label"/);
});

test("a self-closing <application /> works, and a manifest without one is refused", () => {
  const out = patchManifest('<manifest xmlns:android="x"><application android:allowBackup="true" /></manifest>');
  assert.match(out, /android:dataExtractionRules="@xml\/myle_backup"[\s\S]*\/>/);
  assert.throws(() => patchManifest("<manifest></manifest>"), /no <application>/);
});

test("every app data domain is kept out of cloud backup and device transfer", () => {
  for (const domain of ["root", "file", "database", "sharedpref", "external"]) {
    assert.equal(DATA_EXTRACTION_RULES.split(`<exclude domain="${domain}" path="." />`).length - 1, 2, domain);
    assert.ok(FULL_BACKUP_CONTENT.includes(`<exclude domain="${domain}" path="." />`), domain);
  }
  assert.match(DATA_EXTRACTION_RULES, /<cloud-backup>[\s\S]*<\/cloud-backup>\s*<device-transfer>[\s\S]*<\/device-transfer>/);
});
