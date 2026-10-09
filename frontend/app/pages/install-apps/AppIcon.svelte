<script lang="ts">
  import { CUSTOM_ICONS, iconSources as sourcesOf } from "./icons";
  import { appsState, type AppEntry, type Status } from "./state.svelte";

  let { app, status, size = 36 }: { app: AppEntry; status: Status; size?: number } = $props();

  const iconSources = $derived(sourcesOf(app));

  let rejectedSources = $state<string[]>([]);
  const src = $derived(iconSources.find((candidate) => !rejectedSources.includes(candidate)) ?? null);
  const isCustomSvg = $derived(!!src && src.startsWith("/icons/"));
  const isResolvedFavicon = $derived(!app.icon && !CUSTOM_ICONS[app.id] && !!src);

  function rejectSource() {
    if (src && !rejectedSources.includes(src)) rejectedSources = [...rejectedSources, src];
  }

  // A small raster becomes visibly soft in the 44px glass tile. Try the next
  // high-resolution provider, then use the crisp local letter tile.
  function onLoad(e: Event) {
    const image = e.currentTarget as HTMLImageElement;
    if (isResolvedFavicon && Math.min(image.naturalWidth, image.naturalHeight) < 48) rejectSource();
  }

  // Catalog search results carry no site: look it up once the card is visible.
  function lookUpWhenVisible(node: HTMLElement) {
    if (app.source !== "catalog" || app.site) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        observer.disconnect();
        appsState.resolveLinks(app);
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }

  // Fallback tile: the first letter on a hue derived from the id.
  const hue = $derived([...app.id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7));
  const label = { installed: "Installed", update: "Update available", missing: "Not installed", unknown: "Checking…" };
</script>

<div class="app-icon" class:custom-svg={isCustomSvg} style:--size="{size}px" {@attach lookUpWhenVisible}>
  {#if src}
    <img {src} alt="" width={size} height={size} loading="lazy" decoding="async" onload={onLoad} onerror={rejectSource} />
  {:else}
    <span class="letter" style:--hue={hue}>{app.name.charAt(0).toUpperCase()}</span>
  {/if}
  <span class="dot {status}" title={label[status]}></span>
</div>

<style>
  .app-icon {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: calc(var(--size) + 12px);
    height: calc(var(--size) + 12px);
    border: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 12px;
    background: linear-gradient(180deg, rgb(255 255 255 / 0.08), rgb(255 255 255 / 0.02));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08);
  }

  .app-icon.custom-svg {
    border: none;
    background: none;
    box-shadow: 0 6px 14px -8px rgb(0 0 0 / 0.75);
  }

  img {
    width: var(--size);
    height: var(--size);
    border-radius: 6px;
    object-fit: contain;
  }

  .app-icon.custom-svg img {
    width: 100%;
    height: 100%;
    border-radius: 11px;
    object-fit: cover;
  }

  .letter {
    display: grid;
    place-items: center;
    width: var(--size);
    height: var(--size);
    border-radius: 8px;
    background: linear-gradient(135deg, hsl(var(--hue) 70% 62%), hsl(calc(var(--hue) + 40) 70% 48%));
    color: #fff;
    font-family: var(--font-display);
    font-size: calc(var(--size) * 0.5);
    font-weight: 700;
  }

  .dot {
    position: absolute;
    right: -2px;
    bottom: -2px;
    width: 11px;
    height: 11px;
    border: 2px solid var(--solid-fill-top);
  }

  .dot.missing {
    background: var(--solid-fill-top);
  }
</style>
