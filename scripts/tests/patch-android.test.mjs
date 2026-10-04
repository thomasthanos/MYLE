import assert from "node:assert/strict";
import test from "node:test";
import { PERMISSION_TAG, PROVIDER_BLOCK, patchManifest } from "../patch-android.mjs";

test("patchManifest adds REQUEST_INSTALL_PACKAGES and FileProvider", () => {
  const dummyManifest = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="uk.thomast.myle.passwords">
    <application
        android:label="MYLE Passwords"
        android:icon="@mipmap/ic_launcher">
        <activity android:name=".MainActivity">
        </activity>
    </application>
</manifest>`;

  const patched = patchManifest(dummyManifest);
  assert.ok(patched.includes(PERMISSION_TAG));
  assert.ok(patched.includes("uk.thomast.myle.passwords.fileprovider"));
  assert.ok(patched.includes("@xml/file_paths"));

  // Calling it again should be idempotent
  const secondPatch = patchManifest(patched);
  assert.equal(secondPatch, patched);
});
