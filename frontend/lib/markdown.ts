// A small Markdown reader for release notes. It turns the text into plain
// data that components render as text nodes: nothing from the notes ever
// becomes HTML, raw HTML in the notes is reduced to its text, and links are
// kept only when they go to http(s) addresses.

export type Inline =
  | { t: "text"; v: string }
  | { t: "strong"; c: Inline[] }
  | { t: "em"; c: Inline[] }
  | { t: "code"; v: string }
  | { t: "link"; href: string; c: Inline[] }
  | { t: "br" };

export interface ListItem {
  c: Inline[];
  sub: Block[];
}

export type Block =
  | { t: "h"; level: number; c: Inline[] }
  | { t: "p"; c: Inline[] }
  | { t: "ul"; items: ListItem[] }
  | { t: "ol"; start: number; items: ListItem[] }
  | { t: "code"; v: string }
  | { t: "quote"; c: Block[] }
  | { t: "hr" }
  | { t: "table"; head: Inline[][]; rows: Inline[][][] };

const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", "#39": "'" };

/** Only web addresses are links; anything else (javascript:, file:, data:) is not. */
export function safeHref(href: string): string | null {
  const url = href.trim();
  return /^https?:\/\/[^\s]+$/i.test(url) ? url : null;
}

function decode(text: string): string {
  return text.replace(/&(#\d+|#x[0-9a-f]+|[a-z]+);/gi, (whole, name: string) => {
    const lower = name.toLowerCase();
    if (lower in ENTITIES) return ENTITIES[lower];
    const code = lower.startsWith("#x") ? parseInt(lower.slice(2), 16) : lower.startsWith("#") ? parseInt(lower.slice(1), 10) : NaN;
    return Number.isFinite(code) && code > 31 && code < 0x110000 ? String.fromCodePoint(code) : whole;
  });
}

/** Raw HTML reduced to Markdown-ish text: no tags survive. */
export function stripHtml(text: string): string {
  return (
    text
      .replace(/<!--[\s\S]*?-->/g, "")
      .replace(/<(script|style|iframe|object|embed|svg|template)\b[\s\S]*?<\/\1\s*>/gi, "")
      .replace(/<(script|style|iframe|object|embed|svg|template)\b[^>]*\/?>/gi, "")
      // <https://…> autolinks first, before they look like tags.
      .replace(/<(https?:\/\/[^\s<>]+)>/gi, "[$1]($1)")
      .replace(/<a\b[^>]*?\bhref\s*=\s*["']([^"']*)["'][^>]*>([\s\S]*?)<\/a\s*>/gi, (_, href: string, inner: string) => {
        const text = stripHtml(inner).trim();
        return text ? `[${text}](${href})` : "";
      })
      .replace(/<img\b[^>]*>/gi, "")
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<\/?(strong|b)\s*>/gi, "**")
      .replace(/<\/?(em|i)\s*>/gi, "*")
      .replace(/<\/?code\s*>/gi, "`")
      .replace(/<\/(p|div|h[1-6]|details|summary|li|tr|table)\s*>/gi, "\n")
      .replace(/<\/?[a-zA-Z][^>]*>/g, "")
  );
}

// ─── Inline ────────────────────────────────────────────────────────────────

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ t: "text", v: decode(plain) });
    plain = "";
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    let m: RegExpMatchArray | null;
    if (rest[0] === "\\" && /^\\[\\`*_{}[\]()#+\-.!|~<>]/.test(rest)) {
      plain += rest[1];
      i += 2;
    } else if (rest[0] === "\n") {
      flush();
      out.push({ t: "br" });
      i += 1;
    } else if ((m = rest.match(/^(`+)([\s\S]*?[^`])\1(?!`)/))) {
      flush();
      out.push({ t: "code", v: m[2].trim() || m[2] });
      i += m[0].length;
    } else if ((m = rest.match(/^!\[([^\]]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)/))) {
      // Pictures are left out: only their place is skipped.
      i += m[0].length;
    } else if ((m = rest.match(/^\[((?:[^[\]]|\[[^\]]*\])+)\]\(\s*<?((?:[^()\s<>]|\([^()\s]*\))*)>?(?:\s+"[^"]*")?\s*\)/))) {
      flush();
      const href = safeHref(m[2]);
      const inner = parseInline(m[1]);
      if (href) out.push({ t: "link", href, c: inner });
      else out.push(...inner);
      i += m[0].length;
    } else if ((m = rest.match(/^(\*\*|__)(?=\S)([\s\S]*?\S)\1/))) {
      flush();
      out.push({ t: "strong", c: parseInline(m[2]) });
      i += m[0].length;
    } else if ((m = rest.match(/^~~(?=\S)([\s\S]*?\S)~~/))) {
      plain += m[1];
      i += m[0].length;
    } else if ((m = rest.match(/^\*(?=[^\s*])([\s\S]*?[^\s*])\*(?!\*)/))) {
      flush();
      out.push({ t: "em", c: parseInline(m[1]) });
      i += m[0].length;
    } else if ((m = rest.match(/^_(?=\S)([^_]*?\S)_(?![A-Za-z0-9])/)) && !/[A-Za-z0-9]$/.test(plain)) {
      flush();
      out.push({ t: "em", c: parseInline(m[1]) });
      i += m[0].length;
    } else if ((m = rest.match(/^https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"]/i)) && !/[A-Za-z0-9]$/.test(plain)) {
      flush();
      out.push({ t: "link", href: m[0], c: [{ t: "text", v: m[0] }] });
      i += m[0].length;
    } else {
      plain += rest[0];
      i += 1;
    }
  }
  flush();
  return out;
}

// ─── Blocks ────────────────────────────────────────────────────────────────

const FENCE = /^\s{0,3}(```+|~~~+)\s*([\w+-]*)\s*$/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const QUOTE = /^\s{0,3}>\s?/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const TABLE_RULE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

function cells(line: string): string[] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  return row.split(/(?<!\\)\|/).map((cell) => cell.trim().replace(/\\\|/g, "|"));
}

function startsBlock(line: string, next: string | undefined): boolean {
  return FENCE.test(line) || HEADING.test(line) || RULE.test(line) || QUOTE.test(line) || ITEM.test(line) || (line.includes("|") && next !== undefined && TABLE_RULE.test(next));
}

function indentOf(line: string): number {
  return (line.match(/^\s*/)?.[0] ?? "").replace(/\t/g, "    ").length;
}

function parseList(lines: string[], start: number): { block: Block; end: number } {
  const first = lines[start].match(ITEM)!;
  const base = indentOf(lines[start]);
  const ordered = /\d/.test(first[2]);
  const items: ListItem[] = [];
  let i = start;
  while (i < lines.length) {
    const line = lines[i];
    const m = line.match(ITEM);
    if (!m || indentOf(line) !== base || /\d/.test(m[2]) !== ordered) break;
    const text = [m[3]];
    const nested: string[] = [];
    i++;
    while (i < lines.length) {
      const next = lines[i];
      if (!next.trim()) {
        // A blank line ends the list unless the item goes on after it.
        const after = lines[i + 1];
        if (after !== undefined && after.trim() && indentOf(after) > base) {
          i++;
          continue;
        }
        break;
      }
      if (indentOf(next) > base && (ITEM.test(next) || nested.length)) {
        nested.push(next);
      } else if (indentOf(next) > base || (!startsBlock(next, lines[i + 1]) && !nested.length)) {
        text.push(next.trim());
      } else {
        break;
      }
      i++;
    }
    const sub = nested.length ? parseBlocks(dedent(nested)) : [];
    items.push({ c: parseInline(text.join("\n")), sub });
  }
  const block: Block = ordered ? { t: "ol", start: parseInt(first[2], 10) || 1, items } : { t: "ul", items };
  return { block, end: i };
}

function dedent(lines: string[]): string[] {
  const least = Math.min(...lines.filter((l) => l.trim()).map(indentOf));
  return lines.map((l) => l.replace(/\t/g, "    ").slice(Math.min(least, indentOf(l))));
}

function parseBlocks(lines: string[]): Block[] {
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    let m: RegExpMatchArray | null;
    if (!line.trim()) {
      i++;
    } else if ((m = line.match(FENCE))) {
      const close = m[1][0];
      const body: string[] = [];
      i++;
      while (i < lines.length && !new RegExp(`^\\s{0,3}${close === "`" ? "`" : "~"}{${m[1].length},}\\s*$`).test(lines[i])) body.push(lines[i++]);
      i++;
      blocks.push({ t: "code", v: body.join("\n") });
    } else if ((m = line.match(HEADING))) {
      blocks.push({ t: "h", level: m[1].length, c: parseInline(m[2]) });
      i++;
    } else if (RULE.test(line)) {
      blocks.push({ t: "hr" });
      i++;
    } else if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && lines[i].trim() && (QUOTE.test(lines[i]) || !startsBlock(lines[i], lines[i + 1]))) body.push(lines[i++].replace(QUOTE, ""));
      blocks.push({ t: "quote", c: parseBlocks(body) });
    } else if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1])) {
      const head = cells(line).map(parseInline);
      const rows: Inline[][][] = [];
      i += 2;
      while (i < lines.length && lines[i].trim() && lines[i].includes("|")) rows.push(cells(lines[i++]).map(parseInline));
      blocks.push({ t: "table", head, rows });
    } else if (ITEM.test(line)) {
      const { block, end } = parseList(lines, i);
      blocks.push(block);
      i = end;
    } else {
      const body: string[] = [];
      while (i < lines.length && lines[i].trim() && (body.length === 0 || !startsBlock(lines[i], lines[i + 1]))) body.push(lines[i++].trim());
      const c = parseInline(body.join("\n"));
      if (c.some((part) => part.t !== "br" && (part.t !== "text" || part.v.trim()))) blocks.push({ t: "p", c });
    }
  }
  return blocks;
}

/** Release notes as blocks, safe to render as text. */
export function parseMarkdown(markdown: string): Block[] {
  // HTML is stripped outside code blocks; inside them it is shown as text.
  const lines: string[] = [];
  let prose: string[] = [];
  let fence: string | null = null;
  const flush = () => {
    if (prose.length) lines.push(...stripHtml(prose.join("\n")).split("\n"));
    prose = [];
  };
  for (const line of markdown.replace(/\r\n?/g, "\n").split("\n")) {
    const m = line.match(FENCE);
    if (fence === null && m) {
      flush();
      fence = m[1][0];
      lines.push(line);
    } else if (fence !== null) {
      lines.push(line);
      if (m && m[1][0] === fence && !m[2]) fence = null;
    } else {
      prose.push(line);
    }
  }
  flush();
  return parseBlocks(lines);
}
