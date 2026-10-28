// Automatic categories from keywords in package IDs.
export const CATEGORIES = [
  "Browsers",
  "Communication",
  "Games",
  "Media",
  "Development",
  "Security",
  "Hardware",
  "Utilities",
  "Others",
] as const;
export type Category = (typeof CATEGORIES)[number];
// A keyword matches an ID token that starts with it ("git" matches "github"). Checked in this order, so put the more specific categories first.
const KEYWORDS: [Category, string[]][] = [
  ["Browsers", ["chrome", "firefox", "brave", "opera", "vivaldi", "zen", "librewolf", "torbrowser", "waterfox", "chromium", "edge", "browser"]],
  ["Communication", ["discord", "telegram", "zoom", "teams", "slack", "signal", "viber", "teamspeak", "thunderbird", "whatsapp", "skype", "element", "mumble"]],
  ["Games", ["steam", "epicgames", "eadesktop", "ubisoft", "gog", "battlenet", "playnite", "minecraft", "riot", "rockstar", "heroic", "game"]],
  ["Development", ["visualstudio", "vscode", "git", "nodejs", "python", "docker", "postman", "notepad++", "jetbrains", "windowsterminal", "powershell", "rust", "golang", "jdk", "java", "sublime", "cursor", "dbeaver", "wsl"]],
  ["Media", ["vlc", "spotify", "obsproject", "audacity", "handbrake", "gimp", "krita", "inkscape", "mpc", "foobar", "stremio", "paintdotnet", "kodi", "plex", "potplayer", "musicbee", "itunes"]],
  ["Security", ["bitwarden", "keepass", "malwarebytes", "protonvpn", "wireguard", "veracrypt", "nordpass", "1password", "vpn", "antivirus"]],
  ["Hardware", ["cpu", "gpu", "hwmonitor", "hwinfo", "afterburner", "crystaldisk", "fancontrol", "openrgb", "displaydriveruninstaller", "nvidia", "amd", "intel", "logitech", "corsair", "razer"]],
  ["Utilities", ["7zip", "winrar", "everything", "powertoys", "sharex", "rufus", "qbittorrent", "dropbox", "googledrive", "anydesk", "teamviewer", "sumatrapdf", "acrobat", "freedownloadmanager", "revouninstaller", "bleachbit", "ditto", "onedrive", "flameshot"]],
];
export function categorize(id: string): Category {
  const tokens = id.toLowerCase().split(/[.\-_ ]+/).filter(Boolean);
  for (const [category, keywords] of KEYWORDS) {
    if (tokens.some((token) => keywords.some((k) => token.startsWith(k)))) return category;
  }
  return "Others";
}
export function isCategory(value: unknown): value is Category {
  return typeof value === "string" && (CATEGORIES as readonly string[]).includes(value);
}
