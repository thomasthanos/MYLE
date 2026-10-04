<script lang="ts">
  import Download from "@lucide/svelte/icons/download";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import PackageCheck from "@lucide/svelte/icons/package-check";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import X from "@lucide/svelte/icons/x";
  import { updater as u } from "./updater.svelte";

  const update = $derived(u.update);
</script>

{#if u.visible && update}
  <div class="update" role="status">
    <div class="row">
      <span class="text">
        {#if u.step.kind === "downloading"}
          Downloading {update.version}{u.step.percent === null ? "…" : ` · ${u.step.percent}%`}
        {:else if u.step.kind === "installing"}
          Android's installer has {update.version}: tap <b>Update</b> there.
        {:else if u.step.kind === "allow"}
          Allow MYLE Passwords to install its updates, then come back.
        {:else if u.step.kind === "error"}
          {u.step.message}
        {:else}
          MYLE Passwords {update.version} is out.
        {/if}
      </span>

      {#if u.step.kind === "available"}
        <button class="btn small primary" onclick={() => u.start()}><Download size={14} /> Update</button>
      {:else if u.step.kind === "downloading"}
        <LoaderCircle size={16} class="spin" />
      {:else if u.step.kind === "installing"}
        <button class="btn small" onclick={() => u.install()}><PackageCheck size={14} /> Install</button>
      {:else if u.step.kind === "allow"}
        <button class="btn small primary" onclick={() => u.install()}><ShieldCheck size={14} /> Install</button>
      {:else}
        <button class="btn small primary" onclick={() => u.start()}>Retry</button>
      {/if}

      {#if u.step.kind !== "downloading"}
        <button class="icon-btn" aria-label="Later" onclick={() => (u.dismissed = true)}><X size={15} /></button>
      {/if}
    </div>

    {#if u.step.kind === "downloading"}
      <div class="track" aria-hidden="true">
        <div class="fill" class:moving={u.step.percent === null} style:width="{u.step.percent ?? 30}%"></div>
      </div>
    {:else if u.step.kind === "error"}
      <button class="link" onclick={() => u.inBrowser()}>Download it in the browser instead</button>
    {/if}
  </div>
{/if}

<style>
  .update {
    display: grid;
    flex: none;
    gap: 8px;
    padding: 8px 12px 8px 16px;
    border-bottom: 1px solid rgb(var(--accent-rgb) / 0.2);
    background: rgb(var(--accent-rgb) / 0.14);
    font-size: 13.5px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .text {
    flex: 1;
    min-width: 0;
    font-weight: 500;
  }

  .track {
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.12);
  }

  .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent-grad, #6366f1);
    transition: width 0.15s ease-out;
  }

  .fill.moving {
    animation: slide 1.2s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  .link {
    justify-self: start;
    min-height: 32px;
    color: #b7bef5;
    font-size: 12.5px;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
</style>
