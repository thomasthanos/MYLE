<script lang="ts">
  let { title, subtitle = "" }: { title: string; subtitle?: string } = $props();
</script>

<header class="page-header">
  <div class="title-wrap">
    <h1>{title}</h1>
    <span class="ambient-glow" aria-hidden="true"></span>
  </div>
  {#if subtitle}<p class="subtitle">{subtitle}</p>{/if}
</header>

<style>
  .page-header { position: relative; margin-bottom: 22px; }

  .title-wrap { position: relative; display: inline-flex; align-items: center; }

  h1 { font-family: var(--font-handwritten); font-size: 28px; font-weight: 700; font-synthesis: none; line-height: 1.25; letter-spacing: 0; background: linear-gradient(180deg, #ffffff 15%, #c5cde0 100%); -webkit-background-clip: text; background-clip: text; -webkit-text-fill-color: transparent; filter: drop-shadow(0 2px 10px rgb(0 0 0 / 0.5)); margin: 0; }

  .ambient-glow { position: absolute; top: -6px; left: -14px; width: 80px; height: 42px; border-radius: 50%; background: radial-gradient(ellipse at center, rgb(var(--accent-rgb) / 0.18) 0%, transparent 70%); pointer-events: none; z-index: -1; }

  .subtitle { margin: 5px 0 0; color: var(--text-2); font-size: 13px; line-height: 1.45; letter-spacing: -0.005em; max-width: 68ch; }

  @media (max-width: 600px) {
    h1 {
      font-size: 23px;
    }
    .subtitle {
      font-size: 12px;
    }
  }
</style>
