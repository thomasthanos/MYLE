<script lang="ts">
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import { plural, runningSource, type RunningSource } from "../model";
  import { steamAutomationState as page } from "../state.svelte";

  const SOURCE: Record<RunningSource, string> = { hand: "Idling by hand", idler: "Automatic idler", farming: "Card farming" };

  const rows = $derived.by(() => {
    const status = page.status;
    const settings = page.settings;
    if (!status || !settings) return [];
    return status.running.map((appid) => {
      const game = page.library?.find((entry) => entry.appid === appid);
      return { appid, name: game?.name ?? `Game ${appid}`, art: game?.headerUrl ?? null, source: runningSource(appid, status, settings) };
    });
  });
  const activity = $derived([...(page.status?.activity ?? [])].reverse().slice(0, 30));

  const time = (at: number) => new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" }).format(new Date(at * 1000));

  function stop(appid: number, source: RunningSource) {
    if (source === "hand") void page.idleGame(appid, false);
    else if (source === "idler") page.toggleQueue("idleGames", appid);
    else void page.setStarted("farm", false);
  }
</script>

<div class="bar">
  <h2>Idling now</h2>
  <span class="count">{plural(rows.length, "game")}</span>
  <span class="grow"></span>
  <button class="btn" type="button" disabled={rows.length === 0} onclick={() => void page.stopAll()}>Stop all</button>
</div>

{#each rows as row (row.appid)}
  <div class="run surface">
    {#if row.art}<img src={row.art} alt="" loading="lazy" />{:else}<span class="blank"><Gamepad2 size={15} /></span>{/if}
    <div class="what"><b>{row.name}</b><span>{SOURCE[row.source]}</span></div>
    <button class="btn small" type="button" onclick={() => stop(row.appid, row.source)}>{row.source === "farming" ? "Stop farming" : "Stop"}</button>
  </div>
{:else}
  <p class="empty">Nothing is idling. Press play on a game in Games, or start a feature.</p>
{/each}

<h2 class="activity-title">Activity</h2>
<div class="surface activity">
  {#each activity as entry (entry.at + entry.text)}
    <div class="entry"><time>{time(entry.at)}</time><span>{entry.text}</span></div>
  {:else}
    <p class="empty small">Nothing yet. What happens will be written here.</p>
  {/each}
</div>

<style>
  .run { display: grid; grid-template-columns: 62px minmax(0, 1fr) auto; align-items: center; gap: 11px; margin-bottom: 7px; padding: 7px 10px; }
  img, .blank { width: 62px; aspect-ratio: 460 / 215; border-radius: 5px; object-fit: cover; }
  .blank { display: grid; place-items: center; color: var(--text-3); background: var(--input-fill); }
  .what { display: grid; min-width: 0; }
  .what b { overflow: hidden; font-size: 13px; white-space: nowrap; text-overflow: ellipsis; }
  .what span { color: var(--text-3); font-size: 11.5px; }
  .empty { margin: 24px auto; color: var(--text-3); font-size: 13px; text-align: center; }
  .empty.small { margin: 6px 0; font-size: 12.5px; text-align: left; }
  .activity-title { margin: 18px 0 8px; }
  .activity { padding: 6px 12px; }
  .entry { display: flex; gap: 14px; padding: 5px 0; color: var(--text-2); font-size: 12.5px; }
  .entry + .entry { border-top: 1px solid var(--panel-rim); }
  time { flex: none; color: var(--text-3); }
</style>
