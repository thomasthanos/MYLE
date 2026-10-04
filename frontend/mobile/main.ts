// MYLE Passwords for phones: the Password Manager page in a shell of its own
// (`vite build --mode mobile`, embedded by backend/mobile).
import "@fontsource-variable/outfit";
import "../styles/tokens.css";
import "../styles/base.css";
import "../styles/glass.css";
import "../styles/controls.css";
import "./mobile.css";

import { mount } from "svelte";
import MobileApp from "./MobileApp.svelte";

export default mount(MobileApp, { target: document.getElementById("app")! });
