import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("Disk Analyzer renders all cloud providers without a closed dialog blocking global shortcuts", async () => {
  const server = await createServer({ server: { middlewareMode: true }, logLevel: "error" });
  try {
    // Use one module graph for Svelte and the component's SSR runtime.
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: CloudStorage } = await server.ssrLoadModule("/app/pages/disk-analyzer/cloud-storage/CloudStorage.svelte");
    const { body } = render(CloudStorage);
    for (const name of ["Google Drive", "MEGA", "Dropbox", "OneDrive"]) {
      assert.ok(body.includes(`>${name}</span>`), `${name} button is visible`);
    }
    assert.ok(body.includes("<dialog"));
    assert.ok(!body.includes('aria-modal="true"'), "closed dialogs must not match the app's modal keyboard guard");
  } finally {
    await server.close();
  }
});
