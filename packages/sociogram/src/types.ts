// The public data and event types of `SociogramView`. They live in their own
// file so `view.ts` stays small; `view.ts` re-exports them, so importing them
// from either place works.

import type { ClickSelection } from "./pointer.js";

export interface GraphData {
  nodeCount: number;
  /** Flat `[source, target, source, target, ...]`. */
  edges: ArrayLike<number> | Uint32Array;
  /** Two numbers per node. Random if omitted. */
  positions?: Float32Array;
  /** One radius per node, in pixels at zoom 1. */
  sizes?: Float32Array;
  /** RGBA (0..1), four numbers per node. */
  colors?: Float32Array;
}

/**
 * How long links fade out (they dominate the cost of drawing large graphs).
 *
 * - `"auto"` (default): nothing changes for small graphs; graphs with 50,000 or
 *   more links fade links between 1/8 and 3/8 of the viewport size.
 * - `"off"`: always draw every link at full strength.
 * - `{ startPx, endPx }`: fade between these on-screen lengths, in CSS pixels.
 */
export type LinkFadeMode = "auto" | "off" | { startPx: number; endPx: number };

export interface SociogramOptions {
  /** Where to load `sociogram_web_bg.wasm` from. Defaults to next to the module. */
  wasmUrl?: string | URL;
  /** Upper bound for the device pixel ratio used for the drawing buffer. */
  maxPixelRatio?: number;
  /** Dragging a node moves it, dragging empty space pans. Default `true`. */
  dragNodes?: boolean;
  /** Layout temperature (0 to 1) kept while a node is dragged, so the graph reacts. Default `0.3`. */
  dragHeat?: number;
  /** What clicking a node selects. Default `"node"`. */
  clickSelection?: ClickSelection;
  /** Dim everything outside the selection. Default `true`. */
  dimUnselected?: boolean;
}

/** A node drag started, moved or ended. `x` and `y` are world coordinates. */
export interface NodeDragEvent {
  index: number;
  x: number;
  y: number;
}

export interface SelectionChange {
  /** Indices of the selected nodes. */
  nodes: Uint32Array;
  /** Number of selected links (self-loops included). */
  linkCount: number;
}

export interface SociogramEvents {
  /** Fired when the node under the pointer changes (`null` = none). */
  nodeHover: (index: number | null) => void;
  /** Fired for a click (not a drag) on a node. */
  nodeClick: (index: number) => void;
  nodeDragStart: (e: NodeDragEvent) => void;
  nodeDrag: (e: NodeDragEvent) => void;
  nodeDragEnd: (e: NodeDragEvent) => void;
  /** The selection changed through the pointer or the API. */
  selectionChange: (e: SelectionChange) => void;
  /** Fired once if the render loop stopped because of an error. */
  error: (error: Error) => void;
}
