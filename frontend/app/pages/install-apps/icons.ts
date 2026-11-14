// Where an app's icon comes from, best first. Shared by the card and by the app's preload, so the preload warms exactly the addresses the card asks for.
import type { AppEntry } from "./state.svelte";

export const CUSTOM_ICONS: Record<string, string> = {
  "Custom.NvidiaApp": "/icons/installapps/NVIDIA-App.svg",
  "Custom.Optimizer": "/icons/installapps/Optimizer.svg",
  "Custom.Vencord": "/icons/installapps/Vencord.svg",
  "Custom.BetterDiscord": "/icons/installapps/BetterDiscord.svg",
  "GitHub.cli": "/icons/installapps/GitHub-CLI.svg",
  "GitHub.Copilot": "/icons/installapps/GitHub-Copilot.svg",
  "GitHub.CopilotApp": "/icons/installapps/GitHub-Copilot.svg",
  "Roblox.Roblox": "/icons/installapps/Roblox.svg",
  "IObit.DriverBooster": "/icons/installapps/Driver-Booster.svg",
  "CreativeTechnology.OpenAL": "/icons/installapps/OpenAL.svg",
  "OpenSCAD.OpenSCAD": "/icons/installapps/OpenSCAD.svg",
  "xAI.GrokBot": "/icons/installapps/Grok.svg",
  "Microsoft.Edge": "/icons/installapps/Microsoft-Edge.svg",
  "BlenderFoundation.Blender": "/icons/installapps/Blender.svg",
  "M2Team.NanaZip": "/icons/installapps/NanaZip.svg",
  "WiresharkFoundation.Wireshark": "/icons/installapps/Wireshark.svg",
  "Rustlang.Rustup": "/icons/installapps/Rust.svg",
  "GoLang.Go": "/icons/installapps/Go.svg",
  "Discord.Discord.Canary": "/icons/installapps/Discord-Canary.svg",
  "OpenAI.ChatGPT": "/icons/installapps/ChatGPT.svg",
  "Anthropic.Claude": "/icons/installapps/Claude.svg",
};

export function resolveCustomIcon(id: string): string | undefined {
  if (!id) return undefined;
  if (CUSTOM_ICONS[id]) return CUSTOM_ICONS[id];
  const lower = id.toLowerCase();
  for (const [key, icon] of Object.entries(CUSTOM_ICONS)) {
    if (key.toLowerCase() === lower) return icon;
  }
  // Package families & prefixes
  if (lower.startsWith("microsoft.visualstudio.")) return "/icons/installapps/Visual-Studio.svg";
  if (lower.startsWith("microsoft.dotnet.")) return "/icons/installapps/Microsoft-DotNet.svg";
  if (lower.startsWith("microsoft.vcredist.")) return "/icons/installapps/Visual-Cpp-Redist.svg";
  if (lower.startsWith("openscad.openscad")) return "/icons/installapps/OpenSCAD.svg";
  if (lower.startsWith("github.copilot")) return "/icons/installapps/GitHub-Copilot.svg";
  if (lower.startsWith("discord.discord.canary")) return "/icons/installapps/Discord-Canary.svg";
  return undefined;
}

export function iconSources(app: Pick<AppEntry, "id" | "icon" | "iconDomain" | "site">): string[] {
  const raw = app.icon ?? resolveCustomIcon(app.id);
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
