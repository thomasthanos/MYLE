<script lang="ts">
  import { onMount } from "svelte";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import { steamAutomationApi, type Status } from "./api";

  let status = $state<Status | null>(null);
  let busy = $state(false);
  let message = $state("");

  const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

  async function refresh() {
    status = await steamAutomationApi.status();
  }

  async function run(work: () => Promise<string | void>) {
    busy = true;
    message = "";
    try {
      message = (await work()) ?? "";
      await refresh();
    } catch (error) {
      message = messageOf(error);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void refresh();
  });
</script>

<PageHeader title="Steam Automation" subtitle="Cards, playtime, achievements and your inventory, on their own." />

<div class="row">
  <span class="pill"><span class="dot" class:on={status?.steamRunning}></span>{status?.steamRunning ? "Steam is running, signed in" : "Steam is not running"}</span>
  <span class="pill"><span class="dot" class:on={status?.communitySignedIn}></span>{status?.communitySignedIn ? "Steam Community connected" : "Steam Community not connected"}</span>
  {#if status?.communitySignedIn && status?.steamRunning && !status.sameAccount}
    <span class="pill warn">Different accounts</span>
  {/if}
</div>

<div class="row">
  <button class="btn" type="button" disabled={busy} onclick={() => run(steamAutomationApi.signIn)}>Sign in to Steam</button>
  <button class="btn" type="button" disabled={busy || !status?.communitySignedIn} onclick={() => run(steamAutomationApi.capture)}>Capture</button>
</div>

{#if message}<pre class="message selectable">{message}</pre>{/if}

<style>
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 12px; }
  .pill { display: inline-flex; align-items: center; gap: 7px; padding: 5px 11px; border: 1px solid var(--btn-border); border-radius: var(--chip-radius); background: var(--chip-fill); color: var(--text-2); font-size: 12px; }
  .pill.warn { color: var(--warn); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--idle); }
  .dot.on { background: var(--ok); }
  .message { margin: 0; padding: 10px 12px; border: 1px solid var(--glass-border); border-radius: var(--radius-sm); background: var(--input-fill); color: var(--text-2); font-family: var(--font-mono); font-size: 12px; white-space: pre-wrap; }
</style>
