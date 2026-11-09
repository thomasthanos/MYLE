<script lang="ts">
  import { onMount } from "svelte";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import Package from "@lucide/svelte/icons/package";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Select from "../../../../lib/components/Select.svelte";
  import { confirm } from "../../../../lib/confirm.svelte";
  import type { InventoryItem, Listing } from "../api";
  import FeaturePanel from "../FeaturePanel.svelte";
  import { euros, filterItems, ITEM_SORTS, kindLabel, parseEuros, plural, requestedPrice, untilText, validMinimum, type ItemFilter } from "../model";
  import { steamAutomationState as page, type BulkKind } from "../state.svelte";

  const FILTERS: { value: ItemFilter; label: string }[] = [
    { value: "all", label: "All" },
    { value: "card", label: "Cards" },
    { value: "foil", label: "Foil" },
    { value: "background", label: "Backgrounds" },
    { value: "emoticon", label: "Emoticons" },
    { value: "booster", label: "Boosters" },
    { value: "duplicates", label: "Duplicates" },
    { value: "locked", label: "Locked" },
    { value: "onSale", label: "On sale" },
  ];

  let now = $state(Date.now() / 1000);
  const all = $derived(page.inventory ?? []);
  const onSaleView = $derived(page.itemFilter === "onSale");
  const busy = $derived(!!page.job);
  const ready = $derived(page.listingsKnown && !busy && !page.inventoryLoading);
  const selectedItems = $derived(page.bulkItems("selected"));
  const shownItems = $derived(page.bulkItems("all"));
  const duplicateItems = $derived(page.bulkItems("duplicates"));
  const autoOn = $derived(page.settings?.inventoryOn ?? false);
  const panelDetail = $derived.by(() => {
    const floor = euros(page.settings?.minimumCents ?? 5);
    const next = autoOn && page.status?.inventory.nextAt ? ` Next run ${untilText(page.status.inventory.nextAt, now)}.` : "";
    return `Every 6 hours MYLE lists your spare cards just under the cheapest price, never below ${floor}. One of each is always kept.${next}`;
  });

  function countOf(filter: ItemFilter): number {
    return filter === "onSale" ? page.listings.length : filterItems(all, filter, "", page.marks).length;
  }

  async function bulk(kind: BulkKind, count: number, title: string, label: string) {
    const warning = kind === "duplicates" ? "One of each card is kept." : "This can include your last copy of a card.";
    const message = `${plural(count, "item")} will go on the Steam Market at the price you typed, or just under the cheapest listing where you typed none. ${warning} Each one still needs confirming in Steam Guard.`;
    if (await confirm({ title, message, confirmLabel: "List items", danger: true })) void page.listBulk(kind, label);
  }

  async function removeAll() {
    const count = plural(page.listings.length, "listing");
    if (await confirm({ title: "Remove your listings?", message: `${count} will come off the Steam Market and the items will go back to your inventory.`, confirmLabel: "Remove listings", danger: true })) void page.removeListings([...page.listings]);
  }

  function setMinimum(text: string) {
    const cents = parseEuros(text);
    if (cents !== null && validMinimum(cents)) void page.change({ minimumCents: cents });
  }

  function priceBad(item: InventoryItem): boolean {
    return requestedPrice(page.prices[item.assetId]) === "invalid";
  }

  onMount(() => {
    void page.loadInventory(false);
    const clock = setInterval(() => (now = Date.now() / 1000), 15_000);
    return () => clearInterval(clock);
  });
</script>

{#if page.settings}
  <FeaturePanel title="Sell duplicates automatically" detail={panelDetail} active={autoOn}>
    <label class="floor">Never below<input class="input" type="text" inputmode="decimal" aria-label="Lowest price for automatic selling, in euros" value={euros(page.settings.minimumCents)} onchange={(event) => setMinimum(event.currentTarget.value)} /></label>
    <input class="switch" type="checkbox" aria-label="Sell duplicates automatically" checked={autoOn} onchange={(event) => void page.change({ inventoryOn: event.currentTarget.checked })} />
  </FeaturePanel>
{/if}

<div class="row">
  <div class="chips" role="group" aria-label="Show">
    {#each FILTERS as filter (filter.value)}
      <button class="chip" class:active={page.itemFilter === filter.value} type="button" aria-pressed={page.itemFilter === filter.value} onclick={() => (page.itemFilter = filter.value)}>{filter.label}<span class="count">{countOf(filter.value)}</span></button>
    {/each}
  </div>
</div>

<div class="row">
  {#if onSaleView}
    <span class="grow"></span>
    <button class="btn" type="button" disabled={busy || page.listings.length === 0} onclick={() => void removeAll()}>Remove all listings</button>
  {:else}
    <input class="input search" type="search" placeholder="Search" aria-label="Search your inventory" bind:value={page.itemQuery} />
    <Select bind:value={page.itemSort} options={ITEM_SORTS} ariaLabel="Sort by" />
    <span class="grow"></span>
    {#if selectedItems.length > 0}
      <button class="btn primary" type="button" disabled={!ready} onclick={() => void bulk("selected", selectedItems.length, "List the selected items?", "Listing selected items")}>List selected ({selectedItems.length})</button>
    {/if}
    <button class="btn" type="button" disabled={!ready || duplicateItems.length === 0} onclick={() => void bulk("duplicates", duplicateItems.length, "List your duplicates?", "Listing duplicates")}>List duplicates ({duplicateItems.length})</button>
    <button class="btn" type="button" disabled={!ready || shownItems.length === 0} onclick={() => void bulk("all", shownItems.length, "List everything shown?", "Listing items")}>List all ({shownItems.length})</button>
  {/if}
  <button class="icon-btn" type="button" aria-label="Read the inventory again" title="Read the inventory again" disabled={page.inventoryLoading || busy} onclick={() => void page.loadInventory(true)}><RefreshCw size={15} /></button>
</div>

{#if page.inventoryError}
  <div class="empty" role="alert"><p>{page.inventoryError}</p><button class="btn small" type="button" onclick={() => void page.loadInventory(true)}>Try again</button></div>
{:else if page.inventoryLoading && all.length === 0}
  <p class="empty" aria-busy="true">Reading your inventory from Steam…</p>
{:else if onSaleView}
  <div class="items">
    {#each page.listings as listing (listing.listingId)}{@render forSale(listing)}{:else}<p class="empty">Nothing on sale.</p>{/each}
  </div>
{:else}
  <div class="items">
    {#each page.shownItems as item (item.assetId)}{@render entry(item)}{:else}<p class="empty">Nothing here.</p>{/each}
  </div>
{/if}

{#snippet art(url: string | null)}
  {#if url}<img src={url} alt="" loading="lazy" decoding="async" />{:else}<span class="blank"><Package size={20} /></span>{/if}
{/snippet}

{#snippet entry(item: InventoryItem)}
  {@const locked = page.locked.has(item.assetId)}
  {@const sale = page.onSale.has(item.assetId)}
  {@const lowest = page.lowestOf(item.hashName, now)}
  {@const canPick = !locked && !sale}
  <article class="item surface" class:picked={page.selected.has(item.assetId)}>
    <div class="top">
      <input class="check" type="checkbox" aria-label={`Select ${item.name}`} checked={page.selected.has(item.assetId)} disabled={!canPick} onchange={() => page.toggleSelected(item.assetId)} />
      {@render art(item.iconUrl)}
      <div class="meta"><b title={item.name}>{item.name}</b><span title={item.game}>{item.game}</span><span class="type">{kindLabel(item.kind)}</span></div>
      <button class="icon-btn" class:active={locked} type="button" aria-pressed={locked} aria-label={locked ? `Unlock ${item.name}` : `Lock ${item.name}`} title={locked ? "Locked: it will never be sold" : "Lock it so it is never sold"} onclick={() => page.toggleLocked(item.assetId)}>{#if locked}<Lock size={14} />{:else}<LockOpen size={14} />{/if}</button>
    </div>
    {#if sale}
      <p class="state">On sale</p>
    {:else}
      <div class="price">
        <input class="input" class:bad={priceBad(item)} type="text" inputmode="decimal" placeholder={lowest && lowest.cents !== null ? euros(lowest.cents) : "Market price"} title="Leave empty to sell just under the cheapest listing" aria-label={`Price for ${item.name}, in euros`} aria-invalid={priceBad(item)} disabled={locked} bind:value={page.prices[item.assetId]} />
        <button class="btn small" type="button" disabled={busy || locked || priceBad(item)} onclick={() => void page.listItems([item], "Listing")}>List</button>
      </div>
      <button class="link" type="button" disabled={page.pricing.has(item.assetId)} onclick={() => void page.lookupPrice(item)}>{page.pricing.has(item.assetId) ? "Checking…" : lowest ? (lowest.cents === null ? "No market price" : `Cheapest now ${euros(lowest.cents)}`) : "Check the Market price"}</button>
    {/if}
  </article>
{/snippet}

{#snippet forSale(listing: Listing)}
  <article class="item surface">
    <div class="top">
      {@render art(listing.iconUrl)}
      <div class="meta"><b title={listing.name}>{listing.name || "Unknown item"}</b><span>{euros(listing.buyerCents)}</span><span class="type">{listing.waitingConfirmation ? "Waiting for Steam Guard" : "On sale"}</span></div>
    </div>
    <div class="price end"><button class="btn small" type="button" disabled={busy} onclick={() => void page.removeListings([listing])}>Remove</button></div>
  </article>
{/snippet}

<style>
  .floor { display: inline-flex; align-items: center; gap: 8px; color: var(--text-2); font-size: 12.5px; }
  .floor .input { width: 76px; height: 30px; text-align: right; }
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .grow { flex: 1 1 0; }
  .search { flex: 0 1 220px; min-width: 140px; }
  .items { display: grid; grid-template-columns: repeat(auto-fill, minmax(226px, 1fr)); gap: 10px; }
  .item { padding: 10px; min-width: 0; content-visibility: auto; contain-intrinsic-size: auto 226px auto 132px; }
  .item.picked { border-color: rgb(var(--accent-rgb) / 0.6); }
  .top { display: flex; gap: 9px; align-items: center; }
  img, .blank { flex: none; width: 54px; height: 62px; border-radius: 7px; object-fit: contain; background: var(--input-fill); }
  .blank { display: grid; place-items: center; color: var(--text-3); }
  .meta { display: grid; flex: 1; min-width: 0; gap: 1px; }
  .meta b, .meta span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .meta b { font-size: 13px; }
  .meta span { color: var(--text-2); font-size: 11.5px; }
  .meta .type { justify-self: start; max-width: 100%; margin-top: 2px; padding: 0 6px; border-radius: 5px; background: var(--chip-fill); }
  .state { margin: 11px 0 3px; color: var(--ok); font-size: 12px; font-weight: 700; text-align: center; }
  .price { display: flex; gap: 6px; align-items: center; margin-top: 9px; }
  .price.end { justify-content: flex-end; }
  .price .input { flex: 1; min-width: 0; height: 28px; font-size: 12.5px; text-align: right; }
  .price .input.bad { border-color: var(--danger); }
  .link { display: block; width: 100%; margin-top: 6px; padding: 0; border: 0; background: none; color: var(--accent); font: inherit; font-size: 12px; text-align: center; cursor: pointer; }
  .link:hover:not(:disabled), .link:focus-visible { text-decoration: underline; }
  .link:disabled { color: var(--text-3); cursor: progress; }
  .empty { grid-column: 1 / -1; margin: 40px auto; color: var(--text-3); font-size: 13px; text-align: center; }
  .empty p { margin: 0 0 10px; }
</style>
