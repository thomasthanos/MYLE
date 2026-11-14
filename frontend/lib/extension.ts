/* MYLE Passwords' page in the Chrome Web Store (package.json "myle.extensionUrl", put in by vite.config.ts); empty when the build has none. */
export const EXTENSION_URL: string = import.meta.env.VITE_EXTENSION_URL ?? "";

/* MYLE Passwords' page in Firefox Add-ons (AMO) (package.json "myle.firefoxExtensionUrl", put in by vite.config.ts); empty when none. */
export const FIREFOX_EXTENSION_URL: string = import.meta.env.VITE_FIREFOX_EXTENSION_URL ?? "";
