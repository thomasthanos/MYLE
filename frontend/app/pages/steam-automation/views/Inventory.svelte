<script lang="ts">
  import { onMount } from "svelte";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import Package from "@lucide/svelte/icons/package";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ProgressBar from "../../../../lib/components/ProgressBar.svelte";
  import Select from "../../../../lib/components/Select.svelte";
  import { confirm } from "../../../../lib/confirm.svelte";
  import type { InventoryItem, Listing } from "../api";
  import { duplicateIds, euros, filterItems, ITEM_SORTS, kindLabel, listable, parseEuros, plural, requestedPrice, sortItems, untilText, validMinimum, type ItemFilter, type ItemMarks } from "../model";
  import { steamAutomationState as page } from "../state.svelte";

  const FILTERS: { value: ItemFilter; label: string }[] = [
    { value: "all", label: "Everything" },
    { value: "card", label: "Trading cards" },
    { value: "foil", label: "Foil" },
    { value: "background", label: "Backgrounds" },
    { value: "emoticon", label: "Emoticons" },
    { value: "booster", label: "Booster packs" },
    { value: "onSale", label: "On sale" },
    { value: "duplicates", label: "Duplicates" },
    { value: "locked", label: "Locked" },
    { value: "selected", label: "Selected" },
  ];

  const all = $derived(page.inventory ?? []);
  const duplicates = $derived(duplicateIds(all, page.onSale));
  const marks = $derived<ItemMarks>({ selected: page.selected, locked: page.locked, onSale: page.onSale, duplicates });
  const shown = $derived(sortItems(filterItems(all, page.itemFilter, page.itemQuery, marks), page.itemSort));
  const showingListings = $derived(page.itemFilter === "onSale");
  const canList = $derived(listable(all, page.locked, page.onSale));
  const canListNow = $derived(listable(shown, page.locked, page.onSale));
  const canListIds = $derived(new Set(canList.map((item) => item.assetId)));
  const selectedItems = $derived(canList.filter((item) => page.selected.has(item.assetId)));
  const duplicateItems = $derived(canList.filter((item) => duplicates.has(item.assetId)));
  const waiting = $derived(page.listings.filter((listing) => listing.waitingConfirmation).length);
  const busy = $derived(!!page.job);

  function countOf(filter: ItemFilter): number {
    return filter === "onSale" ? page.listings.length : filterItems(all, filter, "", marks).length;
  }

  async function ask(items: InventoryItem[], title: string, lastCopies: boolean, label: string) {
    const warning = lastCopies ? " This can include your last copy of a card." : " One of each card is kept.";
    const message = `${plural(items.length, "item")} will go on the Steam Market at the price in its field, or a cent under the cheapest listing if the field is empty.${warning} Each one still needs confirming in Steam Guard.`;
    if (await confirm({ title, message, confirmLabel: "List items", danger: true })) void page.listItems(items, label);
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
  });
</script>

<div class="bar">
  <h2>Inventory Manager</h2>
  <span class="count">{all.length}</span>
  <span class="grow"></span>
  {#if page.settings}
    <label class="auto">Sell duplicates automatically<input class="switch" type="checkbox" checked={page.settings.inventoryOn} onchange={(event) => void page.change({ inventoryOn: event.currentTarget.checked })} /></label>
  {/if}
  <button class="btn" type="button" disabled={busy || selectedItems.length === 0} onclick={() => void ask(selectedItems, "List the selected items?", true, "Listing selected items")}>List selected ({selectedItems.length})</button>
  <button class="btn" type="button" disabled={busy || canListNow.length === 0 || showingListings} onclick={() => void ask(canListNow, "List everything shown?", true, "Listing items")}>List all ({showingListings ? 0 : canListNow.length})</button>
  <button class="btn" type="button" disabled={busy || duplicateItems.length === 0} onclick={() => void ask(duplicateItems, "List your duplicates?", false, "Listing duplicates")}>List duplicates ({duplicateItems.length})</button>
  <button class="btn" type="button" disabled={busy || page.listings.length === 0} onclick={() => void removeAll()}>Remove listings ({page.listings.length})</button>
  <button class="icon-btn" type="button" aria-label="Read the inventory again" title="Read the inventory again" disabled={page.inventoryLoading || busy} onclick={() => void page.loadInventory(true)}><RefreshCw size={15} /></button>
</div>

{#if page.job}
  <div class="job surface">
    <div class="job-line"><span>{page.job.label}: {page.job.progress.done} of {page.job.total}</span><button class="btn small" type="button" onclick={() => page.stopJob()}>Stop</button></div>
    <ProgressBar value={page.job.progress.done / page.job.total} label={page.job.label} />
  </div>
{/if}
{#if page.settings?.inventoryOn && page.status?.inventory.nextAt}<p class="note">Next automatic run {untilText(page.status.inventory.nextAt, Date.now() / 1000)}. It sells duplicates only and always keeps one of each.</p>{/if}
{#if waiting > 0}<p class="note warn">{plural(waiting, "listing")} waiting for your confirmation in Steam Guard. MYLE never approves these for you.</p>{/if}

<div class="layout">
  <aside>
    <input class="input" type="search" placeholder="Search" aria-label="Search your inventory" bind:value={page.itemQuery} />
    <p class="label">Sort by</p>
    <Select bind:value={page.itemSort} options={ITEM_SORTS} ariaLabel="Sort by" fullWidth />
    <p class="label">Filter</p>
    <div class="filters">
      {#each FILTERS as filter (filter.value)}
        <button class="filter" class:on={page.itemFilter === filter.value} type="button" aria-pressed={page.itemFilter === filter.value} onclick={() => (page.itemFilter = filter.value)}>{filter.label}<i>{countOf(filter.value)}</i></button>
      {/each}
    </div>
    {#if page.settings}
      <p class="label">Never below</p>
      <input class="input" type="text" inputmode="decimal" aria-label="Lowest price for automatic selling, in euros" value={euros(page.settings.minimumCents)} onchange={(event) => setMinimum(event.currentTarget.value)} />
    {/if}
  </aside>

  <div class="items-area">
    {#if page.inventoryError}
      <div class="empty" role="alert"><p>{page.inventoryError}</p><button class="btn small" type="button" onclick={() => void page.loadInventory(true)}>Try again</button></div>
    {:else if page.inventoryLoading && all.length === 0}
      <p class="empty" aria-busy="true">Reading your inventory from Steam…</p>
    {:else if showingListings}
      <div class="items">
        {#each page.listings as listing (listing.listingId)}{@render onSale(listing)}{:else}<p class="empty">Nothing on sale.</p>{/each}
      </div>
    {:else}
      <div class="items">
        {#each shown as item (item.assetId)}{@render entry(item)}{:else}<p class="empty">Nothing matches.</p>{/each}
      </div>
    {/if}
  </div>
</div>

{#snippet art(url: string | null)}
  {#if url}<img src={url} alt="" loading="lazy" decoding="async" />{:else}<span class="blank"><Package size={20} /></span>{/if}
{/snippet}

{#snippet entry(item: InventoryItem)}
  {@const locked = page.locked.has(item.assetId)}
  {@const sale = page.onSale.has(item.assetId)}
  {@const known = page.lowest[item.hashName]}
  <article class="item surface" class:picked={page.selected.has(item.assetId)}>
    <div class="top">
      <input class="check" type="checkbox" aria-label={`Select ${item.name}`} checked={page.selected.has(item.assetId)} disabled={!canListIds.has(item.assetId)} onchange={() => page.toggleSelected(item.assetId)} />
      {@render art(item.iconUrl)}
      <div class="meta"><b title={item.name}>{item.name}</b><span title={item.game}>{item.game}</span><span class="type">{kindLabel(item.kind)}{item.amount > 1 ? ` × ${item.amount.toLocaleString("en-US")}` : ""}</span></div>
      <button class="icon-btn" class:active={locked} type="button" aria-pressed={locked} aria-label={locked ? `Unlock ${item.name}` : `Lock ${item.name}`} title={locked ? "Locked, never sold" : "Lock so it is never sold"} onclick={() => page.toggleLocked(item.assetId)}>{#if locked}<Lock size={14} />{:else}<LockOpen size={14} />{/if}</button>
    </div>
    {#if sale}
      <p class="state">On sale</p>
    {:else if !item.marketable}
      <p class="state">Cannot be sold</p>
    {:else}
      <button class="link" type="button" disabled={page.pricing.has(item.assetId)} onclick={() => void page.lookupPrice(item)}>{page.pricing.has(item.assetId) ? "Checking the Market…" : known === null ? "No market price" : known !== undefined ? `Lowest now ${euros(known)}` : "Get market price"}</button>
      <div class="price">
        <input class="input" class:bad={priceBad(item)} type="text" inputmode="decimal" placeholder="Market price" aria-label={`Price for ${item.name}, in euros`} aria-invalid={priceBad(item)} bind:value={page.prices[item.assetId]} />
        <button class="btn small" type="button" disabled={busy || locked || priceBad(item)} onclick={() => void page.listItems([item], "Listing")}>List</button>
      </div>
    {/if}
  </article>
{/snippet}

{#snippet onSale(listing: Listing)}
  <article class="item surface">
    <div class="top">
      {@render art(listing.iconUrl)}
      <div class="meta"><b title={listing.name}>{listing.name || "Unknown item"}</b><span>{euros(listing.buyerCents)}</span><span class="type">{listing.waitingConfirmation ? "Waiting for Steam Guard" : "On sale"}</span></div>
    </div>
    <div class="price end"><button class="btn small" type="button" disabled={busy} onclick={() => void page.removeListings([listing])}>Remove</button></div>
  </article>
{/snippet}

<style>
  .job { display: grid; gap: 7px; margin-bottom: 12px; padding: 10px 12px; }
  .job-line { display: flex; justify-content: space-between; align-items: center; gap: 10px; font-size: 12.5px; }
  .layout { display: grid; grid-template-columns: 172px minmax(0, 1fr); gap: 16px; align-items: start; }
  aside { position: sticky; top: 0; display: flex; flex-direction: column; gap: 4px; }
  .label { margin: 10px 2px 3px; color: var(--text-3); font-size: 11px; font-weight: 700; }
  .filters { display: flex; flex-direction: column; gap: 1px; }
  .filter { display: flex; justify-content: space-between; align-items: center; padding: 6px 9px; border: 0; border-radius: var(--radius-sm); background: none; color: var(--text-2); font: inherit; font-size: 12.5px; text-align: left; cursor: pointer; }
  .filter:hover { background: var(--hover); }
  .filter.on { background: var(--selected); color: var(--text-1); }
  .filter i { color: var(--text-3); font-style: normal; font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .items-area { min-width: 0; }
  .items { display: grid; grid-template-columns: repeat(auto-fill, minmax(216px, 1fr)); gap: 10px; }
  .item { padding: 10px; min-width: 0; content-visibility: auto; contain-intrinsic-size: 216px 138px; }
  .item.picked { border-color: rgb(var(--accent-rgb) / 0.6); }
  .top { display: flex; gap: 9px; align-items: center; }
  img, .blank { flex: none; width: 54px; height: 62px; border-radius: 7px; object-fit: contain; background: var(--input-fill); }
  .blank { display: grid; place-items: center; color: var(--text-3); }
  .meta { display: grid; flex: 1; min-width: 0; gap: 1px; }
  .meta b, .meta span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .meta b { font-size: 13px; }
  .meta span { color: var(--text-2); font-size: 11.5px; }
  .meta .type { justify-self: start; max-width: 100%; margin-top: 2px; padding: 0 6px; border-radius: 5px; background: var(--chip-fill); }
  .state { margin: 9px 0 2px; color: var(--text-3); font-size: 12px; text-align: center; }
  .link { display: block; width: 100%; margin-top: 7px; padding: 0; border: 0; background: none; color: var(--accent); font: inherit; font-size: 12px; text-align: center; cursor: pointer; }
  .link:hover:not(:disabled), .link:focus-visible { text-decoration: underline; }
  .link:disabled { color: var(--text-3); cursor: progress; }
  .price { display: flex; gap: 6px; align-items: center; margin-top: 7px; }
  .price.end { justify-content: flex-end; }
  .price .input { flex: 1; min-width: 0; height: 28px; font-size: 12.5px; text-align: right; }
  .price .input.bad { border-color: var(--danger); }
  .empty { grid-column: 1 / -1; margin: 32px auto; color: var(--text-3); font-size: 13px; text-align: center; }
  .empty p { margin: 0 0 10px; }
  @media (max-width: 860px) { .layout { grid-template-columns: minmax(0, 1fr); } aside { position: static; } }
</style>
