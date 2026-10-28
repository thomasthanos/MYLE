import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, normalize, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const siteRoot = join(root, "site");
const required = [
  "index.html",
  "changelog.html",
  "readme.html",
  "copyright.html",
  "_headers",
  "_redirects",
  "assets/site.css",
  "assets/releases.js",
  "assets/app-icon.svg",
];
const htmlFiles = required.filter((file) => file.endsWith(".html"));
const checkedTextFiles = [
  ...htmlFiles,
  "assets/site.css",
  "assets/releases.js",
];
const expectedInstaller = "https://downloads.thomast.uk/MYLE.exe";
const allowedHosts = new Set([
  "downloads.thomast.uk",
  "github.com",
  "api.github.com",
]);
const failures = [];
const staleClaims = [
  [/\belectron\b/i, "Electron"],
  [/\bportable\b/i, "Portable"],
  [/activation helpers/i, "activation helpers"],
];
function fail(message) {
  failures.push(message);
}
for (const file of required) {
  const full = join(siteRoot, file);
  if (!existsSync(full) || !statSync(full).isFile()) fail(`Missing required site file: ${file}`);
}
for (const file of checkedTextFiles) {
  const full = join(siteRoot, file);
  if (!existsSync(full)) continue;
  const text = readFileSync(full, "utf8");
  for (const [pattern, label] of staleClaims) {
    if (pattern.test(text)) fail(`${file} still contains stale copy: ${label}`);
  }
}
for (const file of htmlFiles) {
  const full = join(siteRoot, file);
  if (!existsSync(full)) continue;
  const html = readFileSync(full, "utf8");
  const refs = [...html.matchAll(/(?:href|src)=["']([^"']+)["']/gi)].map((match) => match[1]);
  for (const ref of refs) {
    if (/^(?:#|mailto:|tel:)/i.test(ref)) continue;
    if (/^https:\/\//i.test(ref)) {
      const host = new URL(ref).hostname.toLowerCase();
      if (!allowedHosts.has(host)) fail(`${file} links to unexpected external host: ${host}`);
      continue;
    }
    if (/^[a-z][a-z0-9+.-]*:/i.test(ref)) {
      fail(`${file} contains unsupported URL scheme: ${ref}`);
      continue;
    }
    const clean = ref.split(/[?#]/, 1)[0];
    if (!clean || clean === "/") continue;
    const candidate = clean.startsWith("/")
      ? join(siteRoot, clean.slice(1))
      : resolve(dirname(full), clean);
    const rel = relative(siteRoot, normalize(candidate));
    if (rel.startsWith(`..${sep}`) || rel === ".." || !existsSync(candidate)) {
      fail(`${file} has a missing or escaping local reference: ${ref}`);
    }
  }
}
for (const file of ["index.html", "readme.html"]) {
  const full = join(siteRoot, file);
  if (existsSync(full) && !readFileSync(full, "utf8").includes(expectedInstaller)) {
    fail(`${file} does not contain the canonical R2 installer URL`);
  }
}
const redirects = existsSync(join(siteRoot, "_redirects"))
  ? readFileSync(join(siteRoot, "_redirects"), "utf8")
  : "";
for (const legacyPath of ["/installer.html", "/installer"]) {
  const escaped = legacyPath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (!new RegExp(`^${escaped}\\s+\\/\\s+301\\s*$`, "m").test(redirects)) {
    fail(`_redirects must permanently redirect ${legacyPath} to /`);
  }
}
const headers = existsSync(join(siteRoot, "_headers"))
  ? readFileSync(join(siteRoot, "_headers"), "utf8")
  : "";
for (const header of [
  "Content-Security-Policy",
  "X-Content-Type-Options",
  "Referrer-Policy",
  "Permissions-Policy",
]) {
  if (!headers.includes(header)) fail(`_headers is missing ${header}`);
}
const wrangler = readFileSync(join(root, "wrangler.toml"), "utf8");
if (!/pages_build_output_dir\s*=\s*"\.\/site"/.test(wrangler)) {
  fail("wrangler.toml must publish ./site");
}
if (failures.length) {
  console.error(`Site validation failed (${failures.length}):`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}
console.log(`Site validation passed: ${required.length} required files and ${htmlFiles.length} HTML pages.`);
