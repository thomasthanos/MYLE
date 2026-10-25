export interface ScanProgress {
  percent: number;
  /** The surrounding words identify the phase, including localized SFC text. */
  signature: string;
  bar: boolean;
}

export function scanProgress(text: string): ScanProgress | null {
  const match = /^([^%]*?)(\d{1,3}(?:[.,]\d+)?)\s*%([^%]*)$/.exec(text.trim());
  if (!match || /[\d.,+-]$/.test(match[1])) return null;
  const percent = Number(match[2].replace(",", "."));
  if (percent > 100) return null;
  const bar = /^\s*\[[=\s]*$/.test(match[1]) && /^[=\s]*\]\s*$/.test(match[3]);
  return { percent, signature: bar ? "dism-bar" : `${match[1]}%${match[3]}`, bar };
}

/** CR replacements and newline progress both update the same visible row. */
export function appendOutput(lines: string[], line: { text: string; replace: boolean }) {
  if (line.replace && lines.length) lines[lines.length - 1] = line.text;
  else lines.push(line.text);

  const current = scanProgress(line.text);
  const previous = lines.length > 1 ? scanProgress(lines[lines.length - 2]) : null;
  // Check after replacing too: a read can split the new percentage in half.
  if (current && previous?.signature === current.signature) lines.splice(lines.length - 2, 1);
}

export function outputText(lines: string[]): string {
  return lines.map((line) => {
    const progress = scanProgress(line);
    return progress?.bar ? `Progress: ${progress.percent}%` : line;
  }).join("\n");
}
