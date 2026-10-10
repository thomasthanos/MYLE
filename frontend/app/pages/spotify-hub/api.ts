import { Channel, invoke } from "@tauri-apps/api/core";

export type SpotifyHubAction = "installSpicetify" | "restoreSpotify" | "purgeAll";
export type SpotifyHubStage =
  | "preparing"
  | "downloading"
  | "verifying"
  | "installing"
  | "restoring"
  | "uninstalling"
  | "cleaning"
  | "finalizing";
export type SpotifyHubResult = "done" | "cancelled" | "partial" | "needsAdmin";

export interface DetectedInstall {
  installed: boolean;
  version: string | null;
}

export interface SpicetifyState extends DetectedInstall {
  healthy: boolean;
  applied: boolean;
}

export interface MarketplaceState {
  installed: boolean;
  version: string | null;
}

export interface SpotifyHubReleases {
  cli: string;
  marketplace: string;
}

export interface SpotifyHubPrerequisites {
  desktopSpotify: boolean;
  supported: boolean;
  message: string | null;
}

export interface SpotifyHubJob {
  jobId: string;
  action: SpotifyHubAction;
  stage: SpotifyHubStage;
  progress: number | null;
}

export interface SpotifyHubOutcome {
  result: SpotifyHubResult;
  jobId: string;
  action: SpotifyHubAction;
  note?: string | null;
}

export interface SpotifyHubSnapshot {
  desktop: DetectedInstall;
  store: DetectedInstall;
  spicetify: SpicetifyState;
  marketplace: MarketplaceState;
  prerequisites: SpotifyHubPrerequisites;
  lastOutcome: SpotifyHubOutcome | null;
  activeJob: SpotifyHubJob | null;
}

export interface PurgePreview {
  token: string;
  detectedVariants: string[];
  categories: string[];
  warnings: string[];
  /** Unix time in milliseconds. */
  expiresAt: number;
}

export type SpotifyHubEvent =
  | { event: "stage"; data: { jobId: string; stage: SpotifyHubStage } }
  | { event: "progress"; data: { jobId: string; fraction: number } }
  | { event: "line"; data: { jobId: string; text: string; replace: boolean } };

function channel(onEvent: (event: SpotifyHubEvent) => void): Channel<SpotifyHubEvent> {
  const value = new Channel<SpotifyHubEvent>();
  value.onmessage = onEvent;
  return value;
}

export const spotifyHubApi = {
  getState: () => invoke<SpotifyHubSnapshot>("spotify_hub_get_state"),
  checkUpdates: () => invoke<SpotifyHubReleases>("spotify_hub_check_updates"),
  previewPurge: () => invoke<PurgePreview>("spotify_hub_preview_purge"),
  run: (
    action: SpotifyHubAction,
    purgeToken: string | null,
    onEvent: (event: SpotifyHubEvent) => void,
  ) =>
    invoke<SpotifyHubOutcome>("spotify_hub_run", {
      action,
      purgeToken,
      onEvent: channel(onEvent),
    }),
  cancel: (jobId: string) => invoke<void>("spotify_hub_cancel", { jobId }),
};
