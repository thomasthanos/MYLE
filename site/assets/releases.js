const repository = "thomasthanos/MYLE";
const github = `https://github.com/${repository}`;
const releasesUrl = `https://api.github.com/repos/${repository}/releases?per_page=20`;
const status = document.getElementById("release-status");
const list = document.getElementById("releases");
const currentVersion = document.getElementById("current-version");
const imageHosts = new Set([
  "raw.githubusercontent.com", "user-images.githubusercontent.com", "private-user-images.githubusercontent.com",
  "github.com", "github.githubassets.com", "objects.githubusercontent.com", "img.shields.io",
]);
const notesMarkup = {
  RETURN_DOM_FRAGMENT: true,
  ALLOW_ARIA_ATTR: false,
  ALLOW_DATA_ATTR: false,
  ALLOWED_TAGS: [
    "a", "b", "blockquote", "br", "code", "dd", "del", "details", "div", "dl", "dt", "em", "figcaption",
    "figure", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "input", "kbd", "li", "ol", "p", "pre",
    "s", "small", "span", "strong", "sub", "summary", "sup", "table", "tbody", "td", "th", "thead", "tr", "ul",
  ],
  ALLOWED_ATTR: [
    "align", "alt", "checked", "colspan", "disabled", "height", "href", "open", "rowspan", "scope",
    "src", "start", "title", "type", "width",
  ],
};
const calloutNames = { NOTE: "Note", TIP: "Tip", IMPORTANT: "Important", WARNING: "Warning", CAUTION: "Caution" };

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function webUrl(value, base) {
  if (!value?.trim()) return null;
  try {
    const url = new URL(value, base);
    return url.protocol === "https:" && !url.username && !url.password ? url : null;
  } catch {
    return null;
  }
}

function externalLink(link, url) {
  link.href = url;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
}

function renderNotes(notes, release, sourceUrl) {
  const body = release.body?.trim();
  if (!body) {
    notes.append(element("p", "", "No additional release notes were published."));
    return;
  }
  if (!window.marked || !window.DOMPurify?.isSupported) {
    notes.classList.add("plain");
    notes.textContent = body;
    return;
  }

  const content = window.DOMPurify.sanitize(window.marked.parse(body, { gfm: true }), notesMarkup);
  for (const link of content.querySelectorAll("a[href]")) {
    const url = webUrl(link.getAttribute("href"), sourceUrl);
    if (url) externalLink(link, url.href);
    else link.removeAttribute("href");
  }
  const imageBase = `https://raw.githubusercontent.com/${repository}/${encodeURIComponent(release.tag_name)}/`;
  for (const image of content.querySelectorAll("img")) {
    const url = webUrl(image.getAttribute("src"), imageBase);
    if (!url || !imageHosts.has(url.hostname)) {
      image.replaceWith(document.createTextNode(image.alt));
      continue;
    }
    image.loading = "lazy";
    image.decoding = "async";
    image.referrerPolicy = "no-referrer";
    image.src = url.href;
  }
  for (const item of content.querySelectorAll("input")) {
    if (item.type === "checkbox") item.disabled = true;
    else item.remove();
  }
  for (const table of content.querySelectorAll("table")) {
    const scroller = element("div", "release-table");
    table.replaceWith(scroller);
    scroller.append(table);
  }
  for (const quote of content.querySelectorAll("blockquote")) {
    const text = quote.firstElementChild?.firstChild;
    if (!(text instanceof Text)) continue;
    const marker = text.data.match(/^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*(?:\n|$)/);
    if (!marker) continue;
    text.deleteData(0, marker[0].length);
    quote.className = "release-callout";
    quote.dataset.kind = marker[1].toLowerCase();
    quote.prepend(element("strong", "callout-label", calloutNames[marker[1]]));
  }
  notes.append(content);
}

function releaseCard(release, index) {
  const article = element("article", index === 0 ? "release-card latest" : "release-card");
  const head = element("div", "release-head");
  const titleWrap = element("div");
  const title = element("h2", "", release.name || release.tag_name);
  title.id = `release-title-${index}`;
  const version = element("span", index === 0 ? "pill" : "pill neutral", release.tag_name);
  const date = element(
    "time",
    "release-date",
    new Intl.DateTimeFormat(undefined, { dateStyle: "long" }).format(new Date(release.published_at)),
  );
  date.dateTime = release.published_at;

  titleWrap.append(version, title);
  head.append(titleWrap, date);
  article.append(head);

  const linkFromApi = webUrl(release.html_url, github);
  const sourceUrl = linkFromApi?.origin === "https://github.com"
    ? linkFromApi.href
    : `${github}/releases/tag/${encodeURIComponent(release.tag_name)}`;
  const notes = element("div", "release-notes");
  notes.tabIndex = 0;
  notes.setAttribute("role", "region");
  notes.setAttribute("aria-labelledby", title.id);
  renderNotes(notes, release, sourceUrl);
  article.append(notes);

  const footer = element("footer", "release-footer");
  const link = element("a", "button small release-link", "View on GitHub ↗");
  externalLink(link, sourceUrl);
  footer.append(link);
  article.append(footer);
  return article;
}

async function loadReleases() {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 10_000);
  try {
    const response = await fetch(releasesUrl, {
      headers: { Accept: "application/vnd.github+json" },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`GitHub returned ${response.status}`);

    const data = await response.json();
    const stable = Array.isArray(data)
      // The Windows app's releases (v1.2.3): a MYLE Passwords one (mobile-v…) is never "the current release".
      ? data.filter((release) => /^v\d/.test(release.tag_name) && !release.draft && !release.prerelease && release.published_at).slice(0, 8)
      : [];
    if (!stable.length) throw new Error("No stable releases were returned");

    const cards = document.createDocumentFragment();
    for (const [index, release] of stable.entries()) cards.append(releaseCard(release, index));
    list.replaceChildren(cards);
    currentVersion.textContent = stable[0].tag_name;
    status.remove();
  } catch (error) {
    currentVersion.textContent = "Unavailable";
    status.className = "error-card";
    status.textContent = "Release history could not be loaded right now. ";
    const link = element("a", "inline-link", "Open GitHub Releases instead.");
    link.href = `https://github.com/${repository}/releases`;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    status.append(link);
  } finally {
    window.clearTimeout(timeout);
  }
}

void loadReleases();
