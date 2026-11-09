<script lang="ts">
  // Favorite a game, or idle it by hand.
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import Star from "@lucide/svelte/icons/star";
  import { steamAutomationState as page } from "./state.svelte";

  let { game }: { game: { appid: number; name: string } } = $props();

  const favorite = $derived(page.settings?.favorites.includes(game.appid) ?? false);
  const idling = $derived(page.status?.manual.includes(game.appid) ?? false);
</script>

<button type="button" class:on={favorite} aria-pressed={favorite} aria-label={favorite ? `Remove ${game.name} from favorites` : `Add ${game.name} to favorites`} title={favorite ? "Remove from favorites" : "Add to favorites"} onclick={() => page.toggleFavorite(game.appid)}><Star size={14} /></button>
<button type="button" class:on={idling} aria-label={idling ? `Stop idling ${game.name}` : `Idle ${game.name}`} title={idling ? "Stop idling" : "Idle this game"} onclick={() => void page.idleGame(game.appid, !idling)}>{#if idling}<Square size={13} />{:else}<Play size={14} />{/if}</button>
