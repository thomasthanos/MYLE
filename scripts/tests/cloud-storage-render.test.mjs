import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("Disk Analyzer shows one online-only button with an info, and no dialog or app buttons", async () => {
  const server = await createServer({ server: { middlewareMode: true }, logLevel: "error" });
  try {
    // Use one module graph for Svelte and the component's SSR runtime.
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: CloudStorage } = await server.ssrLoadModule("/app/pages/disk-analyzer/cloud-storage/CloudStorage.svelte");
    const { body } = render(CloudStorage);
    assert.equal(body.match(/<button/g)?.length, 1, "only the one button");
    assert.ok(body.includes("Make all online-only"));
    assert.match(body, /class="info[^"]*"[^>]*title="[^"]*stay on this PC/, "the info explains what stays");
    assert.ok(!body.includes("<dialog"), "no review window");
  } finally {
    await server.close();
  }
});
