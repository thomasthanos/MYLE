<script lang="ts">
  import { onMount } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { readJson, writeJson } from "../../../lib/storage";
  import { passwordsApi as api, type GeneratorOptions, type Strength } from "./api";
  import { passwords as p } from "./state.svelte";
  import StrengthMeter from "./StrengthMeter.svelte";

  /** `onuse` puts the password in the entry being edited. */
  let { onuse }: { onuse?: (password: string) => void } = $props();

  const KEY = "myle.passwords.generator";
  const DEFAULTS: Required<GeneratorOptions> = {
    kind: "password",
    length: 20,
    lower: true,
    upper: true,
    digits: true,
    symbols: true,
    avoidAmbiguous: false,
    words: 5,
    separator: "-",
    capitalize: true,
    number: true,
  };
  // Saved by an older version: whatever it lacks takes the default.
  let options = $state<Required<GeneratorOptions>>({
    ...DEFAULTS,
    ...readJson<Partial<GeneratorOptions>>(KEY, {}, (v) => !!v && typeof v === "object"),
  });
  let value = $state("");
  let strength = $state<Strength>("none");
  /** Only the latest request's answer is shown (a slider asks many times). */
  let asked = 0;

  async function next() {
    const ticket = ++asked;
    try {
      const made = await api.generate(options);
      const rated = await api.strength(made);
      if (ticket !== asked) return;
      value = made;
      strength = rated;
      writeJson(KEY, options);
    } catch (error) {
      if (ticket !== asked) return;
      value = "";
      strength = "none";
      p.error = error instanceof Error ? error.message : String(error);
    }
  }

  onMount(() => void next());

  const kinds = [
    ["lower", "a-z"],
    ["upper", "A-Z"],
    ["digits", "0-9"],
    ["symbols", "!@#"],
  ] as const;
  const separators = [
    ["-", "-"],
    [".", "."],
    ["_", "_"],
    ["+", "+"],
    ["", "None"],
  ] as const;
  /** Words in the app's list (the EFF's large wordlist). */
  const WORDS = 7775;

  const onlyKind = $derived(kinds.filter(([key]) => options[key]).length === 1);

  /** About how many guesses it takes, in bits, from how it is made. */
  const bits = $derived.by(() => {
    if (options.kind === "passphrase") {
      const count = Math.min(12, Math.max(3, options.words));
      return count * Math.log2(WORDS) + (options.number ? Math.log2(10 * count) : 0);
    }
    const avoid = options.avoidAmbiguous;
    const pool =
      (options.lower ? (avoid ? 24 : 26) : 0) +
      (options.upper ? (avoid ? 24 : 26) : 0) +
      (options.digits ? (avoid ? 8 : 10) : 0) +
      (options.symbols ? (avoid ? 21 : 25) : 0);
    return pool > 1 ? options.length * Math.log2(pool) : 0;
  });

  function set<K extends keyof GeneratorOptions>(key: K, next_: Required<GeneratorOptions>[K]) {
    options[key] = next_;
    void next();
  }

  function toggleKind(key: (typeof kinds)[number][0]) {
    // At least one kind of character stays chosen.
    if (options[key] && onlyKind) return;
    set(key, !options[key]);
  }

  const copied = $derived(p.copied === "text");
</script>

<div class="generator">
  <div class="mode" role="radiogroup" aria-label="What to make">
    <button
      type="button"
      role="radio"
      aria-checked={options.kind === "password"}
      class:active={options.kind === "password"}
      onclick={() => set("kind", "password")}>Password</button
    >
    <button
      type="button"
      role="radio"
      aria-checked={options.kind === "passphrase"}
      class:active={options.kind === "passphrase"}
      title="Random words: easier to read out and type"
      onclick={() => set("kind", "passphrase")}>Passphrase</button
    >
  </div>

  <div class="result">
    <span class="value selectable" data-sensitive>{value || "—"}</span>
    <button type="button" class="icon-btn" title="New one" aria-label="Make a new one" onclick={next}><RefreshCw size={15} /></button>
  </div>
  <div class="rating">
    <StrengthMeter {strength} />
    {#if bits}<span class="bits" title="About how many guesses it would take, as a power of 2">≈ {Math.round(bits)} bits</span>{/if}
  </div>

  {#if options.kind === "password"}
    <label class="length">
      <span>Length <strong>{options.length}</strong></span>
      <input type="range" min="8" max="64" bind:value={options.length} oninput={next} />
    </label>

    <div class="kinds">
      {#each kinds as [key, label] (key)}
        <button
          type="button"
          class="chip"
          class:active={options[key]}
          aria-pressed={options[key]}
          disabled={options[key] && onlyKind}
          title={options[key] && onlyKind ? "At least one kind of character stays on" : undefined}
          onclick={() => toggleKind(key)}>{label}</button
        >
      {/each}
      <button
        type="button"
        class="chip"
        class:active={options.avoidAmbiguous}
        aria-pressed={options.avoidAmbiguous}
        title="Leave out l, 1, I, O, 0 and similar"
        onclick={() => set("avoidAmbiguous", !options.avoidAmbiguous)}>No look-alikes</button
      >
    </div>
  {:else}
    <label class="length">
      <span>Words <strong>{options.words}</strong></span>
      <input type="range" min="3" max="12" bind:value={options.words} oninput={next} />
    </label>

    <div class="kinds" role="group" aria-label="Between the words">
      <span class="kinds-label">Between</span>
      {#each separators as [sep, label] (label)}
        <button
          type="button"
          class="chip"
          class:active={options.separator === sep}
          aria-pressed={options.separator === sep}
          onclick={() => set("separator", sep)}>{label}</button
        >
      {/each}
    </div>
    <div class="kinds">
      <button
        type="button"
        class="chip"
        class:active={options.capitalize}
        aria-pressed={options.capitalize}
        onclick={() => set("capitalize", !options.capitalize)}>Capitals</button
      >
      <button
        type="button"
        class="chip"
        class:active={options.number}
        aria-pressed={options.number}
        title="One word gets a digit after it"
        onclick={() => set("number", !options.number)}>A number</button
      >
    </div>
  {/if}

  <div class="actions">
    <button type="button" class="btn" disabled={!value} onclick={() => p.copyText(value)}>
      {#if copied}<Check size={14} /> Copied{:else}<Copy size={14} /> Copy{/if}
    </button>
    {#if onuse}
      <button type="button" class="btn primary" disabled={!value} onclick={() => onuse?.(value)}>Use this {options.kind}</button>
    {/if}
  </div>
</div>

<style>
  .generator { display: grid; gap: 12px; width: 336px; padding: 14px; }

  .mode { display: grid; grid-template-columns: 1fr 1fr; gap: 3px; padding: 3px; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 10px; background: rgb(0 0 0 / 0.22); }

  .mode button { height: 28px; border-radius: 7px; color: var(--text-2); font-size: 12px; font-weight: 600; transition: background var(--dur-fast), color var(--dur-fast); }

  .mode button:hover { color: var(--text-1); }

  .mode button.active { background: rgb(var(--accent-rgb) / 0.2); color: var(--text-1); box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.35); }

  .result { display: flex; align-items: center; gap: 6px; min-height: 44px; padding: 8px 6px 8px 12px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 10px; background: rgb(0 0 0 / 0.25); }

  .value { flex: 1; min-width: 0; overflow-wrap: anywhere; font-family: var(--font-mono); font-size: 13.5px; line-height: 1.4; }

  .rating { display: flex; align-items: center; justify-content: space-between; gap: 10px; }

  .bits { color: var(--text-3); font-size: 11px; font-variant-numeric: tabular-nums; }

  .length { display: grid; gap: 6px; color: var(--text-2); font-size: 12px; }

  .length strong { color: var(--text-1); }

  input[type="range"] { width: 100%; accent-color: var(--accent); }

  .kinds { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }

  .kinds-label { margin-right: 2px; color: var(--text-3); font-size: 11.5px; }

  .kinds .chip { height: 26px; padding: 0 10px; font-size: 11.5px; }

  .kinds .chip:disabled { cursor: not-allowed; }

  .actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
