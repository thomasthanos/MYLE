// Dev-only stand-in for the Rust updater, so the splash can be worked on in a browser: `npx vite`, then open /splash.html?demo=update (or =offline, =latest). Inside the app, `MYLE_UPDATER_DEMO` does the same for real.
import type { DownloadEvent, UpdateAsset, UpdateCheck } from "../lib/updater";
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
export function previewUpdater(mode: string) {
  const current = "7.0.1";
  return {
    version: current,
    async checkForUpdate(): Promise<UpdateCheck> {
      await sleep(900);
      if (mode === "offline") throw new Error("error sending request for url (https://downloads.thomast.uk/latest.json)");
      if (mode === "latest") return { status: "upToDate", current, latest: current };
      return {
        status: "available",
        current,
        latest: "7.1.0",
        notes: "",
        asset: { name: "MYLE.exe", version: "7.1.0", url: "", size: 13_606_875, digest: null },
      };
    },
    async installUpdate(asset: UpdateAsset, onEvent: (e: DownloadEvent) => void): Promise<void> {
      const total = asset.size;
      onEvent({ event: "started", data: { total } });
      let downloaded = 0;
      while (downloaded < total) {
        await sleep(50);
        // An uneven connection, so the speed readout has something to do.
        downloaded = Math.min(total, downloaded + 60_000 + Math.random() * 180_000);
        onEvent({ event: "progress", data: { downloaded, total } });
      }
      onEvent({ event: "verifying" });
      await sleep(700);
      onEvent({ event: "installing" });
      await sleep(1400);
    },
    async finishStartup(): Promise<void> {},
  };
}
