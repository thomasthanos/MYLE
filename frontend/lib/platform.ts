// Which app the page is built into: MYLE on Windows, or MYLE Passwords on a
// phone (`vite build --mode mobile`, see vite.config.ts). The Password
// Manager's page is the same in both; what only Windows does is left out on
// a phone, and the words fit the device.

/** MYLE Passwords on a phone. */
export const MOBILE: boolean = import.meta.env.MYLE_MOBILE === true;

const IOS = MOBILE && /iPhone|iPad|iPod/.test(navigator.userAgent);

/** This device, as the page names it ("this PC", "this phone"). */
export const DEVICE = MOBILE ? "phone" : "PC";

/** What opens the vault here without the master password. */
export const QUICK_UNLOCK = MOBILE ? (IOS ? "Face ID or Touch ID" : "your fingerprint or face") : "Windows Hello";
