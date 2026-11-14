import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { CUSTOM_ICONS, iconSources, resolveCustomIcon } from "../../frontend/app/pages/install-apps/icons.ts";

test("resolveCustomIcon maps all winget upgrade packages and families", () => {
  const cases = [
    ["GitHub.cli", "/icons/installapps/GitHub-CLI.svg"],
    ["github.cli", "/icons/installapps/GitHub-CLI.svg"],
    ["GitHub.CopilotApp", "/icons/installapps/GitHub-Copilot.svg"],
    ["GitHub.Copilot", "/icons/installapps/GitHub-Copilot.svg"],
    ["Microsoft.VisualStudio.2022.BuildTools", "/icons/installapps/Visual-Studio.svg"],
    ["Microsoft.VisualStudio.2022.Professional", "/icons/installapps/Visual-Studio.svg"],
    ["Microsoft.VisualStudio.2022.Community", "/icons/installapps/Visual-Studio.svg"],
    ["Microsoft.DotNet.SDK.8", "/icons/installapps/Microsoft-DotNet.svg"],
    ["Microsoft.DotNet.SDK.10", "/icons/installapps/Microsoft-DotNet.svg"],
    ["Microsoft.DotNet.DesktopRuntime.8", "/icons/installapps/Microsoft-DotNet.svg"],
    ["Microsoft.VCRedist.2015+.x64", "/icons/installapps/Visual-Cpp-Redist.svg"],
    ["Microsoft.VCRedist.2015+.x86", "/icons/installapps/Visual-Cpp-Redist.svg"],
    ["IObit.DriverBooster", "/icons/installapps/Driver-Booster.svg"],
    ["CreativeTechnology.OpenAL", "/icons/installapps/OpenAL.svg"],
    ["OpenSCAD.OpenSCAD", "/icons/installapps/OpenSCAD.svg"],
    ["OpenSCAD.OpenSCAD.Nightly", "/icons/installapps/OpenSCAD.svg"],
    ["xAI.GrokBot", "/icons/installapps/Grok.svg"],
    ["Roblox.Roblox", "/icons/installapps/Roblox.svg"],
    ["Microsoft.Edge", "/icons/installapps/Microsoft-Edge.svg"],
    ["BlenderFoundation.Blender", "/icons/installapps/Blender.svg"],
    ["M2Team.NanaZip", "/icons/installapps/NanaZip.svg"],
    ["WiresharkFoundation.Wireshark", "/icons/installapps/Wireshark.svg"],
    ["Rustlang.Rustup", "/icons/installapps/Rust.svg"],
    ["GoLang.Go", "/icons/installapps/Go.svg"],
    ["Discord.Discord.Canary", "/icons/installapps/Discord-Canary.svg"],
    ["OpenAI.ChatGPT", "/icons/installapps/ChatGPT.svg"],
    ["Anthropic.Claude", "/icons/installapps/Claude.svg"],
  ];

  for (const [id, expectedIcon] of cases) {
    const resolved = resolveCustomIcon(id);
    assert.equal(resolved, expectedIcon, `Failed for id: ${id}`);
    const sources = iconSources({ id });
    assert.equal(sources[0], `${expectedIcon}?v=9`, `Source failed for id: ${id}`);
  }
});

test("all referenced custom SVG icons exist on disk and are non-empty", () => {
  const root = path.resolve("frontend/public");
  for (const iconPath of Object.values(CUSTOM_ICONS)) {
    const fullPath = path.join(root, iconPath);
    assert.ok(fs.existsSync(fullPath), `Icon missing on disk: ${fullPath}`);
    const stat = fs.statSync(fullPath);
    assert.ok(stat.size > 500, `Icon file unexpectedly small: ${fullPath} (${stat.size} bytes)`);
  }
});
