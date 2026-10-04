// Patches the generated Android project (.github/workflows/mobile.yml) to add
// FileProvider and native installer support for automatic in-app APK updates.
import { mkdir, readFile, writeFile, copyFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const PERMISSION_TAG = '<uses-permission android:name="android.permission.REQUEST_INSTALL_PACKAGES" />';

export const PROVIDER_BLOCK = `
        <provider
            android:name="androidx.core.content.FileProvider"
            android:authorities="uk.thomast.myle.passwords.fileprovider"
            android:exported="false"
            android:grantUriPermissions="true">
            <meta-data
                android:name="android.support.FILE_PROVIDER_PATHS"
                android:resource="@xml/file_paths" />
        </provider>`;

/**
 * Injects REQUEST_INSTALL_PACKAGES and FileProvider into AndroidManifest.xml string.
 */
export function patchManifest(content) {
  let patched = content;

  if (!patched.includes("REQUEST_INSTALL_PACKAGES")) {
    patched = patched.replace(/<manifest[^>]*>/, (match) => `${match}\n    ${PERMISSION_TAG}`);
  }

  if (!patched.includes("uk.thomast.myle.passwords.fileprovider")) {
    patched = patched.replace(/<\/application>/, `${PROVIDER_BLOCK}\n    </application>`);
  }

  return patched;
}

/**
 * Copies native installer plugin and file paths XML, and patches AndroidManifest.xml.
 */
export async function patchAndroid(root = new URL("../", import.meta.url).pathname) {
  const normalizedRoot = root.replace(/^\/([A-Z]:)/i, "$1");
  const androidSrc = join(normalizedRoot, "backend/mobile/android");
  const genAndroid = join(normalizedRoot, "backend/mobile/gen/android/app/src/main");

  const javaDest = join(genAndroid, "java/uk/thomast/myle/passwords");
  const resXmlDest = join(genAndroid, "res/xml");
  const manifestPath = join(genAndroid, "AndroidManifest.xml");

  await mkdir(javaDest, { recursive: true });
  await mkdir(resXmlDest, { recursive: true });

  await copyFile(join(androidSrc, "InstallerPlugin.kt"), join(javaDest, "InstallerPlugin.kt"));
  await copyFile(join(androidSrc, "file_paths.xml"), join(resXmlDest, "file_paths.xml"));

  const manifest = await readFile(manifestPath, "utf8");
  const updatedManifest = patchManifest(manifest);
  await writeFile(manifestPath, updatedManifest);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const root = process.argv[2] || process.cwd();
  await patchAndroid(root);
  console.log("Successfully patched Android project for in-app updates.");
}
