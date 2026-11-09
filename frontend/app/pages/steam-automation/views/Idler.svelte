<script lang="ts">
  import { canQueueIdle, IDLE_LIMIT, plural } from "../model";
  import QueueView from "../QueueView.svelte";
  import { steamAutomationState as page } from "../state.svelte";

  const status = $derived(page.status);
  const settings = $derived(page.settings);
  const farming = $derived(!!status?.started.farm && status.farm.phase !== "done");
</script>

{#snippet note()}
  {#if status?.started.idle && settings}
    <p class="note">{farming ? "Waiting for card farming to finish." : `${plural(settings.idleGames.length, "game")} adding playtime.`}</p>
  {/if}
{/snippet}

<QueueView find={page.find.idler} queue="idleGames" feature="idle" candidates={page.library ?? []} autoLabel="Start with MYLE" autoOn={settings?.idleOn ?? false} onAuto={(on) => void page.change({ idleOn: on })} canAdd={(appid) => canQueueIdle(settings?.idleGames ?? [], appid)} fullHint={`Steam runs up to ${IDLE_LIMIT} games at once`} emptyAll="No games yet." {note} />
