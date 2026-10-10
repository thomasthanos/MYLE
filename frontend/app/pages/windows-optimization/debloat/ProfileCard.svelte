<script lang="ts">
  // One Quick setup profile: what it is for, and what it would do on this PC.
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Feather from "@lucide/svelte/icons/feather";
  import Rocket from "@lucide/svelte/icons/rocket";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import type { Profile } from "./selection";

  let { profile, name, tagline, text, settings, apps, selected = false, best = false, disabled = false, onchoose }: {
    profile: Profile;
    name: string;
    tagline: string;
    text: string;
    /** Settings it would turn on here, and apps it would remove. */
    settings: number;
    apps: number;
    selected?: boolean;
    /** The one to pick when unsure. */
    best?: boolean;
    disabled?: boolean;
    onchoose: () => void;
  } = $props();

  const icons = { light: Feather, recommended: ShieldCheck, maximum: Rocket } as const;
  /** How safe it is, in a word and a line. */
  const safety = {
    light: { tone: "safe", word: "Safe", line: "Nothing you use stops working" },
    recommended: { tone: "safe", word: "Safe", line: "Fine for almost every PC" },
    maximum: { tone: "advanced", word: "Advanced", line: "Some Windows features turn off" },
  } as const;
  const safe = $derived(safety[profile]);
  const Icon = $derived(icons[profile]);
  const done = $derived(settings === 0 && apps === 0);
</script>

<button type="button" class="profile surface" class:selected class:best aria-pressed={selected} {disabled} onclick={onchoose}>
  <span class="head">
    <span class="icon"><Icon size={17} /></span>
    <span class="names">
      <strong>{name}{#if best}<em>Best for most people</em>{/if}</strong>
      <span>{tagline}</span>
    </span>
    <span class="mark" aria-hidden="true">{#if selected}<CircleCheck size={18} />{/if}</span>
  </span>
  <span class="text">{text}</span>
  <span class="safety {safe.tone}"><i aria-hidden="true"></i><b>{safe.word}</b> · {safe.line}</span>
  <span class="counts">
    {#if done}
      <CircleCheck size={13} /> Already done on this PC
    {:else}
      <b>{settings}</b> setting{settings === 1 ? "" : "s"} · <b>{apps}</b> app{apps === 1 ? "" : "s"} to remove
    {/if}
  </span>
</button>

<style>
  .profile { display: flex; flex-direction: column; gap: 8px; min-width: 0; padding: 12px 13px; border-radius: 12px; text-align: left; transition: border-color var(--dur-fast), background var(--dur-fast), transform var(--dur-fast) var(--ease-out); }
  .profile:hover:not(:disabled) { border-color: rgb(var(--accent-rgb) / 0.3); transform: translateY(-1px); }
  .profile:focus-visible { outline: 2px solid rgb(var(--accent-rgb) / 0.8); outline-offset: 2px; }
  .profile.selected { border-color: rgb(var(--accent-rgb) / 0.55); background: var(--selected); }
  .profile:disabled { opacity: 0.55; cursor: default; }
  .head { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .icon { display: grid; place-items: center; flex: none; width: 32px; height: 32px; border: 1px solid rgb(var(--accent-rgb) / 0.22); border-radius: 9px; background: rgb(var(--accent-rgb) / 0.1); color: var(--accent); }
  .best .icon { background: var(--accent-grad); color: #fff; border-color: transparent; }
  .names { display: grid; flex: 1; gap: 1px; min-width: 0; }
  strong { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; color: var(--text-1); font-size: 14px; font-weight: 650; }
  em { padding: 1px 6px; border-radius: 5px; background: rgb(var(--accent-rgb) / 0.16); color: rgb(var(--accent-soft-rgb)); font-size: 9.5px; font-style: normal; font-weight: 600; letter-spacing: 0.02em; }
  .names > span { color: var(--text-2); font-size: 11.5px; }
  .mark { display: grid; place-items: center; flex: none; width: 20px; color: var(--accent); }
  .text { color: var(--text-2); font-size: 11.5px; line-height: 1.5; }
  .safety { display: inline-flex; align-items: center; gap: 6px; color: var(--text-2); font-size: 11px; }
  .safety i { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--ok); box-shadow: 0 0 6px var(--ok-glow); }
  .safety b { font-weight: 650; color: rgb(110 225 175); }
  .safety.advanced i { background: #e8ad55; box-shadow: 0 0 6px rgb(232 173 85 / 0.45); }
  .safety.advanced b { color: #efc38a; }
  .counts { display: flex; align-items: center; gap: 5px; margin-top: auto; padding-top: 8px; border-top: 1px solid rgb(255 255 255 / 0.06); color: var(--text-2); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .counts b { color: var(--text-1); font-weight: 650; }
  .counts :global(svg) { color: var(--ok); }
  @media (prefers-reduced-motion: reduce) { .profile { transition: none; } .profile:hover:not(:disabled) { transform: none; } }
</style>
