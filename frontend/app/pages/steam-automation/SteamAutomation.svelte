<script lang="ts">
  // The owner's Steam section behind one sidebar button, with its own menu. The backend does the work; see backend/src/steam_automation/.
  import { onMount } from "svelte";
  import Award from "@lucide/svelte/icons/award";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import Layers from "@lucide/svelte/icons/layers";
  import Package from "@lucide/svelte/icons/package";
  import Play from "@lucide/svelte/icons/play";
  import Star from "@lucide/svelte/icons/star";
  import Timer from "@lucide/svelte/icons/timer";
  import { waitingText } from "./model";
  import { steamAutomationState as page, type View } from "./state.svelte";
  import Achievements from "./views/Achievements.svelte";
  import Cards from "./views/Cards.svelte";
  import Favorites from "./views/Favorites.svelte";
  import Games from "./views/Games.svelte";
  import Idler from "./views/Idler.svelte";
  import Idling from "./views/Idling.svelte";
  import Inventory from "./views/Inventory.svelte";

  const MENU = [
    [
      { id: "games", label: "Games", icon: Gamepad2 },
      { id: "idling", label: "Idling", icon: Play },
      { id: "favorites", label: "Favorites", icon: Star },
    ],
    [
      { id: "cards", label: "Card Farming", icon: Layers },
      { id: "achievements", label: "Achievement Unlocker", icon: Award },
      { id: "idler", label: "Automatic Idler", icon: Timer },
    ],
    [{ id: "inventory", label: "Inventory Manager", icon: Package }],
  ] as const satisfies readonly (readonly { id: View; label: string; icon: unknown }[])[];

  const status = $derived(page.status);
  const ready = $derived(status?.ready ?? false);
  const connected = $derived(ready && !!status?.communityOk);
  const running = $derived(status?.running.length ?? 0);
  const banner = $derived(waitingText(status?.waiting ?? null));
  const steamLabel = $derived(!ready ? "Checking Steam…" : status?.steamRunning ? "Steam is running" : "Steam is not running");
  const communityLabel = $derived(!ready ? "Checking sign-in…" : connected ? "Signed in to Steam" : status?.communitySignedIn ? "Sign-in has expired" : "Not signed in to Steam");

  onMount(() => {
    page.start();
    return () => page.stop();
  });
</script>

<div class="sa">
  <nav class="section-nav" aria-label="Steam Automation">
    <h1>Steam Automation</h1>
    {#each MENU as group, index (index)}
      {#if index > 0}<div class="gap"></div>{/if}
      {#each group as item (item.id)}
        <button class="item" class:on={page.view === item.id} type="button" aria-current={page.view === item.id ? "page" : undefined} onclick={() => (page.view = item.id)}>
          <item.icon size={15} />
          <span>{item.label}</span>
          {#if item.id === "idling" && running > 0}<small>{running}</small>{/if}
        </button>
      {/each}
    {/each}
    <div class="account">
      <span class="line"><span class="dot" class:on={ready && status?.steamRunning}></span>{steamLabel}</span>
      <span class="line"><span class="dot" class:on={connected}></span>{communityLabel}</span>
      {#if ready && !connected}
        <button class="btn small primary" type="button" disabled={page.signingIn} onclick={() => void page.signIn()}>{page.signingIn ? "Finish in the Steam window" : status?.communitySignedIn ? "Sign in again" : "Sign in to Steam"}</button>
      {/if}
      <button class="btn small" type="button" onclick={() => void page.setPaused(!page.settings?.paused)}>{page.settings?.paused ? "Resume" : "Pause everything"}</button>
    </div>
  </nav>

  <section class="main">
    {#if page.error}<p class="banner bad">{page.error}</p>{/if}
    {#if page.actionError}<p class="banner bad">{page.actionError}</p>{/if}
    {#if banner}<p class="banner">{banner}</p>{/if}
    {#if status?.purchaseDatesUnknown}<p class="banner">Purchase dates could not be read, so games under 2 hours are being skipped for now.</p>{/if}

    {#if page.view === "games"}<Games />
    {:else if page.view === "idling"}<Idling />
    {:else if page.view === "favorites"}<Favorites />
    {:else if page.view === "cards"}<Cards />
    {:else if page.view === "achievements"}<Achievements />
    {:else if page.view === "idler"}<Idler />
    {:else}<Inventory />{/if}
  </section>
</div>

<style>
  .sa { display: grid; grid-template-columns: 204px minmax(0, 1fr); gap: 18px; flex: 1; min-height: 0; }
  .section-nav { display: flex; flex-direction: column; gap: 2px; min-height: 0; padding-right: 14px; border-right: 1px solid var(--panel-rim); }
  h1 { margin: 0 8px 12px; color: var(--text-1); font-family: var(--font-handwritten); font-size: 22px; font-weight: 700; font-synthesis: none; line-height: 1.2; }
  .item { display: flex; align-items: center; gap: 9px; padding: 8px 10px; border: 0; border-radius: var(--radius-sm); background: none; color: var(--text-2); font: inherit; font-size: 13px; font-weight: 600; text-align: left; cursor: pointer; }
  .item:hover { background: var(--hover); }
  .item.on { background: var(--selected); color: var(--text-1); }
  .item span { flex: 1; min-width: 0; }
  .item small { color: var(--ok); font-size: 12px; font-weight: 700; }
  .gap { height: 12px; }
  .account { display: flex; flex-direction: column; gap: 6px; margin-top: auto; padding: 10px 8px 2px; border-top: 1px solid var(--panel-rim); }
  .line { display: flex; align-items: center; gap: 7px; color: var(--text-2); font-size: 12px; }
  .dot { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--idle); }
  .dot.on { background: var(--ok); }
  .main { min-width: 0; min-height: 0; overflow-y: auto; padding: 2px 6px 6px 0; }
  .banner { margin: 0 0 12px; padding: 8px 12px; border: 1px solid color-mix(in srgb, var(--warn) 34%, transparent); border-radius: var(--radius-sm); background: color-mix(in srgb, var(--warn) 13%, transparent); color: var(--text-1); font-size: 12.5px; }
  .banner.bad { border-color: color-mix(in srgb, var(--danger) 40%, transparent); background: color-mix(in srgb, var(--danger) 13%, transparent); }

  .main :global(.bar) { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 14px; }
  .main :global(.grow) { flex: 1 1 0; }
  .main :global(h2) { margin: 0; font-family: var(--font-handwritten); font-size: 20px; font-weight: 700; font-synthesis: none; }
  .main :global(.count) { padding: 2px 9px; border-radius: var(--chip-radius); background: var(--chip-fill); color: var(--text-2); font-size: 12px; font-variant-numeric: tabular-nums; }
  .main :global(.tabs) { display: inline-flex; gap: 2px; padding: 3px; border-radius: 10px; background: var(--chip-fill); }
  .main :global(.tab) { padding: 5px 12px; border: 0; border-radius: 8px; background: none; color: var(--text-2); font: inherit; font-size: 12.5px; font-weight: 600; cursor: pointer; }
  .main :global(.tab:hover) { color: var(--text-1); }
  .main :global(.tab.on) { background: var(--selected); color: var(--text-1); }
  .main :global(.tab i) { margin-left: 6px; color: var(--text-3); font-style: normal; font-weight: 400; font-variant-numeric: tabular-nums; }
  .main :global(.auto) { display: inline-flex; align-items: center; gap: 8px; height: 32px; padding: 0 10px; border: 1px solid var(--btn-border); border-radius: 9px; background: var(--chip-fill); color: var(--text-2); font-size: 12.5px; font-weight: 600; cursor: pointer; }
  .main :global(.note) { margin: -6px 0 12px; color: var(--text-3); font-size: 12.5px; }
  .main :global(.note.warn) { margin: 0 0 12px; color: var(--warn); }

  @media (max-width: 760px) {
    .sa { grid-template-columns: minmax(0, 1fr); }
    .section-nav { padding-right: 0; border-right: 0; }
    .account { margin-top: 10px; }
  }
</style>
