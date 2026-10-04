<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { scanner } from "./camera.svelte";
</script>

{#if scanner.active}
  <div class="scanner">
    <p>Point the camera at the site's 2FA QR code</p>
    <div class="frame" aria-hidden="true"></div>
    <button class="btn" onclick={() => scanner.stop()}><X size={16} /> Cancel</button>
  </div>
{/if}

<style>
  /* Over the camera, which shows through the see-through page. */
  .scanner {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    grid-template-rows: 1fr auto 1fr;
    justify-items: center;
    padding: calc(env(safe-area-inset-top) + 24px) 24px calc(env(safe-area-inset-bottom) + 32px);
    background: radial-gradient(circle at 50% 50%, transparent 0 34vmin, rgb(0 0 0 / 0.55) 35vmin);
    color: #fff;
  }

  p {
    align-self: end;
    margin-bottom: 24px;
    font-size: 15px;
    font-weight: 600;
    text-align: center;
    text-shadow: 0 1px 4px rgb(0 0 0 / 0.7);
  }

  .frame {
    width: 64vmin;
    height: 64vmin;
    border: 3px solid rgb(255 255 255 / 0.9);
    border-radius: 22px;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 0.3);
  }

  .btn {
    align-self: end;
    min-width: 140px;
    background: rgb(20 24 34 / 0.85);
  }
</style>
