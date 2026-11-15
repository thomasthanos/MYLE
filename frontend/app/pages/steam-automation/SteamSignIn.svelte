<script lang="ts">
  // Modern, user-friendly Steam sign-in hero view shown when Steam is not connected.
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

  const FEATURES = [
    {
      icon: Layers,
      title: "Automated Card Farming",
      detail: "Idles games with remaining trading card drops in the background without installing or playing them.",
      color: "#60a5fa",
    },
    {
      icon: Award,
      title: "Achievement Manager",
      detail: "Inspect achievement progress for your library and safely unlock achievements at natural intervals.",
      color: "#f59e0b",
    },
    {
      icon: Package,
      title: "Smart Market & Inventory",
      detail: "Automatically prices and lists duplicate cards on the Steam Market while always keeping one copy.",
      color: "#34d399",
    },
    {
      icon: Gamepad2,
      title: "Library & Idle Booster",
      detail: "Track full library stats, manage favorites, and add playtime safely with refund-window protection.",
      color: "#a78bfa",
    },
  ];
  const TAB_DESCRIPTIONS: Record<string, string> = {
    games: "Sign in with your Steam account to browse and search your library, track playtime, and idle games.",
    cards: "Sign in with your Steam account to automatically idle games and collect all your trading card drops.",
    achievements: "Sign in with your Steam account to inspect and unlock achievements across your games.",
    inventory: "Sign in with your Steam account to browse your inventory and list duplicate cards on the Market.",
  };

  const currentDesc = $derived(TAB_DESCRIPTIONS[page.view] ?? TAB_DESCRIPTIONS.games);
</script>

<div class="steam-connect">
  <!-- Glowing Hero Card -->
  <div class="hero-card">
    <div class="logo-wrap">
      <div class="logo-glow"></div>
      <img src="/icons/installapps/Steam.svg" alt="Steam" class="steam-logo" width="72" height="72" />
    </div>

    <div class="status-badge" class:online={running}>
      <span class="dot" class:on={running}></span>
      <span>{ready ? (running ? "Steam app is open on this PC" : "Steam app is closed") : "Checking Steam…"}</span>
    </div>

    <h2>{expired ? "Steam Session Expired" : "Connect Steam Automation"}</h2>

    <p class="subtitle">
      {#if expired}
        Your Steam Community session has expired or ended. Sign in again to refresh your library, card drops, and market listings.
      {:else}
        {currentDesc}
      {/if}
    </p>

    <!-- Main Call to Action -->
    <div class="action-wrap">
      <button
        class="connect-btn"
        type="button"
        disabled={page.signingIn}
        onclick={() => void page.signIn()}
      >
        {#if page.signingIn}
          <LoaderCircle class="spinner" size={18} />
          <span>Finish in the Steam window…</span>
        {:else if expired}
          <Sparkles size={18} />
          <span>Sign in again with Steam</span>
          <ArrowRight size={17} />
        {:else}
          <Sparkles size={18} />
          <span>Sign in with Steam</span>
          <ArrowRight size={17} />
        {/if}
      </button>

      {#if page.signingIn}
        <div class="signing-banner" role="status">
          <LoaderCircle class="spinner" size={17} />
          <div class="signing-text">
            <strong>A secure Steam login window is open</strong>
            <p>Enter your Steam credentials and Steam Guard code there. MYLE will automatically detect your login once complete.</p>
          </div>
        </div>
      {/if}

      {#if page.actionError}
        <div class="error-banner" role="alert">
          <CircleAlert size={17} />
          <span>{page.actionError}</span>
        </div>
      {/if}
    </div>

    <!-- Feature Highlights Grid -->
    <div class="features-grid">
      {#each FEATURES as feat (feat.title)}
        <div class="feature-card">
          <div class="feat-icon" style="color: {feat.color}; background: color-mix(in srgb, {feat.color} 12%, transparent);">
            <feat.icon size={20} />
          </div>
          <div class="feat-content">
            <h4>{feat.title}</h4>
            <p>{feat.detail}</p>
          </div>
        </div>
      {/each}
    </div>

    <!-- Security & Privacy Guarantee -->
    <div class="security-card">
      <ShieldCheck size={22} class="shield-icon" />
      <div class="security-text">
        <strong>Direct & Safe Steam Authentication</strong>
        <p>
          Sign-in takes place directly on Valve's official authentication page (<code>steamcommunity.com</code>) inside an isolated window.
          MYLE never sees or stores your password or Steam Guard codes. Only the encrypted session cookie is kept on this PC.
        </p>
      </div>
    </div>
  </div>
</div>

<style>
  .steam-connect {
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 12px 6px 36px;
    width: 100%;
  }

  .hero-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    width: 100%;
    max-width: 680px;
    padding: 36px 32px;
    border-radius: var(--radius-xl, 20px);
    background: color-mix(in srgb, var(--panel-solid, #141724) 86%, transparent);
    border: 1px solid var(--panel-rim, rgba(255, 255, 255, 0.08));
    box-shadow: 0 24px 64px -12px rgba(0, 0, 0, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(20px);
  }

  /* Logo */
  .logo-wrap {
    position: relative;
    display: grid;
    place-items: center;
    margin-bottom: 16px;
  }

  .logo-glow {
    position: absolute;
    inset: -14px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(102, 192, 244, 0.35) 0%, rgba(27, 72, 107, 0.15) 55%, transparent 70%);
    filter: blur(14px);
    pointer-events: none;
  }

  .steam-logo {
    position: relative;
    width: 72px;
    height: 72px;
    border-radius: 18px;
    filter: drop-shadow(0 8px 18px rgba(0, 0, 0, 0.4));
  }

  /* Status badge */
  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 4px 12px;
    margin-bottom: 14px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: var(--text-3);
    font-size: 12px;
    font-weight: 500;
  }

  .status-badge.online {
    background: rgba(62, 207, 142, 0.08);
    border-color: rgba(62, 207, 142, 0.22);
    color: #a7f3d0;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--idle, #64748b);
  }

  .dot.on {
    background: #34d399;
    box-shadow: 0 0 8px rgba(52, 211, 153, 0.7);
  }

  /* Typography */
  h2 {
    margin: 0 0 10px;
    font-size: 23px;
    font-weight: 700;
    color: var(--text-1);
    letter-spacing: -0.02em;
  }

  .subtitle {
    margin: 0 0 24px;
    max-width: 540px;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--text-3);
  }

  /* Action button */
  .action-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    gap: 14px;
    margin-bottom: 28px;
  }

  .connect-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 44px;
    padding: 0 26px;
    border-radius: 12px;
    font-size: 14.5px;
    font-weight: 700;
    color: #ffffff;
    background: linear-gradient(135deg, #2563eb 0%, #1d4ed8 50%, #1e40af 100%);
    border: 1px solid rgba(255, 255, 255, 0.2);
    box-shadow: 0 6px 20px -3px rgba(37, 99, 235, 0.5), inset 0 1px 0 rgba(255, 255, 255, 0.3);
    cursor: pointer;
    transition: all 160ms var(--ease-out, ease-out);
  }

  .connect-btn:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 8px 26px -3px rgba(37, 99, 235, 0.65), inset 0 1px 0 rgba(255, 255, 255, 0.4);
    filter: brightness(1.08);
  }

  .connect-btn:active:not(:disabled) {
    transform: translateY(1px);
    box-shadow: 0 3px 12px -2px rgba(37, 99, 235, 0.4);
  }

  .connect-btn:disabled {
    opacity: 0.75;
    cursor: wait;
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
    gap: 12px;
    max-width: 520px;
    padding: 12px 16px;
    text-align: left;
    border-radius: 12px;
    background: rgba(37, 99, 235, 0.12);
    border: 1px solid rgba(59, 130, 246, 0.3);
    color: #bfdbfe;
  }

  .signing-text strong {
    display: block;
    font-size: 13px;
    font-weight: 600;
    color: #eff6ff;
    margin-bottom: 2px;
  }

  .signing-text p {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: #93c5fd;
  }

  /* Error banner */
  .error-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--danger, #ef4444) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--danger, #ef4444) 36%, transparent);
    color: #fca5a5;
    font-size: 12.5px;
  }

  /* Features Grid */
  .features-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
    width: 100%;
    margin-bottom: 22px;
  }

  @media (max-width: 560px) {
    .features-grid {
      grid-template-columns: 1fr;
    }
  }

  .feature-card {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 14px 15px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.06);
    text-align: left;
    transition: background 140ms ease, border-color 140ms ease;
  }

  .feature-card:hover {
    background: rgba(255, 255, 255, 0.045);
    border-color: rgba(255, 255, 255, 0.1);
  }

  .feat-icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    flex: none;
    border-radius: 10px;
  }

  .feat-content h4 {
    margin: 0 0 3px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-1);
  }

  .feat-content p {
    margin: 0;
    font-size: 11.8px;
    line-height: 1.45;
    color: var(--text-3);
  }

  /* Security banner */
  .security-card {
    display: flex;
    align-items: flex-start;
    gap: 13px;
    width: 100%;
    padding: 13px 16px;
    border-radius: 12px;
    background: rgba(16, 185, 129, 0.06);
    border: 1px solid rgba(16, 185, 129, 0.2);
    text-align: left;
  }

  :global(.shield-icon) {
    color: #10b981;
    flex-shrink: 0;
    margin-top: 2px;
  }

  .security-text strong {
    display: block;
    font-size: 12.5px;
    font-weight: 600;
    color: #a7f3d0;
    margin-bottom: 2px;
  }

  .security-text p {
    margin: 0;
    font-size: 11.6px;
    line-height: 1.5;
    color: #6ee7b7;
    opacity: 0.88;
  }

  .security-text code {
    padding: 1px 4px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.25);
    font-family: var(--font-mono, monospace);
    font-size: 10.8px;
  }
</style>
