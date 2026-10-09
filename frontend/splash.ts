import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/background.css";

import { mount } from "svelte";
import Splash from "./splash/Splash.svelte";
import ContextMenuHost from "./lib/components/ContextMenuHost.svelte";
import { hardenWebview } from "./lib/desktop";

// Solid panels always, like the app by default (see lib/settings.svelte.ts).
document.documentElement.classList.add("solid");
// The dark theme too, when it is on (same storage as the main window).
try {
  if (localStorage.getItem("myle.dark") === "1") document.documentElement.classList.add("dark");
} catch {
  // Storage blocked: the default look.
}
hardenWebview();
mount(ContextMenuHost, { target: document.body });

export default mount(Splash, { target: document.getElementById("splash")! });
