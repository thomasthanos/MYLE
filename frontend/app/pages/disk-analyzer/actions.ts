// What the right-click menu offers for an item, wherever it was clicked: the folder list, the largest files or the treemap.
import Copy from "@lucide/svelte/icons/copy";
import FolderOpen from "@lucide/svelte/icons/folder-open";
import ListTree from "@lucide/svelte/icons/list-tree";
import Trash2 from "@lucide/svelte/icons/trash-2";
import ZoomIn from "@lucide/svelte/icons/zoom-in";
import type { PageAction } from "../../../lib/context-menu";
import { SMALLER_ID } from "./api";
import { diskState as d } from "./state.svelte";
export interface ItemInfo {
  kind?: string;
  /** Known to be protected (the backend refuses it anyway). */
  locked?: boolean;
}
/** The menu for `id`. A right-click outside the selection selects it alone. */
export function itemActions(id: number, info: ItemInfo, from: "tree" | "files" | "map"): PageAction[] {
  if (id === SMALLER_ID || d.scanId === null) return [];
  if (!d.selection.has(id)) d.select(id);
  const many = d.selection.size > 1;
  const actions: PageAction[] = [
    { id: "reveal", label: "Open in Explorer", icon: FolderOpen, run: () => d.reveal(id) },
    { id: "copy-path", label: "Copy path", icon: Copy, run: () => d.copyPath(id) },
  ];
  if (info.kind === "dir") actions.push({ id: "zoom", label: "Show in treemap", icon: ZoomIn, run: () => d.zoomTo(id) });
  if (from !== "tree") {
    actions.push({
      id: "show",
      label: "Show in folder list",
      icon: ListTree,
      run: () => {
        d.setView("folders");
        void d.showInTree(id);
      },
    });
  }
  actions.push({
    id: "recycle",
    label: many ? `Move ${d.selection.size} items to Recycle Bin` : "Move to Recycle Bin",
    icon: Trash2,
    shortcut: "Del",
    disabled: d.busy || (!many && info.locked === true),
    run: () => d.recycle(),
  });
  return actions;
}
