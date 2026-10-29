import type { Component } from "svelte";
import type { PageModule } from "../pages/registry";

/** UI modules are shared by foreground navigation and background preloading. */
export class PageModules {
  loaded = $state.raw<Record<string, Component | undefined>>({});
  errors = $state.raw<Record<string, string | undefined>>({});
  #pending = new Map<string, Promise<void>>();

  load(id: string, module: PageModule): Promise<void> {
    if (this.loaded[id]) return Promise.resolve();
    const pending = this.#pending.get(id);
    if (pending) return pending;
    this.errors = { ...this.errors, [id]: undefined };
    const request = Promise.resolve()
      .then(module)
      .then(({ default: component }) => {
        this.loaded = { ...this.loaded, [id]: component };
        this.errors = { ...this.errors, [id]: undefined };
      }, (error: unknown) => {
        this.errors = { ...this.errors, [id]: error instanceof Error ? error.message : String(error) };
      })
      .finally(() => this.#pending.delete(id));
    this.#pending.set(id, request);
    return request;
  }
}
