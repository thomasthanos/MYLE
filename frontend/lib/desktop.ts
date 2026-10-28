import { blockedWebviewShortcut } from "./webview-policy";
// Native WebView2 settings also disable its menu and release DevTools.
export function hardenWebview(): void {
  if (import.meta.env.DEV) return;
  document.addEventListener("contextmenu", (e) => {
    e.preventDefault();
  }, true);
  document.addEventListener("keydown", (e) => {
    if (!blockedWebviewShortcut(e, false)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }, true);
}
