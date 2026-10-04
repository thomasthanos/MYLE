import { defineConfig, type UserConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const host = process.env.TAURI_DEV_HOST;
const project = import.meta.dirname;
const frontend = resolve(project, "frontend");
// Next to the Rust builds, so every build output lives in one ignored folder.
const target = resolve(project, "backend/target");
// The browser extension's download page (`myle.extensionUrl` in package.json),
// the same one the setup offers. Empty until the extension is published.
const extensionUrl: string = JSON.parse(readFileSync(resolve(project, "package.json"), "utf8")).myle?.extensionUrl ?? "";

// https://v2.tauri.app/start/frontend/vite/
//
// The pages live in frontend/ (index.html, splash.html, installer.html,
// mobile.html), static files in frontend/public/. Three builds come out of it:
//   vite build                the app's two windows -> backend/target/web/
//                             (embedded by the app, backend/tauri.conf.json)
//   vite build --mode setup   the setup and uninstaller window -> backend/target/web-setup/
//                             (embedded by backend/installer only)
//   vite build --mode mobile  MYLE Passwords for phones -> backend/target/web-mobile/
//                             (embedded by backend/mobile; `MYLE_MOBILE` is true)
// The dev server serves all the pages (`--mode mobile` for the phone's words).
export default defineConfig(({ mode }): UserConfig => {
  const setup = mode === "setup";
  const mobile = mode === "mobile";
  return {
    root: frontend,
    envDir: project,
    plugins: [svelte()],
    define: {
      "import.meta.env.VITE_EXTENSION_URL": JSON.stringify(extensionUrl),
      "import.meta.env.MYLE_MOBILE": JSON.stringify(mobile),
    },
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
      watch: { ignored: ["**/backend/**"] },
    },
    envPrefix: ["VITE_", "TAURI_ENV_"],
    build: {
      // WebView2 is evergreen Chromium; phones run Android's WebView (also
      // evergreen) and iOS 15's Safari at least.
      target: mobile ? ["chrome120", "safari15"] : "chrome120",
      sourcemap: !!process.env.TAURI_ENV_DEBUG,
      outDir: resolve(target, setup ? "web-setup" : mobile ? "web-mobile" : "web"),
      emptyOutDir: true,
      // The setup window and the phone need none of the app's icons.
      copyPublicDir: !setup && !mobile,
      rolldownOptions: {
        input: setup
          ? { installer: resolve(frontend, "installer.html") }
          : mobile
            ? { mobile: resolve(frontend, "mobile.html") }
            : {
                // One entry page per window: the updater splash, the main shell.
                main: resolve(frontend, "index.html"),
                splash: resolve(frontend, "splash.html"),
                // A scheduled Game Saves backup's notice, a window of its own.
                notice: resolve(frontend, "notice.html"),
              },
      },
    },
  };
});
