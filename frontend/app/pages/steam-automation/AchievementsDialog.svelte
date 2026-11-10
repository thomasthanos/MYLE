<script lang="ts">
  // One game's achievements: unlock or lock any of them now, or let MYLE unlock the rest one at a time. Steam on this PC says what is unlocked; Steam's public list adds the words, the icons and how many players have each.
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { fade, fly, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Search from "@lucide/svelte/icons/search";
  import Trophy from "@lucide/svelte/icons/trophy";
  import X from "@lucide/svelte/icons/x";
  import Select from "../../../lib/components/Select.svelte";
  import { confirm, confirmState } from "../../../lib/confirm.svelte";
  import { portal } from "../../../lib/portal";
  import { toast } from "../../../lib/toast.svelte";
  import { dialogFocus } from "../game-saves/dialog-focus";
  import { steamAutomationApi as api, type Achievement, type GameAchievements, type LibraryGame } from "./api";
  import { ACHIEVEMENT_SORTS, achievementProgress, changeText, filterAchievements, messageOf, namesToChange, percentText, plural, rarity, sortAchievements, type AchievementFilter, type AchievementSort } from "./model";
  import { steamAutomationState as page } from "./state.svelte";

  let { game, onclose }: { game: LibraryGame; onclose: () => void } = $props();

  const FILTERS: { value: AchievementFilter; label: string }[] = [
    { value: "all", label: "All" },
    { value: "locked", label: "Locked" },
    { value: "unlocked", label: "Unlocked" },
  ];
  const RADIUS = 34;
  const CIRCLE = 2 * Math.PI * RADIUS;

  let data = $state<GameAchievements | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  /** What is being changed: "unlock-all", "lock-all", "selected", or one achievement's name. */
  let busy = $state<string | null>(null);
  let query = $state("");
  let filter = $state<AchievementFilter>("all");
  let sort = $state<AchievementSort>("game");
  let cover = $state(0);
  let search = $state<HTMLInputElement>();
  const selected = new SvelteSet<string>();

  const list = $derived(data?.achievements ?? []);
  const progress = $derived(achievementProgress(list));
  const counts = $derived<Record<AchievementFilter, number>>({ all: list.length, locked: list.length - progress.unlocked, unlocked: progress.unlocked });
  const shown = $derived(sortAchievements(filterAchievements(list, filter, query), sort));
  const allShown = $derived(shown.length > 0 && shown.every((achievement) => selected.has(achievement.name)));
  const unlockPicked = $derived(namesToChange(list, true, selected));
  const lockPicked = $derived(namesToChange(list, false, selected));
  const auto = $derived(page.settings?.achievementGames.includes(game.appid) ?? false);
  const covers = $derived([game.coverUrl, game.headerUrl].filter((url): url is string => !!url));

  async function load() {
    loading = true;
    error = null;
    try {
      data = await api.achievements(game.appid);
      page.noteAchievements(game.appid, achievementProgress(data.achievements));
      const names = new Set(data.achievements.map((achievement) => achievement.name));
      for (const name of [...selected]) if (!names.has(name)) selected.delete(name);
    } catch (err) {
      error = messageOf(err);
    } finally {
      loading = false;
    }
  }

  /** Sends `names` to Steam and shows what it did, without reading the whole list again. */
  async function change(names: string[], unlock: boolean, what: string) {
    if (!names.length || busy || !data) return;
    busy = what;
    try {
      const outcome = await api.setAchievements(game.appid, names, unlock);
      const changed = new Set(outcome.changed);
      data = { ...data, achievements: data.achievements.map((achievement) => (changed.has(achievement.name) ? { ...achievement, unlocked: unlock } : achievement)) };
      page.noteAchievements(game.appid, achievementProgress(data.achievements));
      for (const name of outcome.changed) selected.delete(name);
      const text = changeText(outcome, unlock);
      if (outcome.changed.length) toast.success(text);
      else toast.error(text || "Nothing was changed.");
    } catch (err) {
      toast.error(messageOf(err));
    } finally {
      busy = null;
    }
  }

  async function unlockAll() {
    const names = namesToChange(list, true);
    const ok = await confirm({
      title: `Unlock ${plural(names.length, "achievement")}?`,
      message: `Every locked achievement in ${game.name} is unlocked now. They show on your Steam profile with today's date.`,
      confirmLabel: "Unlock all",
    });
    if (ok) await change(names, true, "unlock-all");
  }

  async function lock(names: string[], what: string) {
    const ok = await confirm({
      title: `Lock ${plural(names.length, "achievement")}?`,
      message: `Steam forgets when you unlocked ${names.length === 1 ? "it" : "them"}. Unlocking again gives today's date, so this cannot be undone.`,
      confirmLabel: names.length === 1 ? "Lock it" : "Lock them",
      danger: true,
    });
    if (ok) await change(names, false, what);
  }

  function pick(name: string) {
    if (selected.has(name)) selected.delete(name);
    else selected.add(name);
  }

  function pickShown() {
    for (const achievement of shown) {
      if (allShown) selected.delete(achievement.name);
      else selected.add(achievement.name);
    }
  }

  const iconOf = (achievement: Achievement) => (achievement.unlocked ? achievement.icon : (achievement.iconGray ?? achievement.icon));
  const tone = (percent: number | null) => rarity(percent).toLowerCase().replace(" ", "-");

  // Before the confirm dialog's own handler: Escape there answers it and leaves this window open.
  function onKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || confirmState.current || busy) return;
    event.preventDefault();
    onclose();
  }

  onMount(() => {
    void load();
    requestAnimationFrame(() => search?.focus());
  });
</script>

<svelte:window onkeydowncapture={onKey} />

<div class="backdrop" role="presentation" transition:fade={{ duration: 140 }} {@attach portal} onclick={(event) => event.target === event.currentTarget && !busy && onclose()}>
  <div class="dialog glass glass--3" role="dialog" aria-modal="true" aria-labelledby="achievements-title" tabindex="-1" {@attach dialogFocus} transition:scale={{ start: 0.97, duration: 170, easing: cubicOut }}>
    <aside class="side">
      <div class="cover">
        {#if covers[cover]}
          <img src={covers[cover]} alt="" onerror={() => (cover += 1)} />
        {:else}
          <span class="blank"><Gamepad2 size={30} /></span>
        {/if}
      </div>
      <h2 id="achievements-title">{game.name}</h2>
      <div class="ring" class:full={data && progress.total > 0 && progress.unlocked === progress.total}>
        <svg viewBox="0 0 80 80" aria-hidden="true">
          <circle class="track" cx="40" cy="40" r={RADIUS} />
          <circle class="fill" cx="40" cy="40" r={RADIUS} stroke-dasharray={CIRCLE} stroke-dashoffset={CIRCLE * (1 - progress.percent / 100)} />
        </svg>
        <strong>{data ? `${progress.percent}%` : "–"}</strong>
      </div>
      <p class="count">
        {#if data}{progress.unlocked} / {progress.total} unlocked{:else if loading}Reading from Steam…{:else}Not read{/if}
      </p>
      <div class="side-actions">
        <button class="btn primary" type="button" disabled={!data || !!busy || counts.locked === 0} onclick={unlockAll}>
          {#if busy === "unlock-all"}<LoaderCircle class="spin" size={15} />{:else}<LockOpen size={15} />{/if} Unlock all
        </button>
        <button class="btn" type="button" disabled={!data || !!busy || counts.unlocked === 0} onclick={() => lock(namesToChange(list, false), "lock-all")}>
          {#if busy === "lock-all"}<LoaderCircle class="spin" size={15} />{:else}<Lock size={15} />{/if} Lock all
        </button>
      </div>
      <label class="auto">
        <input class="switch" type="checkbox" checked={auto} disabled={!page.settings} onchange={() => page.toggleAchievementGame(game.appid)} />
        <span>Unlock the rest automatically</span>
      </label>
      <p class="note">{auto ? "One at a time, 5 to 30 minutes apart, while Achievements is started." : "Off: nothing in this game changes unless you ask here."}</p>
      <p class="note quiet">Reading or changing achievements shows this game as running on Steam for a moment.</p>
    </aside>

    <section class="main">
      <header>
        <h3>Achievements</h3>
        <button class="close" type="button" aria-label="Close" disabled={!!busy} onclick={onclose}><X size={17} /></button>
      </header>

      <div class="tools">
        <label class="search">
          <Search size={14} />
          <input bind:this={search} class="input" type="search" placeholder="Search" aria-label="Search achievements" bind:value={query} />
        </label>
        <div class="filters" role="group" aria-label="Show">
          {#each FILTERS as option (option.value)}
            <button class="chip" class:active={filter === option.value} type="button" aria-pressed={filter === option.value} onclick={() => (filter = option.value)}>{option.label}<span class="count">{counts[option.value]}</span></button>
          {/each}
        </div>
        <div class="sort"><Select bind:value={sort} options={ACHIEVEMENT_SORTS} ariaLabel="Sort achievements" size="sm" align="end" /></div>
      </div>

      {#if selected.size}
        <div class="picked" role="toolbar" aria-label="Selected achievements" transition:fly={{ y: 14, duration: 160 }}>
          <span>{plural(selected.size, "achievement")} selected</span>
          <button class="btn small primary" type="button" disabled={!!busy || !unlockPicked.length} onclick={() => change(unlockPicked, true, "selected")}>
            {#if busy === "selected"}<LoaderCircle class="spin" size={13} />{:else}<LockOpen size={13} />{/if} Unlock
          </button>
          <button class="btn small" type="button" disabled={!!busy || !lockPicked.length} onclick={() => lock(lockPicked, "selected")}><Lock size={13} /> Lock</button>
          <button class="btn small ghost" type="button" disabled={!!busy} onclick={() => selected.clear()}>Clear</button>
        </div>
      {/if}

      <div class="list" class:with-bar={selected.size > 0} aria-busy={loading}>
        {#if loading && !data}
          <p class="state"><LoaderCircle class="spin" size={18} /> Reading the achievements from Steam…</p>
        {:else if error}
          <div class="state bad" role="alert">
            <CircleAlert size={18} />
            <span>{error}</span>
            <button class="btn small" type="button" onclick={load}><RefreshCw size={13} /> Try again</button>
          </div>
        {:else if !list.length}
          <p class="state">{game.name} has no achievements.</p>
        {:else}
          <div class="list-head">
            <label><input class="check" type="checkbox" checked={allShown} disabled={!shown.length} onchange={pickShown} />Select {shown.length === list.length ? "all" : "these"}</label>
            {#if data && !data.described}<span class="hint">Steam's descriptions could not be read, so only names show.</span>{/if}
          </div>
          {#if !shown.length}
            <p class="state">Nothing matches.</p>
          {:else}
            <ul>
              {#each shown as achievement (achievement.name)}
                {@const icon = iconOf(achievement)}
                <li class:unlocked={achievement.unlocked} class:chosen={selected.has(achievement.name)}>
                  <input class="check" type="checkbox" checked={selected.has(achievement.name)} aria-label={`Select ${achievement.title}`} onchange={() => pick(achievement.name)} />
                  <span class="icon">
                    {#if icon}<img src={icon} alt="" loading="lazy" decoding="async" />{:else}<Trophy size={18} />{/if}
                    {#if achievement.unlocked}<span class="tick" aria-hidden="true"><Check size={10} strokeWidth={3.2} /></span>{/if}
                  </span>
                  <div class="text">
                    <strong>
                      {achievement.title}
                      {#if achievement.hidden}<span class="tag" title="Steam keeps this one hidden from other players until it is unlocked"><EyeOff size={10} /> Hidden</span>{/if}
                    </strong>
                    {#if achievement.description}<p>{achievement.description}</p>{/if}
                  </div>
                  {#if achievement.percent !== null}
                    <span class="rate {tone(achievement.percent)}" title="{percentText(achievement.percent)} of players have it">{percentText(achievement.percent)} · {rarity(achievement.percent)}</span>
                  {:else}
                    <span></span>
                  {/if}
                  <button
                    class="row-action"
                    class:lock={achievement.unlocked}
                    type="button"
                    disabled={!!busy}
                    title={achievement.unlocked ? "Lock" : "Unlock"}
                    aria-label={achievement.unlocked ? `Lock ${achievement.title}` : `Unlock ${achievement.title}`}
                    onclick={() => (achievement.unlocked ? lock([achievement.name], achievement.name) : change([achievement.name], true, achievement.name))}
                  >
                    {#if busy === achievement.name}<LoaderCircle class="spin" size={15} />{:else if achievement.unlocked}<Lock size={15} />{:else}<LockOpen size={15} />{/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      </div>
    </section>
  </div>
</div>

<style>
  /* Under the confirm dialog (90) and the toasts (120), over the page. */
  .backdrop { position: fixed; inset: 0; z-index: 80; display: grid; place-items: center; padding: 24px; background: var(--scrim); }
  .dialog { display: grid; grid-template-columns: 252px minmax(0, 1fr); width: min(1060px, 100%); height: min(740px, calc(100vh - 48px)); overflow: hidden; border-radius: var(--radius-xl); outline: none; }

  .side { display: flex; flex-direction: column; align-items: center; gap: 12px; min-height: 0; padding: 24px 18px 18px; overflow-y: auto; border-right: 1px solid var(--panel-rim); background: rgb(0 0 0 / 0.12); text-align: center; }
  .cover { width: 132px; aspect-ratio: 2 / 3; flex: none; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--input-fill); box-shadow: var(--elev-2); }
  .cover img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .blank { display: grid; place-items: center; width: 100%; height: 100%; color: var(--text-3); }
  h2 { font-size: 15px; font-weight: 700; line-height: 1.25; overflow-wrap: anywhere; }
  .ring { position: relative; display: grid; place-items: center; width: 92px; height: 92px; flex: none; }
  .ring svg { position: absolute; inset: 0; width: 100%; height: 100%; transform: rotate(-90deg); }
  .ring circle { fill: none; stroke-width: 7; }
  .ring .track { stroke: rgb(255 255 255 / 0.08); }
  .ring .fill { stroke: var(--accent); stroke-linecap: round; transition: stroke-dashoffset var(--dur-slow) var(--ease-out); }
  .ring.full .fill { stroke: var(--ok); }
  .ring strong { font-size: 19px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .count { margin-top: -4px; color: var(--text-2); font-size: 12.5px; font-variant-numeric: tabular-nums; }
  .side-actions { display: grid; gap: 8px; width: 100%; margin-top: 4px; }
  .side-actions .btn { justify-content: center; width: 100%; }
  .auto { display: flex; align-items: center; gap: 10px; width: 100%; margin-top: 8px; padding-top: 14px; border-top: 1px solid var(--panel-rim); color: var(--text-1); font-size: 12.5px; text-align: left; cursor: pointer; }
  .note { width: 100%; color: var(--text-3); font-size: 11.5px; line-height: 1.45; text-align: left; }
  .note.quiet { margin-top: auto; padding-top: 10px; }

  .main { position: relative; display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 16px 16px 10px 20px; }
  h3 { font-size: 16px; font-weight: 700; }
  .close { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: 9px; background: transparent; color: var(--text-2); cursor: pointer; }
  .close:hover:not(:disabled) { background: var(--hover); color: var(--text-1); }
  .tools { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 0 20px 12px; border-bottom: 1px solid var(--panel-rim); }
  .search { position: relative; display: flex; align-items: center; flex: 1 1 200px; min-width: 160px; color: var(--text-3); }
  .search :global(svg) { position: absolute; left: 11px; pointer-events: none; }
  .search .input { width: 100%; padding-left: 32px; }
  .filters { display: flex; gap: 6px; }
  .sort { margin-left: auto; }
  /* Over the list's bottom, out of the flow: picking a row never moves the rows under the pointer. */
  .picked { position: absolute; right: 20px; bottom: 16px; left: 20px; z-index: 2; display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 8px 10px 8px 14px; border: 1px solid rgb(var(--accent-rgb) / 0.4); border-radius: var(--radius-md); background: var(--float-bg); box-shadow: var(--elev-3); color: var(--text-1); font-size: 12.5px; }
  .picked span { margin-right: auto; font-weight: 600; }

  .list { flex: 1; min-height: 0; padding: 6px 12px 16px; overflow-y: auto; }
  .list.with-bar { padding-bottom: 76px; }
  .list-head { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 16px; padding: 8px 8px 6px; color: var(--text-2); font-size: 12px; }
  .list-head label { display: flex; align-items: center; gap: 10px; cursor: pointer; }
  .hint { color: var(--text-3); }
  .state { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 10px; min-height: 180px; padding: 24px; color: var(--text-2); font-size: 13px; text-align: center; }
  .state.bad { color: #ffb4b0; }
  ul { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
  li { display: grid; grid-template-columns: auto auto minmax(0, 1fr) auto auto; align-items: center; gap: 12px; padding: 9px 10px 9px 8px; border: 1px solid transparent; border-radius: var(--radius-md); background: rgb(255 255 255 / 0.025); content-visibility: auto; contain-intrinsic-size: auto 62px; transition: background var(--dur-fast), border-color var(--dur-fast); }
  li:hover { background: var(--hover); }
  li.chosen { border-color: rgb(var(--accent-rgb) / 0.4); background: var(--selected); }
  .icon { position: relative; display: grid; place-items: center; width: 44px; height: 44px; border-radius: 9px; background: var(--input-fill); color: var(--text-3); }
  .icon img { display: block; width: 44px; height: 44px; border-radius: 9px; }
  /* Some games' "locked" icons are in colour too. */
  li:not(.unlocked) .icon img { filter: grayscale(1); opacity: 0.75; }
  .tick { position: absolute; right: -4px; bottom: -4px; display: grid; place-items: center; width: 17px; height: 17px; border-radius: 50%; background: var(--ok); color: #fff; box-shadow: 0 0 0 2px var(--bg-1); }
  .text { min-width: 0; }
  .text strong { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 7px; font-size: 13.5px; font-weight: 650; line-height: 1.3; }
  li:not(.unlocked) .text strong { color: var(--text-2); }
  .text p { margin-top: 2px; color: var(--text-3); font-size: 12px; line-height: 1.4; overflow-wrap: anywhere; }
  .tag { display: inline-flex; align-items: center; gap: 3px; padding: 0 6px; border-radius: 999px; background: rgb(255 255 255 / 0.06); color: var(--text-3); font-size: 10.5px; font-weight: 600; }
  .rate { padding: 2px 9px; border-radius: 999px; background: rgb(255 255 255 / 0.05); color: var(--text-3); font-size: 11.5px; font-weight: 600; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .rate.uncommon { background: rgb(var(--accent-rgb) / 0.12); color: rgb(var(--accent-soft-rgb)); }
  .rate.rare { background: color-mix(in srgb, var(--warn) 14%, transparent); color: var(--warn); }
  .rate.very-rare { background: color-mix(in srgb, var(--danger) 16%, transparent); color: color-mix(in srgb, var(--warn) 45%, var(--danger)); }
  .row-action { display: grid; place-items: center; width: 34px; height: 34px; padding: 0; border: 1px solid var(--btn-border); border-radius: 10px; background: var(--control-face), var(--btn-primary); color: var(--btn-primary-text); box-shadow: var(--btn-sheen); cursor: pointer; }
  .row-action.lock { background: var(--control-face), var(--btn-fill); color: var(--text-2); }
  .row-action:hover:not(:disabled) { filter: brightness(1.12); }
  .row-action:disabled { opacity: 0.5; cursor: default; }

  @media (max-width: 820px) {
    .dialog { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
    .side { flex-direction: row; flex-wrap: wrap; justify-content: center; padding: 14px; border-right: 0; border-bottom: 1px solid var(--panel-rim); }
    .cover { width: 64px; }
    .note.quiet { display: none; }
  }
  @media (prefers-reduced-motion: reduce) { .ring .fill, li { transition: none; } }
</style>
