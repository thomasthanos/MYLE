<script lang="ts">
  import Logo from "../lib/components/Logo.svelte";

  let {
    tone = "busy",
    progress = null,
    hold = 0,
  }: {
    tone?: "busy" | "done" | "error";
    /** 0..1 fills the ring; null spins an arc (no measure yet). */
    progress?: number | null;
    /** On an error: ms the ring takes to drain before the app opens. */
    hold?: number;
  } = $props();

  const R = 74;
  const LENGTH = 2 * Math.PI * R;
  const offset = $derived(tone === "done" ? 0 : progress === null ? LENGTH * 0.72 : LENGTH * (1 - progress));
</script>

<!-- The app's own icon in a progress ring. Every animated layer moves only by
     transform, opacity or stroke offset, so it never repaints the icon. -->
<div class="splash-logo {tone}" class:spinning={progress === null && tone === "busy"} style:--hold="{hold}ms">
  <div class="halo"></div>
  <svg class="ring" viewBox="0 0 168 168" aria-hidden="true">
    <defs>
      <linearGradient id="splash-ring" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#67e8f9" />
        <stop offset=".5" stop-color="#848ede" />
        <stop offset="1" stop-color="#34d399" />
      </linearGradient>
    </defs>
    <circle class="track" cx="84" cy="84" r={R} />
    <circle
      class="arc"
      cx="84"
      cy="84"
      r={R}
      stroke-dasharray={LENGTH}
      stroke-dashoffset={offset}
    />
  </svg>
  <div class="mark"><Logo size={112} /></div>
</div>

<style>
  .splash-logo {
    position: relative;
    display: grid;
    place-items: center;
    width: 168px;
    height: 168px;
  }

  .halo {
    position: absolute;
    inset: 10px;
    border-radius: 50%;
    background: radial-gradient(closest-side, rgb(var(--accent-rgb) / 0.5), rgb(79 209 232 / 0.12) 62%, transparent);
    transition: opacity var(--dur-slow) var(--ease-out);
  }

  .error .halo {
    background: radial-gradient(closest-side, rgb(229 72 77 / 0.42), transparent);
  }

  .ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
  }

  .track {
    fill: none;
    stroke: rgb(255 255 255 / 0.07);
    stroke-width: 3;
  }

  .arc {
    fill: none;
    stroke: url(#splash-ring);
    stroke-width: 3.5;
    stroke-linecap: round;
    transition: stroke-dashoffset 220ms linear;
    transform-origin: 84px 84px;
  }

  .done .arc {
    transition: stroke-dashoffset 360ms var(--ease-out);
  }

  .error .arc {
    stroke: #e5484d;
    stroke-dashoffset: 0;
  }

  .mark {
    position: relative;
    filter: drop-shadow(0 14px 22px rgb(0 0 0 / 0.6));
  }

  @media (prefers-reduced-motion: no-preference) {
    .halo {
      animation: pulse 2.2s var(--ease-in-out) infinite alternate;
    }

    .mark {
      animation:
        draw-in 700ms var(--ease-out) both,
        breathe 3.2s var(--ease-in-out) 700ms infinite alternate;
    }

    .ring {
      animation: ring-in 800ms var(--ease-out) 120ms both;
    }

    .spinning .arc {
      animation: spin 1.3s linear infinite;
    }

    .error .arc {
      animation: drain var(--hold) linear forwards;
    }

    .done .mark {
      animation: pop 420ms var(--ease-out) both;
    }
  }

  @keyframes draw-in {
    from {
      opacity: 0;
      transform: scale(0.82) rotate(-6deg);
    }
  }

  @keyframes breathe {
    to {
      transform: translateY(-3px) scale(1.015);
    }
  }

  @keyframes pulse {
    from {
      opacity: 0.55;
      transform: scale(0.9);
    }
    to {
      opacity: 1;
      transform: scale(1.06);
    }
  }

  @keyframes ring-in {
    from {
      opacity: 0;
      transform: rotate(-150deg) scale(0.92);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes drain {
    from {
      stroke-dashoffset: 0;
    }
    to {
      stroke-dashoffset: 465;
    }
  }

  @keyframes pop {
    50% {
      transform: scale(1.06);
    }
  }
</style>
