// Where an app's icon comes from, best first. Shared by the card and by the app's preload, so the preload warms exactly the addresses the card asks for.
import type { AppEntry } from "./state.svelte";
export const CUSTOM_ICONS: Record<string, string> = {
  "Custom.NvidiaApp": "/icons/installapps/NVIDIA-App.svg",
  "Custom.Optimizer": "/icons/installapps/Optimizer.svg",
  "Custom.Vencord": "/icons/installapps/Vencord.svg",
  "Custom.BetterDiscord": "/icons/installapps/BetterDiscord.svg",
};
export function iconSources(app: Pick<AppEntry, "id" | "icon" | "iconDomain" | "site">): string[] {
  const raw = app.icon ?? CUSTOM_ICONS[app.id];
  if (raw) return [raw.startsWith("/icons/installapps/") ? `${raw}?v=9` : raw];
  const domain = (app.iconDomain ?? app.site?.split("/")[0])?.trim().toLowerCase();
  if (!domain || !/^[a-z0-9.-]+$/.test(domain)) return [];
  const encodedDomain = encodeURIComponent(domain);
  const encodedUrl = encodeURIComponent(`https://${domain}`);
  return [
    `https://a.favicon.im/${encodedDomain}?larger=true&throw-error-on-404=true`,
    `https://www.google.com/s2/favicons?domain_url=${encodedUrl}&sz=128`,
  ];
}
