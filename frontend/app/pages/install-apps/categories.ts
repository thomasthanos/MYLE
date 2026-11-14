// Automatic categories for curated apps and packages discovered through WinGet.
export const CATEGORIES = [
  "Browsers",
  "Communication",
  "Productivity",
  "Games",
  "Media",
  "Design & 3D",
  "Development",
  "AI",
  "Security",
  "Hardware",
  "Networking & Remote Access",
  "Utilities",
  "System Components",
  "Others",
] as const;
export type Category = (typeof CATEGORIES)[number];

// Match the package family before the broader keywords, including versioned IDs.
const PACKAGE_FAMILIES: [Category, readonly string[]][] = [
  ["Development", ["microsoft.dotnet.sdk", "microsoft.dotnet.uninstalltool", "google.antigravity", "oven-sh.bun", "gnuwin32.make"]],
  ["Communication", ["rocket.chat"]],
  ["Games", ["dolphinemulator.dolphin"]],
  ["Media", ["yt-dlp.yt-dlp"]],
  ["AI", ["jan.jan"]],
  ["Security", ["microsoft.sysinternals.sigcheck"]],
  ["System Components", [
    "microsoft.dotnet", "microsoft.vcredist", "microsoft.directx", "microsoft.edgewebview2runtime",
    "microsoft.windowsappruntime", "microsoft.vclibs", "microsoft.ui.xaml", "microsoft.xnaredist",
    "creativetechnology.openal", "oracle.javaruntimeenvironment",
  ]],
];
// A keyword matches an ID token that starts with it ("git" matches "github"). Checked in this order, so put the more specific categories first.
const KEYWORDS: [Category, string[]][] = [
  ["Browsers", [
    "chrome", "firefox", "brave", "opera", "vivaldi", "zen", "librewolf", "torbrowser",
    "waterfox", "chromium", "edge", "browser", "duckduckgo", "floorp", "palemoon", "basilisk",
    "falkon", "seamonkey", "maxthon", "slimjet", "ungoogledchromium", "mullvadbrowser",
  ]],
  ["Communication", [
    "discord", "telegram", "zoom", "teams", "slack", "signal", "viber", "teamspeak",
    "thunderbird", "whatsapp", "skype", "element", "mumble", "mattermost", "webex", "protonmail",
    "ferdium", "outlook", "ringcentral", "zulip", "beeper", "rocketchat",
    "rambox", "mailbird", "mailspring", "betterbird", "evolution", "deltachat", "guilded",
  ]],
  ["Productivity", [
    "onlyoffice", "wpsoffice", "libreoffice", "openoffice", "obsidian", "notion", "onenote", "logseq",
    "joplin", "zotero", "mendeley", "calibre", "pdfgear", "foxit", "pdfxchange", "sumatrapdf",
    "acrobat", "drawio", "xmind", "mindmaster", "todoist", "ticktick", "anytype", "simplenote",
    "evernote", "microsoft365", "office",
  ]],
  ["Networking & Remote Access", [
    "tailscale", "zerotier", "winscp", "putty", "filezilla", "syncthing", "rustdesk", "realvnc",
    "tightvnc", "ultravnc", "tigervnc", "anydesk", "teamviewer", "parsec", "moonlight", "sunshine",
    "rclone", "cyberduck", "mobaxterm", "royalts", "remotedesktop", "tailscalerelay", "duckdns", "wireshark",
  ]],
  ["Games", [
    "steam", "epicgames", "eadesktop", "ubisoft", "gog", "battlenet", "playnite", "minecraft",
    "riot", "rockstar", "heroic", "game", "roblox", "itch", "prismlauncher", "retroarch",
    "lutris", "vortex", "modorganizer2", "dolphinemulator", "pcsx2", "rpcs3",
    "duckstation", "desmume", "mgba", "melonds", "ryujinx", "xemu", "cemu", "legendary",
    "curseforge", "modrinth", "overwolf", "osulazer", "osustable",
  ]],
  ["Design & 3D", [
    "openscad", "freecad", "librecad", "kicad", "blender", "fusion360", "sketchup", "prusaslicer",
    "orcaslicer", "ultimaker", "bambustudio", "gimp", "krita", "inkscape", "paintdotnet", "figma",
    "affinity", "creativecloud", "photoshop", "illustrator", "indesign", "substance", "autocad", "sketchbook",
    "zbrush", "darktable", "rawtherapee", "digikam", "paintnet", "aseprite", "libresprite", "penpot",
    "scribus", "qucs", "meshroom", "plasticity", "marmoset", "toolbag", "nomadsculpt",
  ]],
  ["AI", [
    "grokbot", "chatgpt", "claude", "ollama", "lmstudio", "cherrystudio", "janai", "anythingllm",
    "perplexity", "msty", "gpt4all", "koboldcpp", "openwebui", "diffusionbee", "comfyui", "stablediffusion",
  ]],
  ["Development", [
    "visualstudio", "vscode", "git", "nodejs", "python", "docker", "postman", "notepad++",
    "jetbrains", "windowsterminal", "powershell", "rust", "golang", "jdk", "java", "sublime",
    "cursor", "dbeaver", "wsl", "arduino", "zed", "vscodium", "cmake", "ninja",
    "kubernetes", "podman", "neovim", "winsdk", "azurecli", "awscli", "cloudsdk", "terraform",
    "opentofu", "virtualbox", "vmwareworkstation", "workstationpro", "temurin", "corretto", "zulu", "openjdk",
    "msys2", "mingw", "llvm", "sqlservermanagementstudio", "azuredatastudio", "datagrip", "pgadmin",
    "postgresql", "mariadb", "mysql", "redis", "insomnia", "bruno", "fiddler", "unityhub",
    "unrealengine", "godot", "helix", "emacs", "devhome", "windowskit", "nuget", "deno",
    "pnpm", "positron", "rstudio",
  ]],
  ["Media", [
    "vlc", "spotify", "obsproject", "audacity", "handbrake", "mpc", "foobar", "stremio",
    "kodi", "plex", "potplayer", "musicbee", "itunes", "ffmpeg", "mpv", "kdenlive",
    "shotcut", "davinciresolve", "jellyfin", "tidal", "aimp", "mediamonkey", "picard", "applemusic",
    "ytdlp", "makemkv", "mkvtoolnix", "mediainfo", "imageglass", "irfanview", "xnview",
    "faststone", "screenpresso", "soundcloud", "deezer", "freetube", "kid3",
  ]],
  ["Security", [
    "bitwarden", "keepass", "malwarebytes", "protonvpn", "wireguard", "veracrypt", "nordpass", "1password",
    "vpn", "antivirus", "bitdefender", "eset", "avast", "avg", "sophos", "kaspersky",
    "norton", "mcafee", "nordvpn", "mullvadvpn", "expressvpn", "surfshark", "windscribe", "yubikeymanager",
    "cryptomator", "gpg4win", "gnupg", "passbolt", "enteauth", "authy", "virustotal",
  ]],
  ["Hardware", [
    "cpu", "gpu", "hwmonitor", "hwinfo", "afterburner", "crystaldisk", "fancontrol", "openrgb",
    "displaydriveruninstaller", "nvidia", "amd", "intel", "logitech", "corsair", "razer", "driverbooster",
    "snappydriverinstaller", "occt", "furmark", "3dmark", "nzxt", "steelseries", "icue", "armourycrate",
    "ryzenmaster", "geforceexperience", "gpuz", "cpuz", "aida64", "speccy", "throttlestop", "hwmonitorpro",
    "intelxtu",
  ]],
  ["Utilities", [
    "7zip", "winrar", "everything", "powertoys", "sharex", "rufus", "qbittorrent", "dropbox",
    "googledrive", "freedownloadmanager", "revouninstaller", "bleachbit", "ditto", "onedrive", "flameshot", "wiztree",
    "treesize", "processexplorer", "processmonitor", "autoruns", "sysinternals", "nanazip", "bandizip", "peazip",
    "partitionassistant", "minitool", "teracopy", "totalcommander", "doublecommander", "freecommander", "filescommunity", "everythingtoolbar",
    "flowlauncher", "wox", "autohotkey", "autoit", "winsnapshot", "winmerge", "freefilesync",
  ]],
];
export function categorize(id: string, name = ""): Category {
  const packageId = id.toLowerCase();
  for (const [category, families] of PACKAGE_FAMILIES) {
    if (families.some((family) => packageId === family || packageId.startsWith(`${family}.`))) return category;
  }
  const tokens = packageId.split(/[.\-_ ]+/).filter(Boolean);
  for (const [category, keywords] of KEYWORDS) {
    if (tokens.some((token) => keywords.some((k) => token.startsWith(k)))) return category;
  }
  // Use the display name for IDs that don't identify the product. Match whole
  // words from the beginning so a keyword inside an unrelated title won't win.
  const product = name.toLowerCase().trim().split(/[\s._-]+/);
  const first = product[0];
  const firstTwo = product.slice(0, 2).join("");
  for (const [category, keywords] of KEYWORDS) {
    if (keywords.includes(first) || keywords.includes(firstTwo)) return category;
  }
  return "Others";
}
export function isCategory(value: unknown): value is Category {
  return typeof value === "string" && (CATEGORIES as readonly string[]).includes(value);
}
