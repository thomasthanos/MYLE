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
      You sign in on Discord's or Google's own page in your browser, so the app never sees your password. The session is kept encrypted for your Windows account on this PC.
    </p>
  {/if}
</section>

<style>
  .card { position: relative; display: grid; gap: 16px; padding: 18px; overflow: hidden; border: 1px solid rgb(var(--accent-rgb) / 0.22); border-radius: var(--radius-lg); background: var(--grain), radial-gradient(circle at 8% 0%, rgb(var(--accent-rgb) / 0.14), transparent 45%), linear-gradient(180deg, rgb(200 210 255 / 0.08), rgb(200 210 255 / 0.02) 65%, rgb(0 0 0 / 0.08)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.11), inset 0 -1px 0 rgb(0 0 0 / 0.28), var(--elev-2); }
  .rim { position: absolute; top: 0; left: 10%; width: 80%; height: 1px; background: linear-gradient(90deg, transparent, rgb(var(--accent-rgb) / 0.7), transparent); box-shadow: 0 0 14px rgb(var(--accent-rgb) / 0.4); }
  h2 { font-size: 16.5px; letter-spacing: -0.01em; }
  .intro { display: flex; align-items: flex-start; gap: 13px; }
  .intro p { margin-top: 4px; color: var(--text-2); font-size: 12.5px; line-height: 1.5; }
  .badge { display: grid; place-items: center; width: 42px; height: 42px; flex: none; border: 1px solid rgb(var(--accent-rgb) / 0.25); border-radius: 13px; background: linear-gradient(160deg, rgb(var(--accent-rgb) / 0.2), rgb(var(--accent-rgb) / 0.04)); color: rgb(var(--accent-soft-rgb)); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.14); }
  .providers { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  .provider { display: inline-flex; align-items: center; justify-content: center; gap: 10px; height: 42px; border-radius: 11px; font-size: 13px; font-weight: 600; transition: transform var(--dur-fast) var(--ease-out), filter var(--dur-fast), box-shadow var(--dur-fast); }
  .provider:hover { transform: translateY(-1px); filter: brightness(1.07); }
  .provider:active { transform: translateY(0); }
  .provider.discord { border: 1px solid rgb(255 255 255 / 0.14); background: #5865f2; color: #fff; box-shadow: none; }
  .provider.google { border: 1px solid rgb(0 0 0 / 0.08); background: #f5f6fa; color: #1f2330; box-shadow: none; }
  :global(:root.dark) .card { border-color: var(--glass-border); background: var(--surface-fill); box-shadow: var(--surface-depth); }
  :global(:root.dark) .rim { display: none; }
  :global(:root.dark) .provider { box-shadow: var(--btn-sheen); transition: background var(--dur-fast), scale 140ms var(--ease-out), translate 140ms var(--ease-out); }
  :global(:root.dark) .provider:hover { transform: none; filter: none; }
  :global(:root.dark) .provider.discord { border-color: transparent; background: var(--btn-primary); }
  :global(:root.dark) .provider.discord:hover { background: var(--btn-primary-hover); }
  :global(:root.dark) .provider.discord:active { background: var(--btn-primary-press); }
  :global(:root.dark) .provider.google { border-color: var(--btn-border); background: var(--btn-fill); color: var(--text-1); }
  :global(:root.dark) .provider.google:hover { background: var(--btn-fill-hover); }
  :global(:root.dark) .provider.google:active { background: var(--btn-fill-press); }
  .waiting { display: flex; align-items: center; gap: 12px; padding: 12px 14px; border: 1px solid rgb(var(--accent-rgb) / 0.22); border-radius: 12px; background: rgb(var(--accent-rgb) / 0.08); color: rgb(var(--accent-soft-rgb)); }
  .waiting span { display: grid; flex: 1; min-width: 0; }
  .waiting strong { color: var(--text-1); font-size: 13px; }
  .waiting small { color: var(--text-3); font-size: 11.5px; }
  .fine { display: flex; align-items: flex-start; gap: 7px; color: var(--text-3); font-size: 11px; line-height: 1.5; }
  .fine :global(svg) { flex: none; margin-top: 2px; }
  .profile { display: flex; align-items: center; gap: 14px; }
  .avatar { position: relative; width: 54px; height: 54px; flex: none; }
  .avatar img, .initials { width: 100%; height: 100%; border-radius: 50%; border: 2px solid rgb(var(--accent-rgb) / 0.4); box-shadow: 0 0 0 4px rgb(var(--accent-rgb) / 0.08), 0 10px 26px -12px rgb(0 0 0 / 0.8); }
  .avatar img { object-fit: cover; }
  .initials { display: grid; place-items: center; background: var(--accent-grad); color: #fff; font-size: 18px; font-weight: 700; }
  .provider-dot { position: absolute; right: -2px; bottom: -2px; display: grid; place-items: center; width: 22px; height: 22px; border: 2px solid var(--solid-fill-bottom); border-radius: 50%; }
  .provider-dot.discord { background: #5865f2; color: #fff; }
  .provider-dot.google { background: #fff; }
  .who { display: grid; gap: 3px; flex: 1; min-width: 0; }
  .who-top { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
  .who h2, .who p { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .who p { color: var(--text-2); font-size: 12.5px; }
  .via { flex: none; padding: 2px 9px; border: 1px solid rgb(255 255 255 / 0.09); border-radius: 999px; background: rgb(255 255 255 / 0.045); color: var(--text-2); font-size: 10.5px; font-weight: 500; }
  .sync-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 10px 12px; border: 1px solid rgb(255 255 255 / 0.06); border-radius: 11px; background: rgb(0 0 0 / 0.16); }
  .sync-state { display: inline-flex; align-items: center; gap: 8px; flex: 1; min-width: 0; color: var(--text-2); font-size: 12px; }
  .sync-state .text { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .sync-state.error { color: rgb(255 170 150 / 0.9); }
  .sync-actions { display: flex; align-items: center; gap: 6px; flex: none; }
  .ok-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--ok); box-shadow: 0 0 8px var(--ok-glow); }

  @media (max-width: 620px) {
    .providers { grid-template-columns: 1fr; }
    .sync-row { flex-wrap: wrap; }
  }
</style>
