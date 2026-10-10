<script lang="ts">
  import Brush from "@lucide/svelte/icons/brush";
  import Clapperboard from "@lucide/svelte/icons/clapperboard";
  import Download from "@lucide/svelte/icons/download";
  import FileText from "@lucide/svelte/icons/file-text";
  import Image from "@lucide/svelte/icons/image";
  import Package from "@lucide/svelte/icons/package";
  import Play from "@lucide/svelte/icons/play";
  import RefreshCcw from "@lucide/svelte/icons/refresh-ccw";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import Wrench from "@lucide/svelte/icons/wrench";
  import X from "@lucide/svelte/icons/x";
  import { creativeState, CLIP_STUDIO_ID, WAND_ENHANCER_ID, type CreativeApp, type Job } from "./state.svelte";

  let { app }: { app: CreativeApp } = $props();

  const job = $derived(creativeState.jobs[app.id]);
  let iconFailed = $state(false);

  const categoryIcons: Record<string, typeof Package> = {
    creative: Brush,
    productivity: FileText,
    photo: Image,
    video: Clapperboard,
    art: Brush,
    office: FileText,
    tools: Wrench,
    enhancement: WandSparkles,
  };
  const CategoryIcon = $derived(categoryIcons[app.category.toLowerCase()] ?? Package);

  const phaseLabel: Record<string, string> = {
    resolving: "Preparing…",
    downloading: "Downloading",
    verifying: "Verifying…",
    extracting: "Unpacking",
    installing: "Running setup…",
  };
  const statusText = $derived.by(() => {
    if (!job) return null;
    const label = phaseLabel[job.phase] ?? job.phase;
    if (job.phase !== "downloading" && job.phase !== "extracting") return label;
    const percent = job.progress !== null ? ` ${Math.round(job.progress * 100)}%` : "";
    const bytes =
      job.downloaded === undefined
        ? ""
        : job.total
          ? ` · ${size(job.downloaded)} / ${size(job.total)}`
          : ` · ${size(job.downloaded) ?? "0 MB"}`;
    return `${label}${percent}${bytes}${rate(job)}`;
  });

  /** " · 24 MB/s · 3 min left" while downloading, once a speed is known. */
  function rate(job: Job): string {
    if (job.phase !== "downloading" || !job.speed || job.speed < 1024) return "";
    const speed = ` · ${(job.speed / 1024 ** 2).toFixed(job.speed >= 10 * 1024 ** 2 ? 0 : 1)} MB/s`;
    if (!job.total || job.downloaded === undefined) return speed;
    const seconds = Math.max(0, (job.total - job.downloaded) / job.speed);
    const left = seconds < 60 ? `${Math.ceil(seconds)} s` : `${Math.ceil(seconds / 60)} min`;
    return `${speed} · ${left} left`;
  }

  function size(bytes: number | null) {
    if (!bytes) return null;
    const gb = bytes / 1024 ** 3;
    return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
  }

  /**
   * True only for Clip Studio once the restore exe is present in Downloads.
   * Checked instantly on page load and right after install — no polling delay.
   */
  const showRevert = $derived(
    app.id === CLIP_STUDIO_ID && creativeState.clipStudioRestoreReady,
  );

  /**
   * True only for Wand Enhancer once the exe is present in roaming directory.
   */
  const showWandLaunch = $derived(
    app.id === WAND_ENHANCER_ID && creativeState.wandEnhancerReady,
  );
</script>

<article class="card" class:working={!!job}>
  <div class="top">
    <span class="icon">
      {#if app.icon && !iconFailed}
        <img src={app.icon} alt="" width="34" height="34" loading="lazy" onerror={() => (iconFailed = true)} />
      {:else}
        <CategoryIcon size={22} strokeWidth={1.6} />
      {/if}
    </span>
    <span class="chip">{app.category}</span>
  </div>

  <h3>{app.name}</h3>
  <p class="desc">{app.description}</p>

  <div class="foot">
    {#if job}
      <div class="progress">
        <span class="status" title={job.file}>{statusText}{job.note ? ` · ${job.note}` : ""}</span>
        {#if job.file}<span class="file" title={job.file}>{job.file}</span>{/if}
        <div class="bar" class:indeterminate={job.progress === null}>
          <div class="fill" style:transform={job.progress === null ? undefined : `scaleX(${job.progress})`}></div>
        </div>
      </div>
      <button class="icon-btn" title="Cancel" aria-label="Cancel {app.name}" onclick={() => creativeState.cancel(app)}>
        <X size={16} />
      </button>
    {:else if showWandLaunch}
      <!-- After install: launch directly, with an icon button to rebuild/re-download if desired -->
      <button class="btn primary launch" onclick={() => creativeState.launchWandEnhancer()}>
        <Play size={14} />
        Run Wand Enhancer
      </button>
      <button
        class="icon-btn"
        title="Re-download & Build"
        aria-label="Rebuild {app.name}"
        onclick={() => creativeState.install(app)}
      >
        <RefreshCcw size={14} />
      </button>
    {:else if showRevert}
      <!-- After install: replace the primary button with the revert action -->
      <button class="btn revert" onclick={() => creativeState.swapExe()}>
        <RefreshCcw size={14} />
        Revert exe (v5.1.4)
      </button>
      <span class="meta">After a CELSYS update</span>
    {:else}
      <button class="btn primary" disabled={!app.configured} onclick={() => creativeState.install(app)}>
        <Download size={15} />
        {app.actionLabel}
      </button>
      <span class="meta">
        {#if !app.configured}
          No link yet
        {:else if size(app.sizeHint)}
          {size(app.sizeHint)}
        {/if}
      </span>
    {/if}
  </div>
</article>

<style>
  /* 3D glass card: lit top edge, inner depth, and a lift on hover.
     No backdrop-filter here: it sits inside the content panel. */
  .card { position: relative; display: flex; flex-direction: column; gap: 8px; min-width: 0; padding: 16px 16px 14px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: var(--radius-lg); background: var(--grain), linear-gradient(180deg, rgb(200 210 255 / 0.09), rgb(200 210 255 / 0.025) 60%, rgb(0 0 0 / 0.06)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.12), inset 0 -1px 0 rgb(0 0 0 / 0.25), var(--elev-2); transition: transform var(--dur-med) var(--ease-out), border-color var(--dur-fast), box-shadow var(--dur-med) var(--ease-out); }

  .card:hover { transform: translateY(-2px); border-color: rgb(255 255 255 / 0.16); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.18), inset 0 -1px 0 rgb(0 0 0 / 0.25), var(--elev-3); }

  .card.working { border-color: rgb(var(--accent-rgb) / 0.4); }

  .top { display: flex; align-items: center; justify-content: space-between; gap: 8px; }

  .icon { display: grid; place-items: center; width: 46px; height: 46px; border: 1px solid rgb(255 255 255 / 0.1); border-radius: 13px; background: linear-gradient(160deg, rgb(255 255 255 / 0.14), rgb(255 255 255 / 0.03)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.18); color: var(--accent); }

  img { border-radius: 8px; object-fit: contain; }

  .chip { padding: 3px 9px; border: 1px solid rgb(255 255 255 / 0.08); border-radius: 999px; background: rgb(255 255 255 / 0.04); color: var(--text-3); font-size: 11.5px; text-transform: capitalize; }

  h3 { margin-top: 2px; font-size: 15.5px; }

  .desc { flex: 1; color: var(--text-2); font-size: 12.5px; line-height: 1.5; }

  .foot { display: flex; align-items: center; gap: 10px; margin-top: 6px; }

  .meta { color: var(--text-3); font-size: 11.5px; font-variant-numeric: tabular-nums; }

  .progress { flex: 1; display: grid; gap: 5px; min-width: 0; }

  .status { overflow: hidden; color: var(--text-2); font-size: 11.5px; white-space: nowrap; text-overflow: ellipsis; font-variant-numeric: tabular-nums; }

  .file { overflow: hidden; color: var(--text-3); font-family: var(--font-mono); font-size: 11px; white-space: nowrap; text-overflow: ellipsis; }

  .bar { position: relative; height: 5px; overflow: hidden; border-radius: 999px; background: rgb(0 0 0 / 0.3); box-shadow: inset 0 1px 1px rgb(0 0 0 / 0.4); }

  .fill { position: absolute; inset: 0; border-radius: inherit; background: var(--accent-grad); box-shadow: 0 0 10px var(--accent-glow); transform-origin: left center; transform: scaleX(0); transition: transform 160ms linear; }

  .indeterminate .fill { width: 35%; transition: none; animation: sweep 1.2s var(--ease-in-out) infinite; }

  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(290%);
    }
  }

  /* Revert button: same size/layout as the primary, but muted amber tint */
  .btn.revert { display: inline-flex; align-items: center; gap: 6px; padding: 6px 14px; border: 1px solid rgb(255 200 80 / 0.25); border-radius: var(--radius-md, 8px); background: rgb(255 200 80 / 0.08); color: rgb(255 215 100); font-size: 13px; font-weight: 500; cursor: pointer; transition: background var(--dur-fast), border-color var(--dur-fast); }

  .btn.revert:hover { background: rgb(255 200 80 / 0.16); border-color: rgb(255 200 80 / 0.45); }
</style>
