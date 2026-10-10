import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { transpileModule } from "typescript";

const source = await readFile(new URL("../../frontend/app/pages/password-manager/VerifyPasskey.svelte", import.meta.url), "utf8");

async function verification(api = {}) {
  let verify;
  let cancelled;
  let cleanup;
  const script = source.split('<script lang="ts">')[1].split("</script>")[0].replace(/^\s*import [^\n]+;\r?$/gm, "");
  const context = vm.createContext({
    $state: (value) => value,
    queueMicrotask() {},
    onMount: (mount) => { cleanup = mount(); },
    api: {
      verifyPending: async () => null,
      verifyAnswer: async () => {},
      onVerify: async (handler) => { verify = handler; return () => {}; },
      onVerifyCancelled: async (handler) => { cancelled = handler; return () => {}; },
      ...api,
    },
  });
  vm.runInContext(transpileModule(script, { compilerOptions: { target: 99 } }).outputText + `
    globalThis.view = {
      answer, get request() { return request; }, get busy() { return busy; },
      get master() { return master; }, get error() { return error; }, edit(value) { master = value; }
    };`, context);
  await new Promise((resolve) => setImmediate(resolve));
  return { view: context.view, verify, cancelled, cleanup };
}

test("cancelling master verification closes its own modal and preserves a newer request", async () => {
  const { view, verify, cancelled } = await verification();
  verify({ id: 2, site: "new.example" });
  view.edit("secret");
  cancelled(1);
  assert.equal(view.request.id, 2);
  assert.equal(view.master, "secret");
  cancelled(2);
  assert.equal(view.request, null);
  assert.equal(view.master, "");
});

for (const rejected of [false, true]) {
  test(`an old master answer ${rejected ? "failure" : "success"} cannot alter a newer prompt`, async () => {
    let finish;
    const { view, verify } = await verification({ verifyAnswer: () => new Promise((resolve, reject) => {
      finish = rejected ? () => reject(new Error("expired")) : resolve;
    }) });
    verify({ id: 1, site: "old.example" });
    view.edit("old master");
    const answering = view.answer(true);
    verify({ id: 2, site: "new.example" });
    view.edit("new master");
    finish();
    await answering;
    assert.equal(view.request.id, 2);
    assert.equal(view.master, "new master");
    assert.equal(view.busy, false);
    assert.equal(view.error, null);
  });
}

test("an in-flight initial pending lookup cannot restore a cancelled modal", async () => {
  let release;
  const { view, cancelled } = await verification({ verifyPending: () => new Promise((resolve) => { release = resolve; }) });
  cancelled(1);
  release({ id: 1, site: "old.example" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(view.request, null);
});

test("initial state clears a modal cancelled before its cancellation listener was installed", async () => {
  const { view } = await verification({ onVerify: async (handler) => {
    handler({ id: 1, site: "old.example" });
    return () => {};
  } });
  assert.equal(view.request, null);
});
