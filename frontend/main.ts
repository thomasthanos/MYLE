import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/background.css";
import "./styles/glass.css";
import "./styles/controls.css";
import "./lib/settings.svelte"; // applies saved visual preferences before first paint
import { mount } from "svelte";
import App from "./app/App.svelte";
import { hardenWebview } from "./lib/desktop";
hardenWebview();
export default mount(App, { target: document.getElementById("app")! });
