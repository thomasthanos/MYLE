<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LogOut from "@lucide/svelte/icons/log-out";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import X from "@lucide/svelte/icons/x";
  import SyncStatus from "../app/pages/password-manager/SyncStatus.svelte";
  import { passwords as p } from "../app/pages/password-manager/state.svelte";
  import { account } from "./account.svelte";
  import { shell } from "./shell.svelte";
  import SignIn from "./SignIn.svelte";
  import ProviderSetup from "./ProviderSetup.svelte";

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

  function focusSheet(node: HTMLElement) {
    const previous = document.activeElement as HTMLElement | null;
    const controls = () => Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input:not(:disabled), [tabindex="0"]'));
    controls()[0]?.focus({ preventScroll: true });
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); close(); }
      if (event.key !== "Tab") return;
      const items = controls();
      const first = items[0], last = items.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    node.addEventListener("keydown", keydown);
    return { destroy() { node.removeEventListener("keydown", keydown); previous?.focus({ preventScroll: true }); } };
  }
</script>

{#if shell.accountOpen}
  <div class="backdrop" role="presentation" onclick={close} transition:fade={{ duration: 140 }}></div>
  <div class="sheet" role="dialog" aria-modal="true" aria-label="Account" tabindex="-1" use:focusSheet transition:fly={{ y: 40, duration: 200, easing: cubicOut }}>
    <div class="sheet-heading"><h2>Account</h2><button class="icon-btn" aria-label="Close account" onclick={close}><X size={20} /></button></div>
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
        <button class="btn ghost" disabled={account.signingOut} onclick={() => (close(), account.signOut())}><LogOut size={15} /> Sign out</button>
      </div>
    {:else}
      <h2>Sync with your PC</h2>
      <p>Sign in with the account you use in MYLE on your PC: your vault comes here and stays the same on both.</p>
      <SignIn />
    {/if}
    <ProviderSetup />
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 60; background: rgb(0 0 0 / 0.5); }

  .sheet { position: fixed; right: 0; bottom: 0; left: 0; z-index: 61; display: grid; gap: 16px; max-height: calc(100dvh - env(safe-area-inset-top) - 16px); overflow-y: auto; overscroll-behavior: contain; padding: 10px max(16px, env(safe-area-inset-right)) calc(env(safe-area-inset-bottom) + 20px) max(16px, env(safe-area-inset-left)); border-top: 1px solid rgb(255 255 255 / 0.1); border-radius: 20px 20px 0 0; background: var(--bg-1); }

  .sheet-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }

  .sheet-heading h2 { margin: 0; font-size: 16px; }

  .sheet-heading .icon-btn { width: 44px; height: 44px; flex: none; }

  h2 { font-size: 18px; }

  p { color: var(--text-2); font-size: 14px; }

  .profile { display: flex; align-items: center; gap: 14px; }

  .avatar { width: 52px; height: 52px; flex: none; }

  .avatar img,
  .initials { width: 100%; height: 100%; border: 2px solid rgb(var(--accent-rgb) / 0.4); border-radius: 50%; }

  .avatar img { object-fit: cover; }

  .initials { display: grid; place-items: center; background: var(--accent-grad); color: #fff; font-size: 18px; font-weight: 700; }

  .who { display: grid; min-width: 0; gap: 2px; overflow-wrap: anywhere; }

  .who strong { font-size: 16px; }

  .who small { color: var(--text-2); font-size: 13px; }

  .actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }

  .actions .btn { height: auto; min-height: 44px; padding: 8px; white-space: normal; }

  .sync :global(.sync) { margin-left: -10px; font-size: 13px; }
</style>
