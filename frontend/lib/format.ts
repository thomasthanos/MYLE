// Sizes and times as the pages show them.
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const decimals = unit === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(decimals)} ${units[unit]}`;
}
function toDate(value: number | string): Date {
  return new Date(typeof value === "number" && value < 10_000_000_000 ? value * 1000 : value);
}
const dateTimeFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const dayFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
export function formatDate(value: number | string | null): string {
  if (value === null) return "Never";
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  return dateTimeFormat.format(date);
}
/** "5 minutes ago", "yesterday", "3 days ago"; the day itself after a month. */
export function formatRelative(value: number | string | null): string {
  if (value === null) return "Never";
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  const seconds = (Date.now() - date.getTime()) / 1000;
  if (seconds < 0 || seconds >= 30 * 86_400) {
    return dayFormat.format(date);
  }
  if (seconds < 60) return "Just now";
  const relative = relativeFormat;
  const text =
    seconds < 3_600
      ? relative.format(-Math.round(seconds / 60), "minute")
      : seconds < 86_400
        ? relative.format(-Math.round(seconds / 3_600), "hour")
        : relative.format(-Math.round(seconds / 86_400), "day");
  return text.charAt(0).toUpperCase() + text.slice(1);
}
