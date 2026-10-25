import type { Component } from "svelte";

/** An action a page offers in the shared right-click menu. */
export interface PageAction {
  id: string;
  label: string;
  icon: Component<{ size?: number }>;
  shortcut?: string;
  disabled?: boolean;
  run: () => void | Promise<void>;
}

/** What a page offers for a right-click at (x, y) on `target`. */
export type ActionSource = (target: HTMLElement, x: number, y: number) => PageAction[];

const sources = new Map<Element, ActionSource>();

/** An attachment (`{@attach contextActions(...)}`): right-clicks inside the
 *  element list its actions first in the shared menu. */
export function contextActions(source: ActionSource) {
  return (node: HTMLElement) => {
    sources.set(node, source);
    return () => {
      if (sources.get(node) === source) sources.delete(node);
    };
  };
}

/** The actions of the nearest element above `target` that offers some. */
export function actionsAt(target: HTMLElement, x: number, y: number): PageAction[] {
  for (let element: Element | null = target; element; element = element.parentElement) {
    const source = sources.get(element);
    if (source) return source(target, x, y);
  }
  return [];
}

export interface TextContext {
  editable: boolean;
  disabled: boolean;
  readOnly: boolean;
  sensitive: boolean;
  noCopy: boolean;
  selectedText: string;
  hasText: boolean;
}

/** The shared menu must respect readonly fields and the vault's own Copy buttons. */
export function textActions(context: TextContext) {
  const copy = !context.disabled && !context.sensitive && !context.noCopy && !!context.selectedText;
  return {
    copy,
    cut: copy && context.editable && !context.readOnly,
    paste: context.editable && !context.disabled && !context.readOnly,
    selectAll: context.editable && !context.disabled && context.hasText,
  };
}

/** Keep the entire menu inside the window, including keyboard-opened menus. */
export function menuPosition(x: number, y: number, width: number, height: number, viewportWidth: number, viewportHeight: number) {
  const edge = 8;
  return {
    left: Math.max(edge, Math.min(x, viewportWidth - width - edge)),
    top: Math.max(edge, Math.min(y, viewportHeight - height - edge)),
  };
}
