<script lang="ts">
  // Card farming, idling, achievements and selling duplicates, each with its own switch. The backend does the work; see backend/src/steam_automation/.
  import { onMount } from "svelte";
  import PageHeader from "../../../lib/components/PageHeader.svelte";
  import type { Settings } from "./api";
  import GamePicker from "./GamePicker.svelte";
  import { euros, farmLine, plural, untilText, validMinimum, waitingText, type PickerKind } from "./model";
  import { steamAutomationState as page } from "./state.svelte";

  let now = $state(Date.now() / 1000);
  let picker = $state<PickerKind | null>(null);

  const status = $derived(page.status);
  const settings = $derived(page.settings);
  const ready = $derived(status?.ready ?? false);
  const connected = $derived(ready && !!status?.communityOk);
  const banner = $derived(waitingText(status?.waiting ?? null));
  const farming = $derived(!!settings?.farmOn && !!status && status.farm.phase !== "done");
  const activity = $derived([...(status?.activity ?? [])].reverse().slice(0, 30));
  const unlocked = $derived(status ? status.achievements.total - status.achievements.locked : 0);
  const steamLabel = $derived(!ready ? "Checking Steam…" : status?.steamRunning ? "Steam is running, signed in" : "Steam is not running");
  const communityLabel = $derived(!ready ? "Checking Steam Community…" : connected ? "Steam Community connected" : status?.communitySignedIn ? "Steam Community needs signing in again" : "Steam Community not connected");

  const time = (at: number) => new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" }).format(new Date(at * 1000));

  function toggle(name: keyof Pick<Settings, "farmOn" | "idleOn" | "achievementsOn" | "inventoryOn">, event: Event) {
    void page.change({ [name]: (event.currentTarget as HTMLInputElement).checked });
  }

  function setMinimum(event: Event) {
    const cents = Number((event.currentTarget as HTMLInputElement).value);
    if (validMinimum(cents)) void page.change({ minimumCents: cents });
  }

  function savePicked(games: number[]) {
    void page.change(picker === "idle" ? { idleGames: games } : { achievementGames: games });
    picker = null;
  }

  onMount(() => {
    page.start();
    const clock = setInterval(() => (now = Date.now() / 1000), 15_000);
    return () => {
      clearInterval(clock);
      page.stop();
    };
  });
</script>

<PageHeader title="Steam Automation" subtitle="Cards, playtime, achievements and your inventory, on their own." />

<div class="status">
  <span class="pill"><span class="dot" class:on={ready && status?.steamRunning}></span>{steamLabel}</span>
  <span class="pill"><span class="dot" class:on={connected}></span>{communityLabel}</span>
  <span class="spacer"></span>
  {#if ready && !connected}
    <button class="btn primary" type="button" disabled={page.signingIn} onclick={() => void page.signIn()}>{page.signingIn ? "Finish signing in in the Steam window" : status?.communitySignedIn ? "Sign in again" : "Sign in to Steam"}</button>
  {/if}
  <button class="btn" type="button" onclick={() => void page.setPaused(!page.paused)}>{page.paused ? "Resume" : "Pause everything"}</button>
</div>

{#if page.error}<p class="banner bad">{page.error}</p>{/if}
{#if banner}<p class="banner">{banner}</p>{/if}
{#if status?.purchaseDatesUnknown}<p class="banner">Purchase dates could not be read, so games under 2 hours are being skipped for now.</p>{/if}

{#if settings && status}
  <div class="grid">
    <section class="surface card">
      <header><h2>Card farming</h2><input class="switch" type="checkbox" aria-label="Card farming" checked={settings.farmOn} onchange={(event) => toggle("farmOn", event)} /></header>
      {#if !connected}
        <p class="line">Sign in to Steam to see your cards.</p>
      {:else}
        <p class="line">{settings.farmOn ? farmLine(status.farm) : "Off"}</p>
      {/if}
      <dl>
        <div><dt>Next check for drops</dt><dd>{untilText(status.farm.nextCheckAt, now) || "–"}</dd></div>
        <div><dt>Skipped, still refundable</dt><dd>{plural(status.farm.skippedRefundable, "game")}</dd></div>
      </dl>
    </section>

    <section class="surface card">
      <header><h2>Automatic idler</h2><input class="switch" type="checkbox" aria-label="Automatic idler" checked={settings.idleOn} onchange={(event) => toggle("idleOn", event)} /></header>
      <p class="line">{!settings.idleOn ? "Off" : farming ? "Waiting for card farming to finish" : `${plural(settings.idleGames.length, "game")} adding playtime`}</p>
      <dl>
        <div><dt>Games chosen</dt><dd>{settings.idleGames.length}</dd></div>
      </dl>
      <button class="link" type="button" disabled={!connected} onclick={() => (picker = "idle")}>Choose games</button>
    </section>

    <section class="surface card">
      <header><h2>Achievement unlocker</h2><input class="switch" type="checkbox" aria-label="Achievement unlocker" checked={settings.achievementsOn} onchange={(event) => toggle("achievementsOn", event)} /></header>
      <p class="line">{!settings.achievementsOn ? "Off" : status.achievements.waitingForFarming ? "Waiting for card farming to finish" : `${plural(settings.achievementGames.length, "game")} chosen`}</p>
      <dl>
        <div><dt>Unlocked so far</dt><dd>{unlocked} of {status.achievements.total}</dd></div>
        <div><dt>Next unlock</dt><dd>{untilText(status.achievements.nextAt, now) || "–"}</dd></div>
      </dl>
      <button class="link" type="button" disabled={!connected} onclick={() => (picker = "achievements")}>Choose games</button>
    </section>

    <section class="surface card">
      <header><h2>Inventory manager</h2><input class="switch" type="checkbox" aria-label="Inventory manager" checked={settings.inventoryOn} onchange={(event) => toggle("inventoryOn", event)} /></header>
      <p class="line">{settings.inventoryOn ? "Sells duplicates only, one of each is always kept" : "Off"}</p>
      <dl>
        <div><dt>Listed in the last run</dt><dd>{plural(status.inventory.listed, "item")}, {euros(status.inventory.cents)}</dd></div>
        <div><dt>Next run</dt><dd>{untilText(status.inventory.nextAt, now) || "–"}</dd></div>
        <div><dt>Never below (cents)</dt><dd><input class="input small" type="number" min="1" step="1" value={settings.minimumCents} aria-label="Minimum price in cents" onchange={setMinimum} /></dd></div>
      </dl>
      {#if status.inventory.waitingConfirmation > 0}
        <p class="banner tight">{plural(status.inventory.waitingConfirmation, "listing")} waiting for your confirmation in Steam Guard.</p>
      {/if}
    </section>
  </div>

  <h2 class="activity-title">Activity</h2>
  <div class="surface activity">
    {#each activity as entry (entry.at + entry.text)}
      <div class="entry"><time>{time(entry.at)}</time><span>{entry.text}</span></div>
    {:else}
      <p class="empty">Nothing yet. What happens will be written here.</p>
    {/each}
  </div>
{/if}

{#if picker && settings}
  <GamePicker kind={picker} selected={picker === "idle" ? settings.idleGames : settings.achievementGames} onSave={savePicked} onClose={() => (picker = null)} />
{/if}

<style>
  .status { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 12px; }
  .pill { display: inline-flex; align-items: center; gap: 7px; padding: 5px 11px; border: 1px solid var(--btn-border); border-radius: var(--chip-radius); background: var(--chip-fill); color: var(--text-2); font-size: 12px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--idle); }
  .dot.on { background: var(--ok); }
  .spacer { flex: 1; }
  .banner { margin: 0 0 12px; padding: 8px 12px; border: 1px solid color-mix(in srgb, var(--warn) 34%, transparent); border-radius: var(--radius-sm); background: color-mix(in srgb, var(--warn) 13%, transparent); color: var(--text-1); font-size: 12.5px; }
  .banner.bad { border-color: color-mix(in srgb, var(--danger) 40%, transparent); background: color-mix(in srgb, var(--danger) 13%, transparent); }
  .banner.tight { margin: 10px 0 0; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 12px; }
  .card { padding: 13px 14px; min-width: 0; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h2 { margin: 0; font-size: 14.5px; font-weight: 700; }
  .line { margin: 4px 0 0; color: var(--text-2); font-size: 13px; }
  dl { display: grid; gap: 4px; margin: 10px 0 0; font-size: 12px; }
  dl div { display: flex; justify-content: space-between; align-items: center; gap: 10px; }
  dt { color: var(--text-2); }
  dd { margin: 0; font-weight: 700; }
  .small { width: 70px; height: 26px; text-align: right; }
  .link { margin-top: 10px; padding: 0; border: 0; background: none; color: var(--accent); font: inherit; font-size: 12.5px; cursor: pointer; }
  .link:hover:not(:disabled), .link:focus-visible { text-decoration: underline; }
  .link:disabled { color: var(--text-3); cursor: not-allowed; }
  .activity-title { margin: 18px 0 8px; }
  .activity { padding: 6px 12px; }
  .entry { display: flex; gap: 14px; padding: 5px 0; color: var(--text-2); font-size: 12.5px; }
  .entry + .entry { border-top: 1px solid var(--panel-rim); }
  time { flex: none; color: var(--text-3); }
  .empty { margin: 6px 0; color: var(--text-3); font-size: 12.5px; }
</style>
