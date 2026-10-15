<script lang="ts">
  import Cloud from "@lucide/svelte/icons/cloud";
  import LogOut from "@lucide/svelte/icons/log-out";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SyncStatus from "../../app/pages/password-manager/SyncStatus.svelte";
  import { passwords as p } from "../../app/pages/password-manager/state.svelte";
  import { account } from "../account.svelte";
  import { shell } from "../shell.svelte";
  import SignIn from "../SignIn.svelte";
  import SettingsCard from "./SettingsCard.svelte";

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
</script>

<SettingsCard
  icon={Cloud}
  title="Sync & backup"
  intro={profile
    ? "Your vault stays the same here and on your PC. Only encrypted copies leave this phone, so the sync server cannot read your passwords."
    : "Sign in with the account you use in MYLE on your PC: your vault comes here and stays the same on both."}
>
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
      <button class="btn ghost" disabled={account.signingOut} onclick={() => ((shell.settingsOpen = false), account.signOut())}><LogOut size={15} /> Sign out</button>
    </div>
  {:else}
    <SignIn />
  {/if}
</SettingsCard>

<style>
  .profile { display: flex; align-items: center; gap: 14px; min-width: 0; }

  .avatar { width: 48px; height: 48px; flex: none; }

  .avatar img,
  .initials { width: 100%; height: 100%; border: 2px solid rgb(var(--accent-rgb) / 0.4); border-radius: 50%; }

  .avatar img { object-fit: cover; }

  .initials { display: grid; place-items: center; background: var(--accent-grad); color: #fff; font-size: 17px; font-weight: 700; }

  .who { display: grid; min-width: 0; gap: 2px; overflow-wrap: anywhere; }

  .who strong { font-size: 15.5px; }

  .who small { color: var(--text-2); font-size: 13px; }

  .sync :global(.sync) { height: 48px; margin: -6px 0 -6px -10px; font-size: 13px; }

  .actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }

  .actions .btn { height: auto; min-height: 48px; padding: 8px; white-space: normal; }
</style>
