// Reading a site's 2FA QR code with the phone's camera. The camera shows
// behind the page (`windowed`), so the page draws the frame and the Cancel
// button itself (`Scanner.svelte`); everything else goes see-through.
import { isTauri } from "@tauri-apps/api/core";
import { cancel, checkPermissions, Format, openAppSettings, requestPermissions, scan } from "@tauri-apps/plugin-barcode-scanner";
import { toast } from "../lib/toast.svelte";

class Scanner {
  active = $state(false);

  /** The otpauth:// link in the QR code; null when the user cancels. */
  async readTotp(): Promise<string | null> {
    if (!isTauri()) {
      // In a plain browser: a stand-in, to work on the page.
      return "otpauth://totp/GitHub:thomas?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=GitHub";
    }
    let permission = await checkPermissions();
    if (permission !== "granted") permission = await requestPermissions();
    if (permission !== "granted") {
      toast.error("MYLE Passwords may not use the camera.", { label: "Settings", run: () => void openAppSettings() });
      return null;
    }
    this.active = true;
    document.documentElement.classList.add("scanning");
    try {
      const { content } = await scan({ windowed: true, formats: [Format.QRCode] });
      const link = content.trim();
      if (!/^otpauth:\/\//i.test(link)) throw new Error("That QR code is not for 2FA codes (it has no otpauth:// link).");
      return link;
    } catch (error) {
      if (/cancel/i.test(String(error))) return null;
      throw error;
    } finally {
      this.active = false;
      document.documentElement.classList.remove("scanning");
    }
  }

  stop() {
    void cancel().catch(() => {});
  }
}

export const scanner = new Scanner();
