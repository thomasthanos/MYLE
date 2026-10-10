// Dev-only stand-in for the setup crate, so the window can be worked on in a
// browser: `npx vite`, then /installer.html?demo=install (or =update,
// =uninstall, =running, =error, =shortcut-warning).
import type { Progress, SetupApi, SetupState } from "./api";
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
export function previewApi(mode: string): SetupApi {
  const uninstall = mode === "uninstall";
  const installed = uninstall
    ? { version: "7.0.1", dir: DIR }
    : mode === "reinstall"
      ? { version: "7.1.0", dir: DIR }
      : mode === "update"
        ? { version: "7.0.1", dir: DIR }
        : null;
  const state: SetupState = {
    mode: uninstall ? "uninstall" : "install",
    product: "MYLE",
    version: uninstall ? "7.0.1" : "7.1.0",
    installed,
    dir: DIR,
    size: 44_310_528,
    shortcuts: { desktop: true, startMenu: true, startup: mode !== "update", startMinimized: true },
    passive: mode === "passive",
    ready: true,
    extensionUrl: uninstall ? null : "https://example.com/myle-passwords",
  };
  let runningAsked = false;
  return {
    async state() {
      await sleep(120);
      return state;
    },
    async running() {
      if (mode === "running" && !runningAsked) {
        runningAsked = true;
        return ["MYLE.exe"];
      }
      return [];
    },
    async checkFolder(dir) {
      if (/^[a-z]:\\?$/i.test(dir.trim())) throw "Choose a full folder path, not a drive root.";
    },
    async install(_request, onEvent) {
      await stage(onEvent, "closingApp", 350);
      await stage(onEvent, "preparing", 450);
      onEvent({ event: "stage", data: { stage: "copying" } });
      const files = FILES;
      const total = files.reduce((sum, [, size]) => sum + size, 0);
      let done = 0;
      for (const [file, size] of files) {
        for (let part = 0; part < size; part += 900_000) {
          await sleep(45);
          done = Math.min(total, done + Math.min(900_000, size - part));
          onEvent({ event: "files", data: { done, total, file } });
          if (mode === "error" && done > total * 0.6) {
            throw "ludusavi\\ludusavi.exe is in use: The process cannot access the file because it is being used by another process. (os error 32)";
          }
        }
      }
      await stage(onEvent, "registering", 400);
      await stage(onEvent, "shortcuts", 450);
      await stage(onEvent, "finishing", 300);
      const warnings =
        mode === "shortcut-warning"
          ? ["The Start Menu shortcut could not be created: Access is denied. (0x80070005)."]
          : [];
      return { dir: DIR, warnings };
    },
    async uninstall(removeData, onEvent) {
      await stage(onEvent, "closingApp", 350);
      await stage(onEvent, "preparing", 250);
      await stage(onEvent, "shortcuts", 350);
      onEvent({ event: "stage", data: { stage: "removingFiles" } });
      const names = FILES.map(([file]) => file);
      for (let i = 0; i <= names.length; i++) {
        await sleep(220);
        onEvent({ event: "files", data: { done: i, total: names.length, file: names[i] ?? "" } });
      }
      await stage(onEvent, "registering", 350);
      if (removeData) await stage(onEvent, "removingData", 600);
      await stage(onEvent, "finishing", 250);
    },
    async launch() {
      if (mode === "launch-fail") throw "Windows could not start the app.";
      await sleep(900); // the real one returns once the app's window is up
    },
    async openExtension() {},
    async exit() {},
  };
}
async function stage(onEvent: (e: Progress) => void, name: Extract<Progress, { event: "stage" }>["data"]["stage"], ms: number) {
  onEvent({ event: "stage", data: { stage: name } });
  await sleep(ms);
}
const DIR = String.raw`C:\Users\Thomas\AppData\Local\ThomasThanos\MakeYourLifeEasier`;
const FILES: [string, number][] = [
  ["MYLE.exe", 14_200_000],
  ["install.json", 2_000],
  ["ludusavi/LICENSE", 1_100],
  ["ludusavi/ludusavi.exe", 9_800_000],
  ["ludusavi/manifest.yaml", 17_100_000],
  ["spicetify/placeholder-theme/user.css", 3_400],
  ["uninstall.exe", 3_200_000],
];
