<script lang="ts">
  import FolderSync from "@lucide/svelte/icons/folder-sync";
  import Gamepad2 from "@lucide/svelte/icons/gamepad-2";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Info from "@lucide/svelte/icons/info";
  import PackagePlus from "@lucide/svelte/icons/package-plus";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import { account } from "../../account/account.svelte";

  const synced = [
    {
      icon: SlidersHorizontal,
      title: "App preferences",
      detail: "Dark theme, sidebar, page layouts, filters and sort order",
    },
    {
      icon: PackagePlus,
      title: "Install Apps",
      detail: "The apps you checked and the catalog picks you pinned",
    },
    {
      icon: Gamepad2,
      title: "Game Saves setup",
      detail: "Custom games, the backup schedule and each game's auto-backup switch",
    },
  ];
</script>

<section class="panel" aria-labelledby="synced-title">
  <header class="head">
    <div class="head-left">
      <span class="card-icon"><FolderSync size={16} /></span>
      <div>
        <h2 id="synced-title">What's saved</h2>
        <p class="sub">Cloud sync vs. local machine storage</p>
      </div>
    </div>
    <span class="scope" class:on={account.signedIn}>
      {account.signedIn ? "Synced to your account" : "Saved on this PC only"}
    </span>
  </header>

  <ul>
    {#each synced as item (item.title)}
      <li>
        <span class="icon"><item.icon size={16} /></span>
        <span class="text">
          <strong>{item.title}</strong>
          <small>{item.detail}</small>
        </span>
        <span class="chip" class:synced={account.signedIn}>{account.signedIn ? "Synced" : "This PC"}</span>
      </li>
    {/each}
    <li class="local">
      <span class="icon"><HardDrive size={16} /></span>
      <span class="text">
        <strong>Stays on this PC</strong>
        <small>Backup and game folders, restore locations, the save files themselves and which apps are installed</small>
      </span>
      <span class="chip">This PC</span>
    </li>
  </ul>

  <p class="note">
    <Info size={13} />
    <span>
      When two PCs change settings, the most recent change wins. Game Saves custom games only come over with the save
      folders that exist on this PC.
    </span>
  </p>
</section>

<style>
  .panel {
    display: grid;
    gap: 14px;
    padding: 18px;
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: var(--radius-lg);
    background: linear-gradient(180deg, rgb(255 255 255 / 0.045), rgb(255 255 255 / 0.018));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.06);
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .head-left {
    display: flex;
    align-items: center;
    gap: 11px;
    min-width: 0;
  }

  .card-icon {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.22);
    border-radius: 10px;
    background: linear-gradient(160deg, rgb(var(--accent-rgb) / 0.16), rgb(var(--accent-rgb) / 0.04));
    color: rgb(var(--accent-soft-rgb));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1);
  }

  h2 {
    font-size: 14.5px;
    line-height: 1.2;
  }

  .sub {
    margin-top: 2px;
    color: var(--text-3);
    font-size: 11.5px;
  }

  .scope {
    padding: 3px 10px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 999px;
    background: rgb(255 255 255 / 0.03);
    color: var(--text-3);
    font-size: 11px;
    font-weight: 500;
  }

  .scope.on {
    border-color: rgb(var(--accent-rgb) / 0.28);
    background: rgb(var(--accent-rgb) / 0.1);
    color: rgb(var(--accent-soft-rgb));
  }

  ul {
    display: grid;
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 11px 13px;
    border: 1px solid rgb(255 255 255 / 0.055);
    border-radius: 11px;
    background: rgb(0 0 0 / 0.16);
  }

  li.local {
    border-style: dashed;
    border-color: rgb(255 255 255 / 0.09);
    background: rgb(0 0 0 / 0.08);
  }

  .icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border: 1px solid rgb(var(--accent-rgb) / 0.18);
    border-radius: 9px;
    background: rgb(var(--accent-rgb) / 0.08);
    color: rgb(var(--accent-soft-rgb) / 0.92);
  }

  .local .icon {
    border-color: rgb(255 255 255 / 0.09);
    background: rgb(255 255 255 / 0.035);
    color: var(--text-3);
  }

  .text {
    display: grid;
    gap: 1px;
    flex: 1;
    min-width: 0;
  }

  strong {
    font-size: 12.5px;
    font-weight: 600;
  }

  small {
    color: var(--text-3);
    font-size: 11.5px;
    line-height: 1.45;
  }

  .chip {
    flex: none;
    padding: 2px 9px;
    border: 1px solid rgb(255 255 255 / 0.08);
    border-radius: 999px;
    color: var(--text-3);
    font-size: 10.5px;
    font-weight: 600;
  }

  .chip.synced {
    border-color: rgb(62 207 142 / 0.3);
    background: rgb(62 207 142 / 0.09);
    color: rgb(110 225 175);
  }

  .note {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding-top: 10px;
    border-top: 1px solid rgb(255 255 255 / 0.05);
    color: var(--text-3);
    font-size: 11px;
    line-height: 1.5;
  }

  .note :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--text-3);
  }
</style>
