<script lang="ts">
  // The owner's Steam section, in four parts across the top. The backend does the work; see backend/src/steam_automation/.
  import { onMount } from "svelte";
  import Award from "@lucide/svelte/icons/award";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import ClipboardList from "@lucide/svelte/icons/clipboard-list";
  import Layers from "@lucide/svelte/icons/layers";
  import Package from "@lucide/svelte/icons/package";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import Popover from "../../../lib/components/Popover.svelte";
  import ProgressBar from "../../../lib/components/ProgressBar.svelte";
  import { plural, waitingText } from "./model";
  import { steamAutomationState as page, type View } from "./state.svelte";
  import Achievements from "./views/Achievements.svelte";
  import Cards from "./views/Cards.svelte";
  import Games from "./views/Games.svelte";
  import Inventory from "./views/Inventory.svelte";

  const TABS = [
    { id: "games", label: "Games", icon: Gamepad2 },
    { id: "cards", label: "Card farming", icon: Layers },
    { id: "achievements", label: "Achievements", icon: Award },
    { id: "inventory", label: "Inventory", icon: Package },
  ] as const satisfies readonly { id: View; label: string; icon: unknown }[];

  const status = $derived(page.status);
  const ready = $derived(status?.ready ?? false);
  const connected = $derived(ready && !!status?.communityOk && !status?.storeStale);
  const paused = $derived(page.settings?.paused ?? false);
  const banner = $derived(waitingText(status?.waiting ?? null));
  const runningGames = $derived(
    (status?.running ?? []).map((appid) => {
      const game = page.library?.find((entry) => entry.appid === appid);
      return { appid, name: game?.name ?? `Game ${appid}`, art: game?.headerUrl ?? null };
    }),
  );
  const activity = $derived([...(status?.activity ?? [])].reverse().slice(0, 30));
  const counts = $derived<Record<View, number | null>>({
    games: page.library?.length ?? null,
    cards: page.drops ? page.drops.reduce((sum, game) => sum + game.drops, 0) : null,
    achievements: page.settings?.achievementGames.length || null,
    inventory: page.inventory?.length ?? null,
  });
  const runningText = $derived(runningGames.length === 1 ? runningGames[0].name : plural(runningGames.length, "game"));

  const time = (at: number) => new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" }).format(new Date(at * 1000));

  onMount(() => {
    page.start();
    return () => page.stop();
  });
</script>

<div class="sa">
  <header class="head">
    <h1>Steam Automation</h1>
    <span class="grow"></span>
    <span class="pill" title={ready && status?.steamRunning ? "The Steam app is open on this PC" : "Open the Steam app on this PC"}><span class="light" class:on={ready && status?.steamRunning}></span>{ready ? (status?.steamRunning ? "Steam is open" : "Steam is closed") : "Checking Steam…"}</span>
    {#if ready && !connected}
      <button class="btn small primary" type="button" disabled={page.signingIn} onclick={() => void page.signIn()}>{page.signingIn ? "Finish in the Steam window" : status?.communitySignedIn ? "Sign in again" : "Sign in to Steam"}</button>
    {:else if connected}
      <span class="pill"><span class="light on"></span>Signed in</span>
    {/if}
    <button class="btn small" type="button" title={paused ? "Let everything run again" : "Stop everything until you press Resume"} onclick={() => void page.setPaused(!paused)}>{#if paused}<Play size={13} />Resume{:else}<Pause size={13} />Pause{/if}</button>
    <Popover align="end">
      {#snippet trigger({ toggle, open })}
        <button class="icon-btn" class:active={open} type="button" aria-label="What MYLE has done" title="What MYLE has done" onclick={toggle}><ClipboardList size={15} /></button>
      {/snippet}
      {#snippet children()}
        <div class="activity menu">
          {#each activity as entry, index (index)}
            <div class="entry"><time>{time(entry.at)}</time><span>{entry.text}</span></div>
          {:else}
            <p class="none">Nothing yet. What happens will be written here.</p>
          {/each}
        </div>
      {/snippet}
    </Popover>
  </header>

  <nav class="tabs" aria-label="Steam Automation">
    {#each TABS as tab (tab.id)}
      <button class="tab" class:active={page.view === tab.id} type="button" aria-current={page.view === tab.id ? "page" : undefined} onclick={() => (page.view = tab.id)}>
        <tab.icon size={15} />{tab.label}{#if counts[tab.id] !== null}<i>{counts[tab.id]?.toLocaleString("en-US")}</i>{/if}
      </button>
    {/each}
  </nav>

  <section class="main">
    {#if page.error}<p class="banner bad">{page.error}</p>{/if}
    {#if page.actionError}<p class="banner bad">{page.actionError}</p>{/if}
    {#if banner}<p class="banner">{banner}</p>{/if}
    {#if status?.purchaseDatesUnknown}<p class="banner">Purchase dates could not be read, so games under 2 hours are left alone for now.</p>{/if}
    {#if page.waitingConfirmation > 0}<p class="banner">{plural(page.waitingConfirmation, "listing")} waiting for you to confirm in Steam Guard, on your phone. MYLE never approves these for you.</p>{/if}

    {#if page.job}
      <div class="strip surface">
        <div class="strip-line"><b>{page.job.label}</b><span>{page.job.progress.done} of {page.job.total}</span><span class="grow"></span><button class="btn small" type="button" onclick={() => page.stopJob()}>Stop</button></div>
        <ProgressBar value={page.job.progress.done / page.job.total} label={page.job.label} />
      </div>
    {/if}

    {#if runningGames.length > 0}
      <div class="strip surface">
        <div class="strip-line">
          <span class="light on"></span>
          <b>Running now</b>
          <span class="faces">{#each runningGames.slice(0, 6) as game (game.appid)}{#if game.art}<img src={game.art} alt="" title={game.name} />{/if}{/each}</span>
          <span class="what">{runningText}</span>
          <span class="grow"></span>
          <button class="btn small" type="button" onclick={() => void page.stopAll()}>Stop all</button>
        </div>
      </div>
    {/if}

    {#if page.view === "games"}<Games />
    {:else if page.view === "cards"}<Cards />
    {:else if page.view === "achievements"}<Achievements />
    {:else}<Inventory />{/if}
  </section>
</div>

<style>
  .sa { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .head { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin-bottom: 10px; }
  h1 { margin: 0; color: var(--text-1); font-family: var(--font-handwritten); font-size: 26px; font-weight: 700; font-synthesis: none; line-height: 1.2; }
  .grow { flex: 1 1 0; }
  .pill { display: inline-flex; align-items: center; gap: 7px; color: var(--text-2); font-size: 12px; }
  .light { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--idle); }
  .light.on { background: var(--ok); }
  .tabs { display: flex; flex-wrap: wrap; gap: 4px; border-bottom: 1px solid var(--panel-rim); }
  .tab { position: relative; display: inline-flex; align-items: center; gap: 8px; min-height: 40px; padding: 0 14px; border: 0; background: none; color: var(--text-3); font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; }
  .tab:hover { color: var(--text-1); }
  .tab.active { color: var(--text-1); }
  .tab.active::after { content: ""; position: absolute; right: 10px; bottom: -1px; left: 10px; height: 2px; border-radius: 2px 2px 0 0; background: var(--accent-grad); box-shadow: 0 0 9px var(--accent-glow, transparent); }
  .tab i { color: var(--text-3); font-size: 11.5px; font-style: normal; font-weight: 500; font-variant-numeric: tabular-nums; }
  .main { flex: 1; min-width: 0; min-height: 0; overflow-y: auto; padding: 14px 6px 6px 0; }
  .banner { margin: 0 0 12px; padding: 8px 12px; border: 1px solid color-mix(in srgb, var(--warn) 34%, transparent); border-radius: var(--radius-sm); background: color-mix(in srgb, var(--warn) 13%, transparent); color: var(--text-1); font-size: 12.5px; }
  .banner.bad { border-color: color-mix(in srgb, var(--danger) 40%, transparent); background: color-mix(in srgb, var(--danger) 13%, transparent); }
  .strip { display: grid; gap: 7px; margin-bottom: 12px; padding: 8px 12px; }
  .strip-line { display: flex; align-items: center; gap: 10px; min-width: 0; font-size: 12.5px; }
  .faces { display: inline-flex; gap: 4px; }
  .faces img { width: 44px; aspect-ratio: 460 / 215; border-radius: 4px; object-fit: cover; }
  .what { overflow: hidden; color: var(--text-2); white-space: nowrap; text-overflow: ellipsis; }
  .activity { width: min(420px, 80vw); max-height: 340px; overflow-y: auto; padding: 6px 10px; }
  .entry { display: flex; gap: 12px; padding: 5px 0; color: var(--text-2); font-size: 12px; }
  .entry + .entry { border-top: 1px solid var(--panel-rim); }
  time { flex: none; color: var(--text-3); }
  .none { margin: 6px 0; color: var(--text-3); font-size: 12.5px; }

  .main :global(.count) { color: var(--text-3); font-variant-numeric: tabular-nums; }
  .main :global(.auto) { display: inline-flex; align-items: center; gap: 8px; color: var(--text-2); font-size: 12.5px; font-weight: 600; cursor: pointer; }
  .main :global(.note) { margin: -4px 0 12px; color: var(--text-3); font-size: 12.5px; }
  .main :global(.note.inline) { margin: 0; }
</style>
