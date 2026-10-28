<script lang="ts">
  import {
    CARET,
    CODE,
    CODE_COMPACT,
    CODE_COMPACT_STROKE,
    CODE_STROKE,
    HOOD,
    HOOD_FOLDS,
    HOOD_HEM,
    PROMPT,
    SCREEN,
    SPARKLE,
    TILE,
    TORSO,
    TORSO_SEAMS,
  } from "../brand";

  let { size = 24 }: { size?: number } = $props();
  const uid = $props.id();

  // Below this the fine code lines turn to mush: draw fewer, bolder ones and skip the soft glows.
  const compact = $derived(size < 40);
  const code = $derived(compact ? CODE_COMPACT : CODE);
  const stroke = $derived(compact ? CODE_COMPACT_STROKE : CODE_STROKE);
</script>

<!-- Compact marks crop the margin around the tile: every pixel counts there. -->
<svg width={size} height={size} viewBox={compact ? "64 64 896 896" : "0 0 1024 1024"} aria-hidden="true">
  <defs>
    <linearGradient id="{uid}-bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#141c33" />
      <stop offset=".55" stop-color="#0b101e" />
      <stop offset="1" stop-color="#05070d" />
    </linearGradient>
    <radialGradient id="{uid}-spill" cx=".5" cy=".38" r=".6">
      <stop offset="0" stop-color="#22d3ee" stop-opacity=".48" />
      <stop offset=".42" stop-color="#6366f1" stop-opacity=".28" />
      <stop offset=".78" stop-color="#10b981" stop-opacity=".08" />
      <stop offset="1" stop-color="#05070d" stop-opacity="0" />
    </radialGradient>
    <linearGradient id="{uid}-rim" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#67e8f9" stop-opacity=".65" />
      <stop offset=".4" stop-color="#818cf8" stop-opacity=".2" />
      <stop offset="1" stop-color="#34d399" stop-opacity=".35" />
    </linearGradient>
    <linearGradient id="{uid}-screen" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color={compact ? "#0d2238" : "#09192b"} />
      <stop offset="1" stop-color="#071020" />
    </linearGradient>
    <radialGradient id="{uid}-screen-glow" cx=".5" cy=".95" r=".8">
      <stop offset="0" stop-color="#22d3ee" stop-opacity=".55" />
      <stop offset=".5" stop-color="#6366f1" stop-opacity=".2" />
      <stop offset="1" stop-color="#34d399" stop-opacity="0" />
    </radialGradient>
    <linearGradient id="{uid}-desk" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#22d3ee" stop-opacity="0" />
      <stop offset=".5" stop-color="#38bdf8" stop-opacity=".75" />
      <stop offset="1" stop-color="#22d3ee" stop-opacity="0" />
    </linearGradient>
    <linearGradient id="{uid}-hood" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#182238" />
      <stop offset=".55" stop-color="#0c111f" />
      <stop offset="1" stop-color="#050811" />
    </linearGradient>
    <linearGradient id="{uid}-torso" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#151e33" />
      <stop offset=".5" stop-color="#090d18" />
      <stop offset="1" stop-color="#04060b" />
    </linearGradient>
    <linearGradient id="{uid}-edge" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#67e8f9" />
      <stop offset=".32" stop-color="#34d399" stop-opacity=".85" />
      <stop offset=".68" stop-color="#818cf8" stop-opacity=".45" />
      <stop offset="1" stop-color="#6366f1" stop-opacity="0" />
    </linearGradient>
    <filter id="{uid}-soft" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="6" />
    </filter>
    <clipPath id="{uid}-tile">
      <rect x={TILE.x} y={TILE.y} width={TILE.size} height={TILE.size} rx={TILE.radius} />
    </clipPath>
    <clipPath id="{uid}-hood-clip"><path d={HOOD} /></clipPath>
    <clipPath id="{uid}-torso-clip"><path d={TORSO} /></clipPath>
  </defs>

  <rect x={TILE.x} y={TILE.y} width={TILE.size} height={TILE.size} rx={TILE.radius} fill="url(#{uid}-bg)" />
  <g clip-path="url(#{uid}-tile)">
    <rect x={TILE.x} y={TILE.y} width={TILE.size} height={TILE.size} fill="url(#{uid}-spill)" />

    {#if !compact}
      <!-- Ambient background cyber data columns -->
      <g stroke="#22d3ee" stroke-linecap="round" stroke-width="8" opacity=".16">
        <path d="M136 210v70M136 310v110M136 450v50M176 180v95M176 305v65M176 400v85" />
        <path d="M848 190v85M848 305v70M848 405v80M888 220v60M888 310v105M888 445v45" />
      </g>
    {/if}

    <rect x="64" y="652" width="896" height="308" fill="#05070e" opacity=".78" />
    <rect x="64" y="646" width="896" height="8" fill="url(#{uid}-desk)" />

    <!-- Cyber Terminal Frame -->
    <rect x="222" y="168" width="580" height="424" rx="42" fill="#060912" />
    {#if !compact}
      <rect x="224" y="170" width="576" height="420" rx="40" fill="none" stroke="#38bdf8" stroke-opacity=".32" stroke-width="4" />
      <!-- Terminal titlebar traffic dots & status line -->
      <circle cx="264" cy="191" r="7" fill="#fb7185" opacity=".85" />
      <circle cx="288" cy="191" r="7" fill="#fbbf24" opacity=".85" />
      <circle cx="312" cy="191" r="7" fill="#34d399" opacity=".9" />
      <path d="M348 191h120" stroke="#38bdf8" stroke-opacity=".28" stroke-width="8" stroke-linecap="round" />
    {/if}
    <rect x={SCREEN.x} y={SCREEN.y} width={SCREEN.width} height={SCREEN.height} rx={SCREEN.radius} fill="url(#{uid}-screen)" />
    <rect x={SCREEN.x} y={SCREEN.y} width={SCREEN.width} height={SCREEN.height} rx={SCREEN.radius} fill="url(#{uid}-screen-glow)" />

    <!-- Root `>` prompt chevron -->
    <path
      d={PROMPT}
      fill="none"
      stroke="#34d399"
      stroke-width={compact ? 26 : 16}
      stroke-linecap="round"
      stroke-linejoin="round"
    />

    <g stroke-linecap="round" stroke-width={stroke}>
      {#each code as t, i (i)}
        <path d="M{t.x} {t.y}h{t.w}" stroke={t.color} stroke-opacity={t.alpha ?? 1} />
      {/each}
    </g>
    {#if !compact}
      <rect x={CARET.x} y={CARET.y} width={CARET.width} height={CARET.height} rx="4" fill="#34d399" />
      <path d={SPARKLE} fill="#67e8f9" opacity=".92" />
    {/if}

    <path d={TORSO} fill="url(#{uid}-torso)" />
    <g clip-path="url(#{uid}-torso-clip)">
      {#if !compact}
        <path d={TORSO} fill="none" stroke="url(#{uid}-edge)" stroke-width="18" filter="url(#{uid}-soft)" />
        <path d={TORSO_SEAMS} fill="none" stroke="#38bdf8" stroke-opacity=".22" stroke-width="6" stroke-linecap="round" />
        <ellipse cx="512" cy="684" rx="156" ry="46" fill="#03050a" opacity=".82" filter="url(#{uid}-soft)" />
      {/if}
      <path d={TORSO} fill="none" stroke="url(#{uid}-edge)" stroke-width={compact ? 18 : 6} opacity=".85" />
    </g>
    <path d={HOOD} fill="url(#{uid}-hood)" />
    <g clip-path="url(#{uid}-hood-clip)">
      {#if !compact}
        <path d={HOOD} fill="none" stroke="url(#{uid}-edge)" stroke-width="20" filter="url(#{uid}-soft)" />
        <path d="M512 336V690" stroke="#03050a" stroke-opacity=".65" stroke-width="7" />
        <path d={HOOD_FOLDS} fill="none" stroke="#38bdf8" stroke-opacity=".22" stroke-width="6" stroke-linecap="round" />
      {/if}
      <path d={HOOD} fill="none" stroke="url(#{uid}-edge)" stroke-width={compact ? 20 : 6.5} />
      <path d={HOOD_HEM} fill="none" stroke="#67e8f9" stroke-opacity=".4" stroke-width={compact ? 14 : 6.5} />
    </g>
  </g>
  <rect x="70" y="70" width="884" height="884" rx="214" fill="none" stroke="url(#{uid}-rim)" stroke-width="12" />
</svg>
