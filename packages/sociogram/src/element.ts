// Web Component: `<sociogram-view>`. Works in any framework, including Angular
// (add `CUSTOM_ELEMENTS_SCHEMA`) and Svelte, and in plain HTML.
//
//   import { defineSociogramElement } from "sociogram/element";
//   defineSociogramElement();                       // call once, in the browser
//
//   const el = document.querySelector("sociogram-view");
//   el.data = { nodeCount: 2, edges: [0, 1] };      // properties, not attributes
//   el.addEventListener("nodeclick", (e) => console.log(e.detail));
//
// Events (all `CustomEvent`): `ready` (detail: the view), `nodehover` (detail:
// index or null), `nodeclick` (detail: index), `error` (detail: `{ error, phase }`).
//
// The class is created inside `defineSociogramElement` because `HTMLElement` does
// not exist on the server: importing this module is safe during server rendering.

import { mountSociogram } from "./mount.js";
import type { Mounted, MountErrorPhase } from "./mount.js";
import type { GraphData, LinkFadeMode, SociogramOptions, SociogramView } from "./view.js";

export interface SociogramErrorDetail {
  error: Error;
  phase: MountErrorPhase;
}

/** The events the element dispatches, with the type of each event's `detail`. */
export interface SociogramElementEventMap {
  /** The view is ready. */
  ready: CustomEvent<SociogramView>;
  /** The node under the pointer changed (`null` = none). */
  nodehover: CustomEvent<number | null>;
  /** A node was clicked (not dragged). */
  nodeclick: CustomEvent<number>;
  /** Creating the view failed, data was rejected, or rendering stopped. `phase` tells which. */
  error: CustomEvent<SociogramErrorDetail>;
}

type Options = boolean | AddEventListenerOptions;

/** The element's public surface. */
export interface SociogramElement extends HTMLElement {
  /** The graph. Assign a new object to replace it. */
  data: GraphData | undefined;
  linkFade: LinkFadeMode | undefined;
  /** Create-time options. Set before the element is connected, or reconnect it. */
  options: SociogramOptions | undefined;
  /** The underlying view once ready, otherwise `undefined`. */
  readonly view: SociogramView | undefined;
  fitView(): void;
  reheat(): void;
  /** Rejects when the view is not ready. */
  screenshot(): Promise<Blob>;

  // Typed listeners. `error` would otherwise be typed as the DOM's `ErrorEvent`,
  // which this element never dispatches.
  addEventListener<K extends keyof SociogramElementEventMap>(
    type: K,
    listener: (this: SociogramElement, event: SociogramElementEventMap[K]) => unknown,
    options?: Options,
  ): void;
  // The DOM's own overloads must stay, or `click` and friends lose their types.
  addEventListener<K extends keyof HTMLElementEventMap>(
    type: K,
    listener: (this: HTMLElement, event: HTMLElementEventMap[K]) => unknown,
    options?: Options,
  ): void;
  addEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: Options): void;
  removeEventListener<K extends keyof SociogramElementEventMap>(
    type: K,
    listener: (this: SociogramElement, event: SociogramElementEventMap[K]) => unknown,
    options?: boolean | EventListenerOptions,
  ): void;
  removeEventListener<K extends keyof HTMLElementEventMap>(
    type: K,
    listener: (this: HTMLElement, event: HTMLElementEventMap[K]) => unknown,
    options?: boolean | EventListenerOptions,
  ): void;
  removeEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: boolean | EventListenerOptions): void;
}

declare global {
  interface HTMLElementTagNameMap {
    /** Registered by `defineSociogramElement()` under its default tag name. */
    "sociogram-view": SociogramElement;
  }
}

const UPGRADE_PROPERTIES = ["data", "linkFade", "options"] as const;

/**
 * Register the element (once; later calls return the existing class). Call it
 * in the browser only.
 */
export function defineSociogramElement(tagName = "sociogram-view"): CustomElementConstructor {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") {
    throw new Error("defineSociogramElement must be called in a browser");
  }
  const existing = customElements.get(tagName);
  if (existing !== undefined) return existing;

  class SociogramViewElement extends HTMLElement implements SociogramElement {
    #mounted: Mounted | undefined;
    #host: HTMLDivElement | undefined;
    #data: GraphData | undefined;
    #linkFade: LinkFadeMode | undefined;
    #options: SociogramOptions | undefined;

    get data(): GraphData | undefined {
      return this.#data;
    }
    set data(value: GraphData | undefined) {
      this.#data = value;
      this.#mounted?.update({ data: value });
    }

    get linkFade(): LinkFadeMode | undefined {
      return this.#linkFade;
    }
    set linkFade(value: LinkFadeMode | undefined) {
      this.#linkFade = value;
      this.#mounted?.update({ linkFade: value });
    }

    get options(): SociogramOptions | undefined {
      return this.#options;
    }
    set options(value: SociogramOptions | undefined) {
      this.#options = value;
    }

    get view(): SociogramView | undefined {
      return this.#mounted?.view;
    }

    fitView(): void {
      this.#mounted?.view?.fitView();
    }
    reheat(): void {
      this.#mounted?.view?.reheat();
    }
    screenshot(): Promise<Blob> {
      const view = this.#mounted?.view;
      return view === undefined ? Promise.reject(new Error("Sociogram is not ready yet")) : view.screenshot();
    }

    connectedCallback(): void {
      this.#upgradeProperties();
      if (this.#mounted !== undefined) return;
      const root = this.shadowRoot ?? this.attachShadow({ mode: "open" });
      if (this.#host === undefined) {
        const style = this.ownerDocument.createElement("style");
        style.textContent = ":host{display:block;position:relative;width:100%;height:100%}div{position:absolute;inset:0}";
        const host = this.ownerDocument.createElement("div");
        root.append(style, host);
        this.#host = host;
      }
      this.#mounted = mountSociogram(this.#host, {
        options: this.#options,
        initial: { data: this.#data, linkFade: this.#linkFade },
        callbacks: () => ({
          onReady: (view) => this.#emit("ready", view),
          onNodeHover: (index) => this.#emit("nodehover", index),
          onNodeClick: (index) => this.#emit("nodeclick", index),
          onError: (error, phase) => this.#emit<SociogramErrorDetail>("error", { error, phase }),
        }),
      });
    }

    disconnectedCallback(): void {
      this.#mounted?.destroy();
      this.#mounted = undefined;
    }

    #emit<T>(type: string, detail: T): void {
      this.dispatchEvent(new CustomEvent<T>(type, { detail }));
    }

    // A framework may assign a property before the element class is registered;
    // that value then shadows the accessor. Re-apply it through the accessor.
    #upgradeProperties(): void {
      for (const name of UPGRADE_PROPERTIES) {
        if (Object.prototype.hasOwnProperty.call(this, name)) {
          const value = (this as unknown as Record<string, unknown>)[name];
          delete (this as unknown as Record<string, unknown>)[name];
          (this as unknown as Record<string, unknown>)[name] = value;
        }
      }
    }
  }

  customElements.define(tagName, SociogramViewElement);
  return SociogramViewElement;
}
