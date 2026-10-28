import "./styles/tokens.css";
import "./styles/base.css";
import { mount } from "svelte";
import Notice from "./notice/Notice.svelte";
import ContextMenuHost from "./lib/components/ContextMenuHost.svelte";
import { hardenWebview } from "./lib/desktop";
hardenWebview();
mount(ContextMenuHost, { target: document.body });
export default mount(Notice, { target: document.getElementById("notice")! });
