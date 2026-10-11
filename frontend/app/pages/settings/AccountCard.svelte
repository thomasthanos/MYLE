<script lang="ts">
  import { onMount } from "svelte";
  import CloudUpload from "@lucide/svelte/icons/cloud-upload";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import LockKeyhole from "@lucide/svelte/icons/lock-keyhole";
  import LogOut from "@lucide/svelte/icons/log-out";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { account } from "../../account/account.svelte";
  import BrandIcon from "./BrandIcon.svelte";

  const providerNames = { discord: "Discord", google: "Google" } as const;

  let now = $state(Date.now());
  /** The picture that would not load: a new one (another sign-in) is tried. */
  let failedAvatar = $state<string | null>(null);

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  const profile = $derived(account.profile);
  const provider = $derived(profile?.provider === "discord" || profile?.provider === "google" ? profile.provider : null);
  const displayName = $derived(profile?.name ?? profile?.email?.split("@")[0] ?? "Signed in");
  const initials = $derived(displayName.split(/\s+/).map((part) => part[0]).join("").slice(0, 2).toUpperCase());

  const syncText = $derived.by(() => {
    if (account.syncing) return "Syncing…";
    if (account.error) return account.error;
    if (!account.lastSyncedAt) return "Not synced yet";
    const minutes = Math.round((now - account.lastSyncedAt) / 60_000);
    if (minutes < 1) return "Synced just now";
    if (minutes < 60) return `Synced ${minutes} min ago`;
    const hours = Math.round(minutes / 60);
    return hours < 24 ? `Synced ${hours} h ago` : `Synced ${new Date(account.lastSyncedAt).toLocaleDateString()}`;
  });
</script>

<section class="card" class:signed-in={!!profile} aria-labelledby="account-title">
  <span class="rim" aria-hidden="true"></span>

  {#if profile}
    <div class="profile">
      <span class="avatar">
        {#if profile.avatarUrl && profile.avatarUrl !== failedAvatar}
          <img src={profile.avatarUrl} alt="" referrerpolicy="no-referrer" onerror={() => (failedAvatar = profile.avatarUrl ?? null)} />
        {:else}
          <span class="initials">{initials}</span>
        {/if}
        {#if provider}<span class="provider-dot {provider}"><BrandIcon brand={provider} size={11} /></span>{/if}
      </span>
      <div class="who">
        <div class="who-top">
          <h2 id="account-title" class="selectable">{displayName}</h2>
          {#if provider}<span class="via">Signed in with {providerNames[provider]}</span>{/if}
        </div>
        {#if profile.email}<p class="selectable">{profile.email}</p>{/if}
      </div>
    </div>

    <div class="sync-row">
      <span class="sync-state" class:error={!!account.error && !account.syncing} title={syncText}>
        {#if account.syncing}<LoaderCircle size={13} class="spin" />{:else if account.error}<TriangleAlert size={13} />{:else}<span class="ok-dot" aria-hidden="true"></span>{/if}
        <span class="text">{syncText}</span>
      </span>
      <div class="sync-actions">
        <button class="btn small" disabled={account.syncing} onclick={() => account.sync(true)}>
          <RefreshCw size={13} class={account.syncing ? "spin" : ""} /> Sync now
        </button>
        <button class="btn small ghost" disabled={account.syncing} onclick={() => account.signOut()}><LogOut size={13} /> Sign out</button>
      </div>
    </div>
  {:else}
    <div class="intro">
      <span class="badge"><CloudUpload size={20} /></span>
      <div>
        <h2 id="account-title">Sync with your account</h2>
        <p>Sign in to keep your preferences, app picks and Game Saves setup the same on every PC.</p>
      </div>
    </div>

    {#if account.signingIn}
      <div class="waiting" role="status">
        <LoaderCircle size={18} class="spin" />
        <span>
          <strong>Continue in your browser</strong>
          <small>Finish signing in with {providerNames[account.signingIn]} in the tab that just opened.</small>
        </span>
        <button class="btn small" onclick={() => account.cancelSignIn()}>Cancel</button>
      </div>
    {:else}
      <div class="providers">
        <button class="provider discord" onclick={() => account.signIn("discord")}><BrandIcon brand="discord" size={19} /> Continue with Discord</button>
        <button class="provider google" onclick={() => account.signIn("google")}><BrandIcon brand="google" size={17} /> Continue with Google</button>
      </div>
    {/if}

    <p class="fine">
      <LockKeyhole size={12} />
      You sign in on Discord's or Google's own page in your browser, so the app never sees your password. The session
      is kept encrypted for your Windows account on this PC.
    </p>
  {/if}
</section>

<style>
  .card { position: relative; display: grid; gap: 10px; padding: 12px 14px; overflow: hidden; border: 1px solid var(--glass-border); border-radius: var(--radius-md); background: var(--surface-fill); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.05), var(--surface-depth); transition: border-color var(--dur-fast), box-shadow var(--dur-fast); }
  .card.signed-in { border-color: rgb(var(--accent-rgb) / 0.28); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08), 0 6px 18px -6px rgb(0 0 0 / 0.5); }
  .rim { display: none; }
  h2 { font-size: 14px; font-weight: 600; letter-spacing: -0.01em; margin: 0; }
  .intro { display: flex; align-items: center; gap: 10px; }
  .intro p { margin: 2px 0 0; color: var(--text-2); font-size: 11.5px; line-height: 1.35; }
  .badge { display: grid; place-items: center; width: 34px; height: 34px; flex: none; border: 1px solid rgb(var(--accent-rgb) / 0.25); border-radius: 10px; background: linear-gradient(160deg, rgb(var(--accent-rgb) / 0.2), rgb(var(--accent-rgb) / 0.04)); color: rgb(var(--accent-soft-rgb)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.14); }
  .providers { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .provider { display: inline-flex; align-items: center; justify-content: center; gap: 8px; height: 34px; border-radius: 9px; font-size: 12px; font-weight: 600; box-shadow: var(--btn-sheen); transition: background var(--dur-fast), filter var(--dur-fast), scale 140ms var(--ease-out); }
  .provider:hover { filter: brightness(1.06); }
  .provider:active { scale: 0.98; }
  .provider.discord { border: 1px solid rgb(255 255 255 / 0.12); background: #5865f2; color: #fff; box-shadow: 0 3px 10px -2px rgb(88 101 242 / 0.4); }
  .provider.discord:hover { background: #4752c4; }
  .provider.discord:active { background: #3c45a5; }
  .provider.google { border: 1px solid var(--btn-border); background: var(--btn-fill); color: var(--text-1); }
  .provider.google:hover { background: var(--btn-fill-hover); }
  .provider.google:active { background: var(--btn-fill-press); }
  .waiting { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border: 1px solid rgb(var(--accent-rgb) / 0.22); border-radius: 9px; background: rgb(var(--accent-rgb) / 0.08); color: rgb(var(--accent-soft-rgb)); }
  .waiting span { display: grid; flex: 1; min-width: 0; }
  .waiting strong { color: var(--text-1); font-size: 12px; }
  .waiting small { color: var(--text-3); font-size: 10.5px; }
  .fine { display: flex; align-items: flex-start; gap: 6px; color: var(--text-3); font-size: 10px; line-height: 1.4; margin: 0; }
  .fine :global(svg) { flex: none; margin-top: 1px; }
  .profile { display: flex; align-items: center; gap: 10px; }
  .avatar { position: relative; width: 40px; height: 40px; flex: none; }
  .avatar img, .initials { width: 100%; height: 100%; border-radius: 50%; border: 2px solid rgb(var(--accent-rgb) / 0.5); box-shadow: 0 0 14px -2px rgb(var(--accent-rgb) / 0.35); }
  .avatar img { object-fit: cover; }
  .initials { display: grid; place-items: center; background: var(--accent-grad); color: #fff; font-size: 14px; font-weight: 700; }
  .provider-dot { position: absolute; right: -2px; bottom: -2px; display: grid; place-items: center; width: 18px; height: 18px; border: 2px solid var(--solid-fill-bottom); border-radius: 50%; }
  .provider-dot.discord { background: #5865f2; color: #fff; }
  .provider-dot.google { background: #fff; }
  .who { display: grid; gap: 2px; flex: 1; min-width: 0; }
  .who-top { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-width: 0; }
  .who h2, .who p { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; margin: 0; }
  .who p { color: var(--text-2); font-size: 11.5px; }
  .via { flex: none; padding: 1px 7px; border: 1px solid rgb(var(--accent-rgb) / 0.22); border-radius: 999px; background: rgb(var(--accent-rgb) / 0.08); color: rgb(var(--accent-soft-rgb)); font-size: 10px; font-weight: 600; }
  .sync-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 10px; border: 1px solid rgb(255 255 255 / 0.07); border-radius: 9px; background: rgb(0 0 0 / 0.22); }
  .sync-state { display: inline-flex; align-items: center; gap: 6px; flex: 1; min-width: 0; color: var(--text-2); font-size: 11px; }
  .sync-state .text { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .sync-state.error { color: rgb(255 170 150 / 0.9); }
  .sync-actions { display: flex; align-items: center; gap: 5px; flex: none; }
  .sync-actions :global(.btn) { height: 26px; padding: 0 8px; font-size: 11px; }
  .ok-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--ok); box-shadow: 0 0 6px var(--ok-glow); }

  @media (min-width: 1550px) and (min-height: 850px), (min-width: 2200px) {
    .card {
      padding: 16px 18px;
      gap: 14px;
      border-radius: 14px;
    }
    .avatar {
      width: 48px;
      height: 48px;
    }
    .who h2 {
      font-size: 16px;
    }
    .who p {
      font-size: 13px;
    }
    .via {
      font-size: 11px;
      padding: 2px 9px;
    }
    .sync-row {
      padding: 8px 12px;
      border-radius: 10px;
    }
    .sync-state {
      font-size: 12.5px;
    }
    .sync-actions :global(.btn) {
      height: 29px;
      padding: 0 10px;
      font-size: 11.5px;
    }
    .badge {
      width: 40px;
      height: 40px;
    }
    .provider {
      height: 38px;
      font-size: 13px;
    }
    .intro p {
      font-size: 12.5px;
    }
  }

  @media (max-height: 750px) {
    .card {
      padding: 9px 11px;
      gap: 7px;
    }
    .avatar {
      width: 34px;
      height: 34px;
    }
    .who h2 {
      font-size: 13px;
    }
    .who p {
      font-size: 10.5px;
    }
    .sync-row {
      padding: 4px 7px;
    }
    .sync-state {
      font-size: 10px;
    }
    .sync-actions :global(.btn) {
      height: 23px;
      font-size: 10px;
      padding: 0 6px;
    }
  }

  @media (max-width: 620px) {
    .providers { grid-template-columns: 1fr; }
    .sync-row { flex-wrap: wrap; }
  }
</style>
