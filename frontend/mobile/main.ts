// MYLE Passwords for phones: the Password Manager page in a shell of its own (`vite build --mode mobile`, embedded by backend/mobile).
import "../styles/tokens.css";
import "../styles/base.css";
import "../styles/background.css";
import "../styles/glass.css";
import "../styles/controls.css";
import "./mobile.css";
import "../lib/settings.svelte"; // the same theme as MYLE on Windows, before first paint
import { mount } from "svelte";
import MobileApp from "./MobileApp.svelte";
export default mount(MobileApp, { target: document.getElementById("app")! });
