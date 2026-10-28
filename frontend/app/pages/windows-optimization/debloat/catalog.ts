import type { AppGroup, Category } from "./api";
export interface ChoiceOption { value: string; label: string }
export const tweakCategories: readonly { id: Category; title: string }[] = [
  { id: "privacy", title: "Privacy" }, { id: "system", title: "System" },
  { id: "taskbar", title: "Taskbar & Start" }, { id: "explorer", title: "File Explorer" },
  { id: "ai", title: "AI" }, { id: "apps", title: "Apps" },
];
export const appCategories: readonly { id: AppGroup; title: string }[] = [
  { id: "microsoft", title: "Microsoft" }, { id: "bing", title: "Bing" },
  { id: "xbox", title: "Xbox" }, { id: "thirdParty", title: "Other apps" },
];
export const selectionPresets: readonly ChoiceOption[] = [
  { value: "recommended", label: "Select recommended" },
  { value: "all", label: "Select all" }, { value: "none", label: "Clear selection" },
];
export const startPresets: readonly ChoiceOption[] = [
  { value: "essential", label: "Essential" }, { value: "all", label: "All" }, { value: "none", label: "None" },
];
interface FolderOption { id: string; label: string; essential?: boolean }
interface PinOption extends FolderOption { sub: string }
export const folderCatalog: readonly FolderOption[] = [
  { id: "settings", label: "Settings", essential: true }, { id: "explorer", label: "File Explorer", essential: true },
  { id: "downloads", label: "Downloads", essential: true }, { id: "documents", label: "Documents" },
  { id: "userProfile", label: "Personal folder" }, { id: "pictures", label: "Pictures" },
  { id: "music", label: "Music" }, { id: "videos", label: "Videos" }, { id: "network", label: "Network" },
];
export const pinCatalog: readonly PinOption[] = [
  { id: "explorer", label: "File Explorer", sub: "System", essential: true },
  { id: "settings", label: "Settings", sub: "System", essential: true },
  { id: "store", label: "Microsoft Store", sub: "Store", essential: true },
  { id: "terminal", label: "Terminal", sub: "Developer", essential: true },
  { id: "calculator", label: "Calculator", sub: "Utility", essential: true },
  { id: "notepad", label: "Notepad", sub: "Editor", essential: true },
  { id: "snipping-tool", label: "Snipping Tool", sub: "Capture", essential: true },
  { id: "photos", label: "Photos", sub: "Media" }, { id: "paint", label: "Paint", sub: "Graphics" },
  { id: "clock", label: "Clock", sub: "Utility" }, { id: "edge", label: "Microsoft Edge", sub: "Browser" },
  { id: "xbox", label: "Xbox", sub: "Gaming" },
];
