const repository = "thomasthanos/MYLE";
const releasesUrl = `https://api.github.com/repos/${repository}/releases?per_page=20`;
const status = document.getElementById("release-status");
const list = document.getElementById("releases");
const currentVersion = document.getElementById("current-version");

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function releaseCard(release, index) {
  const article = element("article", "release-card");
  const head = element("div", "release-head");
  const titleWrap = element("div");
  const title = element("h2", "", release.name || release.tag_name);
  const version = element("span", index === 0 ? "pill" : "pill neutral", release.tag_name);
  const date = element(
    "time",
    "release-date",
    new Intl.DateTimeFormat(undefined, { dateStyle: "long" }).format(new Date(release.published_at)),
  );
  date.dateTime = release.published_at;

  titleWrap.append(title, version);
  head.append(titleWrap, date);
  article.append(head);

  const notes = element(
    "div",
    "release-notes",
    release.body?.trim() || "No additional release notes were published.",
  );
  article.append(notes);

  const link = element("a", "release-link", "View this release on GitHub →");
  link.href = release.html_url;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  article.append(link);
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

    currentVersion.textContent = stable[0].tag_name;
    status.remove();
    for (const [index, release] of stable.entries()) list.append(releaseCard(release, index));
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
