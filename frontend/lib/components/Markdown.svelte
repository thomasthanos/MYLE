<script lang="ts">
  // Renders Markdown as text nodes only (lib/markdown.ts): no HTML from the
  // text is ever inserted, and links open in the default browser.
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { parseMarkdown, type Block, type Inline } from "../markdown";
  import { toast } from "../toast.svelte";

  let { source }: { source: string } = $props();

  const blocks = $derived(parseMarkdown(source));

  function open(event: MouseEvent, href: string) {
    event.preventDefault();
    if (event.type === "auxclick" && event.button !== 1) return;
    void openUrl(href).catch((error) => toast.error(`Could not open the link: ${error instanceof Error ? error.message : String(error)}`));
  }
</script>

{#snippet inlines(parts: Inline[])}
  {#each parts as part, i (i)}
    {#if part.t === "text"}{part.v}{:else if part.t === "strong"}<strong>{@render inlines(part.c)}</strong>{:else if part.t === "em"}<em>{@render inlines(part.c)}</em>{:else if part.t === "code"}<code>{part.v}</code>{:else if part.t === "br"}<br />{:else if part.t === "link"}<a
        href={part.href}
        title={part.href}
        rel="noopener noreferrer"
        onclick={(e) => open(e, part.href)}
        onauxclick={(e) => open(e, part.href)}>{@render inlines(part.c)}</a
      >{/if}
  {/each}
{/snippet}

{#snippet render(list: Block[])}
  {#each list as block, i (i)}
    {#if block.t === "h"}
      {#if block.level <= 2}<h3>{@render inlines(block.c)}</h3>{:else}<h4>{@render inlines(block.c)}</h4>{/if}
    {:else if block.t === "p"}
      <p>{@render inlines(block.c)}</p>
    {:else if block.t === "ul" || block.t === "ol"}
      <svelte:element this={block.t} start={block.t === "ol" && block.start !== 1 ? block.start : undefined}>
        {#each block.items as item, j (j)}
          <li>{@render inlines(item.c)}{#if item.sub.length}{@render render(item.sub)}{/if}</li>
        {/each}
      </svelte:element>
    {:else if block.t === "code"}
      <pre><code>{block.v}</code></pre>
    {:else if block.t === "quote"}
      <blockquote>{@render render(block.c)}</blockquote>
    {:else if block.t === "hr"}
      <hr />
    {:else if block.t === "table"}
      <div class="table">
        <table>
          <thead><tr>{#each block.head as cell, j (j)}<th>{@render inlines(cell)}</th>{/each}</tr></thead>
          <tbody>
            {#each block.rows as row, j (j)}
              <tr>{#each row as cell, k (k)}<td>{@render inlines(cell)}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/each}
{/snippet}

<div class="markdown selectable">{@render render(blocks)}</div>

<style>
  .markdown {
    color: var(--text-2);
    font-size: 13px;
    line-height: 1.6;
    overflow-wrap: anywhere;
  }

  .markdown :global(h3) {
    margin: 18px 0 6px;
    color: var(--text-1);
    font-size: 14.5px;
    font-weight: 650;
  }

  .markdown :global(h4) {
    margin: 14px 0 4px;
    color: var(--text-1);
    font-size: 13.5px;
    font-weight: 600;
  }

  .markdown > :global(:first-child) {
    margin-top: 0;
  }

  .markdown :global(p) {
    margin: 6px 0;
  }

  .markdown :global(ul),
  .markdown :global(ol) {
    margin: 6px 0;
    padding-left: 20px;
  }

  .markdown :global(li) {
    margin: 3px 0;
  }

  .markdown :global(strong) {
    color: var(--text-1);
    font-weight: 600;
  }

  .markdown :global(code) {
    padding: 1px 5px;
    border-radius: 5px;
    background: rgb(255 255 255 / 0.07);
    font-family: var(--font-mono);
    font-size: 11.5px;
  }

  .markdown :global(pre) {
    margin: 8px 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: rgb(0 0 0 / 0.25);
    overflow-x: auto;
  }

  .markdown :global(pre code) {
    padding: 0;
    background: none;
  }

  .markdown :global(a) {
    color: rgb(var(--accent-rgb));
    text-decoration: none;
  }

  .markdown :global(a:hover) {
    text-decoration: underline;
  }

  .markdown :global(blockquote) {
    margin: 8px 0;
    padding: 2px 12px;
    border-left: 3px solid rgb(var(--accent-rgb) / 0.4);
    color: var(--text-3);
  }

  .markdown :global(hr) {
    margin: 14px 0;
    border: 0;
    border-top: 1px solid rgb(255 255 255 / 0.08);
  }

  .markdown :global(.table) {
    margin: 8px 0;
    overflow-x: auto;
  }

  .markdown :global(table) {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
  }

  .markdown :global(th),
  .markdown :global(td) {
    padding: 6px 8px;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
    text-align: left;
    vertical-align: top;
  }

  .markdown :global(th) {
    color: var(--text-1);
    font-weight: 600;
  }
</style>
