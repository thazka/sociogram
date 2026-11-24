// Lifecycle helper shared by the React, Vue and Web Component wrappers.
//
// Frameworks mount and unmount components synchronously, while creating a view
// is asynchronous (it loads WebAssembly and asks the GPU for a device). The
// classic bugs live in that gap: a component that unmounts before `create()`
// resolves leaks a GPU context, and React StrictMode mounts, unmounts and
// mounts again on purpose. This helper owns that state machine once, so the
// wrappers stay thin and the behaviour is tested in one place.
//
// It creates its own canvas inside the container for every mount. A canvas that
// has had a WebGPU context must not be shared between a destroyed view and its
// replacement, and the framework never touches the canvas it did not render.

import type { SelectionTool } from "./pointer.js";
import type {
  GraphData,
  LinkFadeMode,
  NodeDragEvent,
  SelectionChange,
  SociogramOptions,
  SociogramView,
} from "./view.js";

/**
 * Where an error came from. Wrappers show their `fallback` only for `"create"`:
 * the other kinds leave a working view on screen.
 *
 * - `"create"`: the view could not be created (for example `WebGpuUnsupportedError`).
 * - `"data"`: `setData` or `setLinkFade` rejected what it was given; the previous state stays.
 * - `"render"`: the render loop stopped.
 * - `"callback"`: the host's own `onReady` callback threw.
 */
export type MountErrorPhase = "create" | "data" | "render" | "callback";

export interface MountCallbacks {
  /** The view is ready; use it for imperative calls (`fitView`, `screenshot`). */
  onReady?: (view: SociogramView) => void;
  onError?: (error: Error, phase: MountErrorPhase) => void;
  onNodeHover?: (index: number | null) => void;
  onNodeClick?: (index: number) => void;
  onNodeDragStart?: (e: NodeDragEvent) => void;
  onNodeDrag?: (e: NodeDragEvent) => void;
  onNodeDragEnd?: (e: NodeDragEvent) => void;
  onSelectionChange?: (e: SelectionChange) => void;
}

/** What the wrapper wants on screen. Both fields are optional. */
export interface MountProps {
  /** Applied when the reference changes. Pass a new object to replace the graph. */
  data?: GraphData | undefined;
  linkFade?: LinkFadeMode | undefined;
  /** Drag nodes with the pointer. Default `true`. */
  dragNodes?: boolean | undefined;
  /** Indices of pinned nodes. The same array reference is applied once. */
  pinnedNodes?: ArrayLike<number> | undefined;
  selectionTool?: SelectionTool | undefined;
}

/** The parts of the DOM and of `SociogramView` this helper needs, so tests can fake them. */
export interface MountDeps {
  create: (canvas: HTMLCanvasElement, options: SociogramOptions) => Promise<SociogramView>;
  createCanvas: (container: HTMLElement) => HTMLCanvasElement;
}

export interface MountConfig {
  options?: SociogramOptions | undefined;
  /**
   * Read at the moment an event happens, so the latest callbacks are used
   * without remounting when a parent re-renders with new closures.
   */
  callbacks?: () => MountCallbacks;
  initial?: MountProps | undefined;
}

export interface Mounted {
  /** Apply new props. Safe to call before the view is ready: the latest values win. */
  update(props: MountProps): void;
  /** Stop everything and release the GPU. Idempotent. Nothing is called back afterwards. */
  destroy(): void;
  /** The view once it is ready, `undefined` before that and after `destroy()`. */
  readonly view: SociogramView | undefined;
  /**
   * Resolves with the view once it exists and the GPU has finished the data
   * applied so far (pipelines compiled, graph uploaded, first frame drawn).
   * Resolves `undefined`, never rejects, if creation failed or the mount was
   * destroyed first; the failure itself goes to `onError`. Data passed to
   * `update` afterwards is not awaited.
   */
  readonly ready: Promise<SociogramView | undefined>;
}

const defaultDeps: MountDeps = {
  // Loaded on demand: the engine (and its WebAssembly glue) is only touched in
  // the browser, never while a server renders the page, and bundlers can put it
  // in its own chunk.
  create: async (canvas, options) => (await import("./view.js")).SociogramView.create(canvas, options),
  createCanvas: (container) => {
    const canvas = container.ownerDocument.createElement("canvas");
    // Fill the container; the host decides the size.
    canvas.style.display = "block";
    canvas.style.width = "100%";
    canvas.style.height = "100%";
    canvas.style.touchAction = "none";
    container.appendChild(canvas);
    return canvas;
  },
};

function toError(e: unknown): Error {
  return e instanceof Error ? e : new Error(String(e));
}

export function mountSociogram(
  container: HTMLElement,
  config: MountConfig = {},
  deps: MountDeps = defaultDeps,
): Mounted {
  const canvas = deps.createCanvas(container);
  const callbacks = (): MountCallbacks => config.callbacks?.() ?? {};

  let destroyed = false;
  let view: SociogramView | undefined;
  const unsubscribe: Array<() => void> = [];

  // Latest wanted props, and what is already applied to the view.
  let wanted: MountProps = { ...config.initial };
  let appliedData: GraphData | undefined;
  let appliedFade: LinkFadeMode | undefined;
  let fadeApplied = false;
  let appliedDrag: boolean | undefined;
  let appliedPins: ArrayLike<number> | undefined;
  let appliedTool: SelectionTool | undefined;

  const report = (e: unknown, phase: MountErrorPhase): void => {
    if (!destroyed) callbacks().onError?.(toError(e), phase);
  };

  const apply = (): void => {
    if (view === undefined || destroyed) return;
    const { data, linkFade } = wanted;
    if (data !== undefined && data !== appliedData) {
      appliedData = data; // remember even on failure: do not retry the same bad object forever
      try {
        view.setData(data);
        appliedPins = undefined; // new data starts unpinned: apply the pins again below
      } catch (e) {
        report(e, "data");
      }
    }
    if (linkFade !== undefined && (!fadeApplied || !sameFade(linkFade, appliedFade))) {
      fadeApplied = true;
      appliedFade = linkFade;
      try {
        view.setLinkFade(linkFade);
      } catch (e) {
        report(e, "data");
      }
    }
    if (wanted.dragNodes !== undefined && wanted.dragNodes !== appliedDrag) {
      appliedDrag = wanted.dragNodes;
      view.setDragNodes(wanted.dragNodes);
    }
    if (wanted.selectionTool !== undefined && wanted.selectionTool !== appliedTool) {
      appliedTool = wanted.selectionTool;
      try {
        view.setSelectionTool(wanted.selectionTool);
      } catch (e) {
        report(e, "data");
      }
    }
    if (wanted.pinnedNodes !== undefined && wanted.pinnedNodes !== appliedPins) {
      appliedPins = wanted.pinnedNodes; // remember even on failure, like data
      try {
        view.setPinnedNodes(wanted.pinnedNodes);
      } catch (e) {
        report(e, "data");
      }
    }
  };

  // `ready` never rejects: failures are reported through `onError`, and a
  // promise nobody awaits must not raise an unhandled rejection.
  const ready = deps
    .create(canvas, { ...config.options })
    .then(async (created): Promise<SociogramView | undefined> => {
      if (destroyed) {
        // Unmounted while the GPU was still starting: give everything back.
        created.destroy();
        return undefined;
      }
      view = created;
      unsubscribe.push(
        created.on("nodeHover", (i) => callbacks().onNodeHover?.(i)),
        created.on("nodeClick", (i) => callbacks().onNodeClick?.(i)),
        created.on("nodeDragStart", (e) => callbacks().onNodeDragStart?.(e)),
        created.on("nodeDrag", (e) => callbacks().onNodeDrag?.(e)),
        created.on("nodeDragEnd", (e) => callbacks().onNodeDragEnd?.(e)),
        created.on("selectionChange", (e) => callbacks().onSelectionChange?.(e)),
        created.on("error", (e) => report(e, "render")),
      );
      apply();
      try {
        callbacks().onReady?.(created);
      } catch (e) {
        report(e, "callback"); // the host's bug must not unmount a working view
      }
      try {
        await created.whenIdle();
      } catch (e) {
        // Destroyed meanwhile (not an error) or the GPU never answered.
        if (!destroyed) report(e, "render");
        return undefined;
      }
      return destroyed ? undefined : created;
    })
    .catch((e: unknown) => {
      report(e, "create");
      return undefined;
    });

  return {
    ready,
    update(props) {
      if (destroyed) return;
      wanted = { ...wanted, ...definedOnly(props) };
      apply();
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      for (const off of unsubscribe.splice(0)) off();
      view?.destroy();
      view = undefined;
      canvas.remove();
    },
    get view() {
      return view;
    },
  };
}

function definedOnly(props: MountProps): MountProps {
  const out: MountProps = {};
  if (props.data !== undefined) out.data = props.data;
  if (props.linkFade !== undefined) out.linkFade = props.linkFade;
  if (props.dragNodes !== undefined) out.dragNodes = props.dragNodes;
  if (props.pinnedNodes !== undefined) out.pinnedNodes = props.pinnedNodes;
  if (props.selectionTool !== undefined) out.selectionTool = props.selectionTool;
  return out;
}

function sameFade(a: LinkFadeMode, b: LinkFadeMode | undefined): boolean {
  if (b === undefined) return false;
  if (typeof a === "string" || typeof b === "string") return a === b;
  return a.startPx === b.startPx && a.endPx === b.endPx;
}
