<script lang="ts">
  // Compact, responsive Steam sign-in hero view that fits 720p, 1080p and 1440p with zero scrollbar,
  // matching MYLE's exact surface and control design tokens.
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Award from "@lucide/svelte/icons/award";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import Layers from "@lucide/svelte/icons/layers";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Package from "@lucide/svelte/icons/package";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { steamAutomationState as page } from "./state.svelte";

  const status = $derived(page.status);
  const ready = $derived(status?.ready ?? false);
  const running = $derived(ready && (status?.steamRunning ?? false));
  const expired = $derived(status?.communitySignedIn ?? false);

  const TAB_DESCRIPTIONS: Record<string, string> = {
    games: "Sign in with your Steam account to browse and search your library, track playtime, and idle games.",
    cards: "Sign in with your Steam account to automatically idle games and collect all your trading card drops.",
    achievements: "Sign in with your Steam account to inspect and unlock achievements across your games.",
    inventory: "Sign in with your Steam account to browse your inventory and list duplicate cards on the Market.",
  };

  const currentDesc = $derived(TAB_DESCRIPTIONS[page.view] ?? TAB_DESCRIPTIONS.games);

  const FEATURES = [
    {
      icon: Layers,
      title: "Automated Card Farming",
      detail: "Idles games in the background to collect trading card drops.",
      color: "var(--accent)",
    },
    {
      icon: Award,
      title: "Achievement Manager",
      detail: "Inspect library achievements and safely unlock milestones.",
      color: "#f59e0b",
    },
    {
      icon: Package,
      title: "Smart Market & Inventory",
      detail: "Prices and lists duplicate cards while keeping one of each.",
      color: "var(--ok)",
    },
    {
      icon: Gamepad2,
      title: "Library & Idle Booster",
      detail: "Track playtime and add hours safely with refund protection.",
      color: "#a78bfa",
    },
  ];
</script>

<div class="steam-connect">
  <div class="hero-card surface">
    <!-- Steam Logo with subtle ambient glow -->
    <div class="logo-wrap">
      <div class="logo-glow"></div>
      <img src="/icons/installapps/Steam.svg" alt="Steam" class="steam-logo" width="52" height="52" />
    </div>

    <!-- Status badge -->
    <div class="status-badge" class:online={running}>
      <span class="dot" class:on={running}></span>
      <span>{ready ? (running ? "Steam app is open on this PC" : "Steam app is closed") : "Checking Steam…"}</span>
    </div>

    <!-- Title & dynamic subtitle -->
    <h2>{expired ? "Steam Session Expired" : "Connect Steam Automation"}</h2>
    <p class="subtitle">
      {#if expired}
        Your Steam Community session has ended. Sign in again to refresh your library, card drops, and market listings.
      {:else}
        {currentDesc}
      {/if}
    </p>

    <!-- Main Call to Action -->
    <div class="action-wrap">
      <button
        class="btn primary connect-btn"
        type="button"
        disabled={page.signingIn}
        onclick={() => void page.signIn()}
      >
        {#if page.signingIn}
          <LoaderCircle class="spinner" size={16} />
          <span>Finish in the Steam window…</span>
        {:else if expired}
          <Sparkles size={15} />
          <span>Sign in again with Steam</span>
          <ArrowRight size={15} />
        {:else}
          <Sparkles size={15} />
          <span>Sign in with Steam</span>
          <ArrowRight size={15} />
        {/if}
      </button>

      {#if page.signingIn}
        <div class="signing-banner" role="status">
          <LoaderCircle class="spinner" size={15} />
          <div class="signing-text">
            <strong>A secure Steam login window is open</strong>
            <p>Log in with your Steam account and Steam Guard there. MYLE will detect your session automatically.</p>
          </div>
        </div>
      {/if}

      {#if page.actionError}
        <div class="error-banner" role="alert">
          <CircleAlert size={15} />
          <span>{page.actionError}</span>
        </div>
      {/if}
    </div>

    <!-- Features Grid (Compact 2x2) -->
    <div class="features-grid">
      {#each FEATURES as feat (feat.title)}
        <div class="feature-card">
          <div class="feat-icon" style="color: {feat.color}; background: color-mix(in srgb, {feat.color} 14%, transparent);">
            <feat.icon size={17} />
          </div>
          <div class="feat-content">
            <h4>{feat.title}</h4>
            <p>{feat.detail}</p>
          </div>
        </div>
      {/each}
    </div>

    <!-- Security & Privacy Guarantee (Compact single row) -->
    <div class="security-card">
      <ShieldCheck size={18} class="shield-icon" />
      <div class="security-text">
        <strong>Direct & Safe Steam Authentication:</strong>
        <span>
          Sign-in occurs on Valve's official page (<code>steamcommunity.com</code>). MYLE never sees or stores your password or Steam Guard codes.
        </span>
      </div>
    </div>
  </div>
</div>

<style>
  .steam-connect {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: 100%;
    min-height: 100%;
    padding: clamp(10px, 2vh, 24px) 16px;
  }

  .hero-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    width: 100%;
    max-width: min(740px, 94vw);
    padding: clamp(20px, 3vh, 32px) clamp(20px, 3vw, 36px);
    border-radius: var(--radius-xl);
  }

  /* Logo */
  .logo-wrap {
    position: relative;
    display: grid;
    place-items: center;
    margin-bottom: clamp(10px, 1.4vh, 16px);
  }

  .logo-glow {
    position: absolute;
    inset: -12px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(107, 133, 255, 0.28) 0%, rgba(30, 40, 70, 0.08) 60%, transparent 70%);
    filter: blur(12px);
    pointer-events: none;
  }

  .steam-logo {
    position: relative;
    width: clamp(50px, 6vh, 64px);
    height: clamp(50px, 6vh, 64px);
    border-radius: 16px;
    filter: drop-shadow(0 8px 18px rgba(0, 0, 0, 0.45));
  }

  /* Status badge */
  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 4px 12px;
    margin-bottom: clamp(10px, 1.4vh, 14px);
    border-radius: 999px;
    background: var(--control-face), var(--hover);
    border: 1px solid var(--panel-rim);
    color: var(--text-3);
    font-size: clamp(11.5px, 0.9vw, 12.5px);
    font-weight: 500;
  }

  .status-badge.online {
    background: color-mix(in srgb, var(--ok) 8%, var(--hover));
    border-color: color-mix(in srgb, var(--ok) 24%, var(--panel-rim));
    color: var(--text-2);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--idle);
  }

  .dot.on {
    background: var(--ok);
    box-shadow: 0 0 6px var(--ok-glow, rgba(16, 185, 129, 0.6));
  }

  /* Typography */
  h2 {
    margin: 0 0 6px;
    font-size: clamp(18px, 2.4vh, 23px);
    font-weight: 700;
    color: var(--text-1);
    letter-spacing: -0.015em;
    line-height: 1.25;
  }

  .subtitle {
    margin: 0 0 clamp(14px, 2vh, 20px);
    max-width: 540px;
    font-size: clamp(12.5px, 1.5vh, 13.8px);
    line-height: 1.45;
    color: var(--text-2);
  }

  /* Action button */
  .action-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    gap: 10px;
    margin-bottom: clamp(14px, 2.2vh, 22px);
  }

  .connect-btn {
    height: clamp(38px, 4.6vh, 44px);
    padding: 0 clamp(22px, 2.8vw, 30px);
    border-radius: var(--radius-md);
    font-size: clamp(13.2px, 1.6vh, 14.5px);
    font-weight: 600;
  }

  :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Signing state banner */
  .signing-banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    max-width: 460px;
    padding: 8px 12px;
    text-align: left;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-fill));
    border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--glass-border));
    color: var(--text-2);
  }

  .signing-text strong {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-1);
    margin-bottom: 1px;
  }

  .signing-text p {
    margin: 0;
    font-size: 11.2px;
    line-height: 1.35;
    color: var(--text-3);
  }

  /* Error banner */
  .error-banner {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--danger) 14%, var(--surface-fill));
    border: 1px solid color-mix(in srgb, var(--danger) 34%, var(--glass-border));
    color: #fca5a5;
    font-size: 12px;
  }

  /* Features Grid */
  .features-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: clamp(8px, 1.2vh, 12px);
    width: 100%;
    margin-bottom: clamp(12px, 1.8vh, 18px);
  }

  @media (max-width: 520px) {
    .features-grid {
      grid-template-columns: 1fr;
    }
  }

  .feature-card {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: clamp(8px, 1.2vh, 12px) 13px;
    border-radius: var(--radius-sm);
    background: var(--control-face), var(--hover);
    border: 1px solid var(--panel-rim);
    text-align: left;
    transition: background var(--dur-fast);
  }

  .feat-icon {
    display: grid;
    place-items: center;
    width: clamp(32px, 3.8vw, 36px);
    height: clamp(32px, 3.8vw, 36px);
    flex: none;
    border-radius: 8px;
  }

  .feat-content h4 {
    margin: 0 0 2px;
    font-size: clamp(12.6px, 1.4vh, 13.6px);
    font-weight: 600;
    color: var(--text-1);
  }

  .feat-content p {
    margin: 0;
    font-size: clamp(11.4px, 1.25vh, 12.2px);
    line-height: 1.35;
    color: var(--text-3);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  /* Security banner */
  .security-card {
    display: flex;
    align-items: center;
    gap: 11px;
    width: 100%;
    padding: clamp(8px, 1.1vh, 11px) 14px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--ok) 7%, var(--hover));
    border: 1px solid color-mix(in srgb, var(--ok) 22%, var(--panel-rim));
    text-align: left;
  }

  :global(.shield-icon) {
    color: var(--ok);
    flex-shrink: 0;
  }

  .security-text {
    font-size: clamp(11.4px, 1.25vh, 12.2px);
    line-height: 1.38;
    color: var(--text-3);
  }

  .security-text strong {
    color: var(--text-2);
    font-weight: 600;
    margin-right: 4px;
  }

  .security-text code {
    padding: 1px 4px;
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.3);
    font-family: var(--font-mono);
    font-size: clamp(10.5px, 1.15vh, 11.2px);
    color: var(--text-2);
  }
</style>
