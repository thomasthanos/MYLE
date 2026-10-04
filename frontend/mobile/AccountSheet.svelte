<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LogOut from "@lucide/svelte/icons/log-out";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SyncStatus from "../app/pages/password-manager/SyncStatus.svelte";
  import { passwords as p } from "../app/pages/password-manager/state.svelte";
  import { account } from "./account.svelte";
  import { shell } from "./shell.svelte";
  import SignIn from "./SignIn.svelte";

  let failedAvatar = $state<string | null>(null);
  const profile = $derived(account.profile);
  const name = $derived(profile?.name ?? profile?.email?.split("@")[0] ?? "Signed in");
  const initials = $derived(
    name
      .split(/\s+/)
      .map((part) => part[0])
      .join("")
      .slice(0, 2)
      .toUpperCase(),
  );

  const close = () => (shell.accountOpen = false);
</script>

{#if shell.accountOpen}
  <div class="backdrop" role="presentation" onclick={close} transition:fade={{ duration: 140 }}></div>
  <div class="sheet" role="dialog" aria-modal="true" aria-label="Account" transition:fly={{ y: 40, duration: 200, easing: cubicOut }}>
    <span class="grip" aria-hidden="true"></span>
    {#if profile}
      <div class="profile">
        <span class="avatar">
          {#if profile.avatarUrl && profile.avatarUrl !== failedAvatar}
            <img src={profile.avatarUrl} alt="" referrerpolicy="no-referrer" onerror={() => (failedAvatar = profile.avatarUrl ?? null)} />
          {:else}
            <span class="initials">{initials}</span>
          {/if}
        </span>
        <span class="who">
          <strong class="selectable">{name}</strong>
          {#if profile.email}<small class="selectable">{profile.email}</small>{/if}
        </span>
      </div>
      <div class="sync"><SyncStatus /></div>
      <div class="actions">
        <button class="btn" disabled={p.sync.kind === "syncing"} onclick={() => p.syncNow()}><RefreshCw size={15} /> Sync now</button>
        <button class="btn ghost" onclick={() => (close(), account.signOut())}><LogOut size={15} /> Sign out</button>
      </div>
    {:else}
      <h2>Sync with your PC</h2>
      <p>Sign in with the account you use in MYLE on your PC: your vault comes here and stays the same on both.</p>
      <SignIn />
    {/if}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgb(0 0 0 / 0.5);
  }

  .sheet {
    position: fixed;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 61;
    display: grid;
    gap: 16px;
    padding: 10px 20px calc(env(safe-area-inset-bottom) + 22px);
    border-top: 1px solid rgb(255 255 255 / 0.1);
    border-radius: 20px 20px 0 0;
    background: var(--bg-1);
  }

  .grip {
    justify-self: center;
    width: 40px;
    height: 4px;
    border-radius: 2px;
    background: rgb(255 255 255 / 0.2);
  }

  h2 {
    font-size: 18px;
  }

  p {
    color: var(--text-2);
    font-size: 14px;
  }

  .profile {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .avatar {
    width: 52px;
    height: 52px;
    flex: none;
  }

  .avatar img,
  .initials {
    width: 100%;
    height: 100%;
    border: 2px solid rgb(var(--accent-rgb) / 0.4);
    border-radius: 50%;
  }

  .avatar img {
    object-fit: cover;
  }

  .initials {
    display: grid;
    place-items: center;
    background: var(--accent-grad);
    color: #fff;
    font-size: 18px;
    font-weight: 700;
  }

  .who {
    display: grid;
    min-width: 0;
    gap: 2px;
  }

  .who strong {
    font-size: 16px;
  }

  .who small {
    overflow: hidden;
    color: var(--text-2);
    font-size: 13px;
    text-overflow: ellipsis;
  }

  .actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .sync :global(.sync) {
    margin-left: -10px;
    font-size: 13px;
  }
</style>
