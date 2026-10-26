// File-type colours, shared by the treemap, the File types tab and the
// largest files: one calm colour per kind of file, not one per extension, so
// the map reads at a glance ("that block is video") instead of as confetti.
// Each theme has its own set: muted mid-tones that sit on the default black
// and on the navy layers, with white text on top.

export type Category = "video" | "image" | "audio" | "archive" | "app" | "document" | "code" | "system" | "other";

export const CATEGORIES: readonly { id: Category; label: string }[] = [
  { id: "video", label: "Video" },
  { id: "image", label: "Images" },
  { id: "audio", label: "Audio" },
  { id: "archive", label: "Archives" },
  { id: "app", label: "Apps & games" },
  { id: "document", label: "Documents" },
  { id: "code", label: "Code & data" },
  { id: "system", label: "System" },
  { id: "other", label: "Other" },
];

const DEFAULT_COLORS: Record<Category, string> = {
  video: "#7b64b9",
  image: "#418b7e",
  audio: "#b65d84",
  archive: "#b17a43",
  app: "#4e7dbc",
  document: "#688c4f",
  code: "#9d8843",
  system: "#5c6370",
  other: "#464a53",
};

const NAVY_COLORS: Record<Category, string> = {
  video: "#8174be",
  image: "#50958e",
  audio: "#b67294",
  archive: "#b58a5a",
  app: "#6788c1",
  document: "#6f925d",
  code: "#a59255",
  system: "#636c83",
  other: "#4c5361",
};

const EXTENSIONS: Record<Exclude<Category, "other">, string> = {
  video: "mp4 mkv avi mov wmv flv webm m4v mpg mpeg m2ts mts ts vob 3gp ogv rmvb bik bk2 usm",
  image: "jpg jpeg png gif bmp tif tiff webp heic heif avif jxl raw cr2 cr3 nef arw dng orf psd psb xcf svg ico exr hdr tga dds kra",
  audio: "mp3 wav flac aac ogg oga m4a wma opus aiff aif alac ape mid midi wv",
  archive: "zip rar 7z tar gz tgz bz2 xz zst lz4 lzma cab iso img vhd vhdx vmdk vdi qcow2 wim esd dmg",
  app: "exe dll msi msix msixbundle appx appxbundle com ocx so dylib jar apk xapk node pak vpk bsa ba2 uasset ubulk utoc ucas bundle assets resource forge wad bnk pck rpf acf archive",
  document: "pdf doc docx docm xls xlsx xlsm ppt pptx odt ods odp rtf txt md epub mobi csv tsv tex one pub xps",
  code: "js mjs cjs ts tsx jsx json xml html htm css scss py pyc rs c cc cpp h hpp cs java class go rb php lua sql yaml yml toml ini cfg conf db sqlite sqlite3 ldb wasm map ipynb sh ps1 bat cmd o obj a lib pdb ilk ipch",
  system: "sys log tmp temp etl evtx dmp mdmp mui cat manifest mum pf blf regtrans-ms edb jrs chk bak old cache msp cdf-ms wer lock",
};

const BY_EXTENSION = new Map<string, Category>();
for (const [category, list] of Object.entries(EXTENSIONS)) {
  for (const ext of list.split(" ")) BY_EXTENSION.set(ext, category as Category);
}

/** The kind of file an extension (without the dot, any case) names. */
export function categoryOf(ext: string): Category {
  return BY_EXTENSION.get(ext.toLowerCase()) ?? "other";
}

/** The extension of a file name, lowercase and without the dot ("" for none). */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export function categoryColor(category: Category, navy: boolean): string {
  return (navy ? NAVY_COLORS : DEFAULT_COLORS)[category];
}

export function categoryLabel(category: Category): string {
  return CATEGORIES.find((c) => c.id === category)?.label ?? "Other";
}
