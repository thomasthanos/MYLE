import "../styles/tokens.css";
import "../styles/base.css";
import "../styles/background.css";
import "../styles/controls.css";
import { mount } from "svelte";
import Setup from "./Setup.svelte";
import ContextMenuHost from "../lib/components/ContextMenuHost.svelte";
import TooltipHost from "../lib/components/TooltipHost.svelte";
import { hardenWebview } from "../lib/desktop";
// Solid panels, like the app by default (see lib/settings.svelte.ts).
document.documentElement.classList.add("solid");
hardenWebview();
mount(ContextMenuHost, { target: document.body });
// Its tooltips are drawn by the app, as in every other window.
mount(TooltipHost, { target: document.body });
export default mount(Setup, { target: document.getElementById("setup")! });
