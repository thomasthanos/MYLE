import assert from "node:assert/strict";
import test from "node:test";
import { parseInline, parseMarkdown, safeHref, stripHtml } from "../../frontend/lib/markdown.ts";

const text = (inlines) => inlines.map((i) => (i.t === "text" || i.t === "code" ? i.v : i.t === "br" ? "\n" : text(i.c ?? []))).join("");

test("only web addresses become links", () => {
  assert.equal(safeHref("https://github.com/x"), "https://github.com/x");
  assert.equal(safeHref("javascript:alert(1)"), null);
  assert.equal(safeHref("JAVASCRIPT:alert(1)"), null);
  assert.equal(safeHref("data:text/html,<script>"), null);
  assert.equal(safeHref("file:///C:/Windows"), null);
  const bad = parseInline("[click](javascript:alert(1)) and [ok](https://a.b/c)");
  assert.ok(!bad.some((i) => i.t === "link" && i.href.startsWith("javascript")));
  assert.deepEqual(bad.filter((i) => i.t === "link").map((i) => i.href), ["https://a.b/c"]);
  assert.equal(text(bad), "click and ok");
  assert.equal(parseInline("[wiki](https://en.wikipedia.org/wiki/Rust_(language))")[0].href, "https://en.wikipedia.org/wiki/Rust_(language)");
});

test("raw HTML never survives, its text does", () => {
  const blocks = parseMarkdown(
    '<p align="center"><img src="https://x/hero.svg" alt="hero" /></p>\n\n<p align="center"><strong>Big line.</strong><br />Second.</p>\n\n<script>alert(1)</script>\n<img src=x onerror="alert(1)">\n<a href="https://dl"><img src="badge.svg"></a>\n\n<iframe src="https://evil"></iframe>Text after',
  );
  const json = JSON.stringify(blocks);
  assert.ok(!/<|onerror|alert|iframe|hero\.svg|badge/.test(json), json);
  assert.ok(json.includes("Big line."));
  assert.ok(json.includes("Text after"));
  assert.equal(stripHtml('<a href="https://x">go</a>'), "[go](https://x)");
});

test("release notes: headings, tables, lists and code", () => {
  const md = [
    "## ✨ Highlights",
    "",
    "| | What's new |",
    "|---|---|",
    "| 🔨 **Build** | The steps, `npm run build`. |",
    "",
    "- **One** thing",
    "  - nested *a*",
    "- Two with [link](https://github.com/thomasthanos/MYLE)",
    "",
    "1. first",
    "2. second",
    "",
    "```",
    "<b>not html</b>",
    "```",
    "",
    "> quoted",
    "",
    "---",
    "Plain snake_case_name and https://downloads.thomast.uk/MYLE.exe.",
  ].join("\r\n");
  const blocks = parseMarkdown(md);
  assert.deepEqual(blocks.map((b) => b.t), ["h", "table", "ul", "ol", "code", "quote", "hr", "p"]);
  assert.equal(blocks[0].level, 2);
  assert.equal(text(blocks[1].rows[0][1]), "The steps, npm run build.");
  assert.equal(blocks[2].items.length, 2);
  assert.equal(blocks[2].items[0].sub[0].t, "ul");
  assert.equal(blocks[2].items[1].c.find((i) => i.t === "link").href, "https://github.com/thomasthanos/MYLE");
  assert.equal(blocks[3].items.length, 2);
  assert.equal(blocks[4].v, "<b>not html</b>");
  const last = blocks[7].c;
  assert.ok(last.some((i) => i.t === "text" && i.v.includes("snake_case_name")));
  assert.equal(last.find((i) => i.t === "link").href, "https://downloads.thomast.uk/MYLE.exe");
});

test("the 9.8.0 notes read cleanly", async () => {
  const { readFile } = await import("node:fs/promises");
  const md = await readFile(new URL("../../docs/release-notes/9.8.0.md", import.meta.url), "utf8");
  const blocks = parseMarkdown(md);
  assert.equal(blocks[0].t, "p");
  assert.ok(text(blocks[0].c).startsWith("GitHub Releases that build everything"));
  assert.ok(blocks.some((b) => b.t === "table"));
  assert.ok(!JSON.stringify(blocks).includes("<"));
});

test("the 9.9.0 notes, the first the What's new window shows, read cleanly", async () => {
  const { readFile } = await import("node:fs/promises");
  const md = await readFile(new URL("../../docs/release-notes/9.9.0.md", import.meta.url), "utf8");
  const blocks = parseMarkdown(md);
  assert.ok(text(blocks[0].c).startsWith("See what changed"));
  assert.ok(blocks.some((b) => b.t === "table"));
  assert.ok(blocks.some((b) => b.t === "h" && text(b.c).includes("What's new window")));
  assert.ok(!JSON.stringify(blocks).includes("<"));
});

test("the 9.10.0 notes read cleanly", async () => {
  const { readFile } = await import("node:fs/promises");
  const md = await readFile(new URL("../../docs/release-notes/9.10.0.md", import.meta.url), "utf8");
  const blocks = parseMarkdown(md);
  assert.ok(text(blocks[0].c).startsWith("Updates without the blank moment"));
  assert.ok(blocks.some((b) => b.t === "table"));
  assert.ok(blocks.some((b) => b.t === "h" && text(b.c).includes("Sidebar")));
  assert.ok(!JSON.stringify(blocks).includes("<"));
});
