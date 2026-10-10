<script lang="ts">
  /**
   * Colored window controls with quiet depth, shared by both title bars.
   */
  type Kind = "minimize" | "maximize" | "close";

  let {
    kind,
    label,
    disabled = false,
    title,
    onclick,
  }: {
    kind: Kind;
    label: string;
    disabled?: boolean;
    title?: string;
    onclick: () => void;
  } = $props();
</script>

<button class="wb {kind}" aria-label={label} {title} {disabled} {onclick}>
  <span class="orb" aria-hidden="true">
    <svg viewBox="0 0 20 20">
      {#if kind === "close"}
        <path class="stroke" d="M7 7l6 6m0-6l-6 6" />
      {:else if kind === "minimize"}
        <path class="stroke" d="M6.4 10h7.2" />
      {:else}
        <path class="fill" d="M6.9 6.9h6.2v6.2z" />
        <path class="fill" d="M6.9 8l5.1 5.1h-5.1z" />
      {/if}
    </svg>
  </span>
</button>

<style>
  .wb {
    --body: #f5433f;
    --ink: #6e0d10;
    display: grid;
    place-items: center;
    width: 27px;
    height: 100%;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: none;
  }

  .minimize {
    --body: #fbb325;
    --ink: #613c00;
  }

  .maximize {
    --body: #2fc147;
    --ink: #074216;
  }

  .orb {
    position: relative;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--body);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.35),
      inset 0 -1px 0 rgb(0 0 0 / 0.2),
      0 1px 3px rgb(0 0 0 / 0.25);
    transition:
      transform var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast),
      filter var(--dur-fast);
  }

  svg {
    width: 16px;
    height: 16px;
  }

  .stroke {
    fill: none;
    stroke: var(--ink);
    stroke-width: 1.85;
    stroke-linecap: round;
  }

  .fill {
    fill: var(--ink);
  }

  .wb:hover .orb {
    filter: brightness(1.12);
    box-shadow: 0 0 0 2px rgb(255 255 255 / 0.12);
  }

  /* A subtle pressed state without glossy highlights. */
  .wb:active .orb {
    transform: translateY(1px) scale(0.86);
    filter: brightness(0.88);
    box-shadow: none;
    transition-duration: 50ms;
  }

  .wb:focus-visible {
    outline: none;
  }

  .wb:focus-visible .orb {
    outline: 2px solid rgb(255 255 255 / 0.75);
    outline-offset: 2px;
  }

  .wb:disabled {
    cursor: not-allowed;
  }

  .wb:disabled .orb {
    opacity: 0.35;
    transform: none;
    filter: grayscale(0.6);
  }
</style>
