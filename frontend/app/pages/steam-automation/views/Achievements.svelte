<script lang="ts">
  import { onMount } from "svelte";
  import { addAll, untilText } from "../model";
  import QueueView from "../QueueView.svelte";
  import { steamAutomationState as page } from "../state.svelte";

  const candidates = $derived((page.library ?? []).filter((game) => game.achievementsOk));
  const status = $derived(page.status);
  const settings = $derived(page.settings);
  const unlocked = $derived(status ? status.achievements.total - status.achievements.locked : 0);
  let now = $state(Date.now() / 1000);

  onMount(() => {
    const clock = setInterval(() => (now = Date.now() / 1000), 15_000);
    return () => clearInterval(clock);
  });
</script>

{#snippet extra()}
  <button class="btn" type="button" disabled={!settings} onclick={() => settings && void page.change({ achievementGames: addAll(settings.achievementGames, candidates) })}>Add all to queue</button>
{/snippet}

{#snippet note()}
  {#if status?.started.achievements}
    <p class="note">
      {#if status.achievements.waitingForFarming}Waiting for card farming to finish.{:else}{unlocked} of {status.achievements.total} unlocked in the queue{#if status.achievements.nextAt}. Next unlock {untilText(status.achievements.nextAt, now)}{/if}.{/if}
    </p>
  {/if}
{/snippet}

<QueueView find={page.find.achievements} queue="achievementGames" feature="achievements" {candidates} autoLabel="Run automatically" autoOn={settings?.achievementsOn ?? false} onAuto={(on) => void page.change({ achievementsOn: on })} emptyAll="No games with achievements to offer. Games with VAC are left out." {extra} {note} />
