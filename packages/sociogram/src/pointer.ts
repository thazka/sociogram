// The pointer state machine: decides what a press, a move and a release mean.
//
// A press on empty canvas pans the camera. A press on a node drags that node,
// but only once the pointer has moved past the click slop (a press that does
// not move is a click). While a selection tool is active the press draws a
// rectangle or a lasso instead. The machine is pure: the engine, the overlay
// and the host callbacks are injected, so it is tested without a browser.

export type SelectionTool = "none" | "rectangle" | "lasso";
export type ClickSelection = "node" | "neighbours" | "none";
export type SelectOp = "replace" | "add" | "remove";

/** Pointer movement (CSS pixels) below which a press counts as a click. */
export const CLICK_SLOP = 4;
/** CSS pixels between stored lasso points. */
export const MIN_LASSO_STEP = 2;
/** A longer lasso path stops growing (the engine caps polygons too). */
export const MAX_LASSO_POINTS = 1000;

export interface PointerInput {
  /** Canvas (drawing-buffer) pixels. */
  x: number;
  y: number;
  /** CSS pixels. */
  clientX: number;
  clientY: number;
  shiftKey: boolean;
  altKey: boolean;
}

export interface PointerEngine {
  /** Node index under the position, or -1. */
  pick(x: number, y: number): number;
  /** Start dragging the node under the position: its index, or -1. */
  beginDrag(x: number, y: number): number;
  dragTo(x: number, y: number): void;
  endDrag(): void;
  /** Pan by canvas pixels. */
  panBy(dx: number, dy: number): void;
  selectRect(x0: number, y0: number, x1: number, y1: number, op: SelectOp): void;
  selectPolygon(xy: Float32Array, op: SelectOp): void;
  selectAt(x: number, y: number, neighbours: boolean, op: SelectOp): "none" | "node" | "link";
  clearSelection(): void;
  hasSelection(): boolean;
  toWorld(x: number, y: number): readonly [number, number];
}

export interface PointerOverlay {
  rectangle(x0: number, y0: number, x1: number, y1: number): void;
  polygon(points: ReadonlyArray<readonly [number, number]>): void;
  hide(): void;
}

export interface PointerHooks {
  /** World position of the pointer. */
  dragStart(index: number, x: number, y: number): void;
  drag(index: number, x: number, y: number): void;
  dragEnd(index: number, x: number, y: number): void;
  click(index: number): void;
  selectionChanged(): void;
}

export interface PointerConfig {
  dragNodes: boolean;
  tool: SelectionTool;
  clickSelection: ClickSelection;
}

export interface PointerDeps {
  engine: PointerEngine;
  overlay: PointerOverlay;
  hooks: PointerHooks;
  config: () => PointerConfig;
  /** Canvas pixels per CSS pixel. */
  scale: () => number;
}

type State = "idle" | "pressed" | "panning" | "dragging" | "selecting";

const opOf = (e: PointerInput): SelectOp => (e.shiftKey ? "add" : e.altKey ? "remove" : "replace");

export class PointerMachine {
  readonly #d: PointerDeps;
  #state: State = "idle";
  #last: PointerInput | undefined;
  #start: PointerInput | undefined;
  #moved = 0;
  #node = -1;
  #pending = -1; // node under the press, while it may still turn out to be a click
  #path: Array<[number, number]> = []; // canvas pixels
  #clientPath: Array<[number, number]> = [];

  constructor(deps: PointerDeps) {
    this.#d = deps;
  }

  /** A gesture is in progress. */
  get busy(): boolean {
    return this.#state !== "idle";
  }

  down(e: PointerInput): void {
    if (this.#state !== "idle") return;
    const cfg = this.#d.config();
    this.#start = this.#last = e;
    this.#moved = 0;
    if (cfg.tool !== "none") {
      this.#state = "selecting";
      this.#path = [[e.x, e.y]];
      this.#clientPath = [[e.clientX, e.clientY]];
      return;
    }
    // A press on a node may still turn out to be a click, so the drag only
    // starts once the pointer has moved past the click slop (see `move`).
    this.#pending = cfg.dragNodes ? this.#d.engine.pick(e.x, e.y) : -1;
    this.#state = this.#pending >= 0 ? "pressed" : "panning";
  }

  move(e: PointerInput): void {
    const last = this.#last;
    if (this.#state === "idle" || last === undefined) return;
    const dx = e.clientX - last.clientX;
    const dy = e.clientY - last.clientY;
    this.#moved += Math.abs(dx) + Math.abs(dy);
    this.#last = e;
    const { engine, overlay, hooks, config, scale } = this.#d;
    if (this.#state === "pressed") {
      if (this.#moved < CLICK_SLOP) return;
      const start = this.#start;
      const hit = start === undefined ? -1 : engine.beginDrag(start.x, start.y);
      if (start !== undefined && hit >= 0) {
        this.#state = "dragging";
        this.#node = hit;
        const [wx, wy] = engine.toWorld(start.x, start.y);
        hooks.dragStart(hit, wx, wy);
      } else {
        this.#state = "panning";
      }
    }
    if (this.#state === "panning") {
      engine.panBy(dx * scale(), dy * scale());
    } else if (this.#state === "dragging") {
      engine.dragTo(e.x, e.y);
      const [wx, wy] = engine.toWorld(e.x, e.y);
      hooks.drag(this.#node, wx, wy);
    } else if (this.#start !== undefined) {
      const lastPoint = this.#clientPath[this.#clientPath.length - 1];
      if (
        lastPoint === undefined ||
        (this.#path.length < MAX_LASSO_POINTS &&
          Math.hypot(e.clientX - lastPoint[0], e.clientY - lastPoint[1]) >= MIN_LASSO_STEP)
      ) {
        this.#path.push([e.x, e.y]);
        this.#clientPath.push([e.clientX, e.clientY]);
      }
      if (config().tool === "rectangle") {
        overlay.rectangle(this.#start.clientX, this.#start.clientY, e.clientX, e.clientY);
      } else {
        overlay.polygon(this.#clientPath);
      }
    }
  }

  up(e: PointerInput): void {
    const state = this.#state;
    if (state === "idle") return;
    const start = this.#start;
    const moved = this.#moved;
    const { engine, overlay, hooks, config } = this.#d;
    if (state === "dragging") {
      const [wx, wy] = engine.toWorld(e.x, e.y);
      engine.endDrag();
      hooks.dragEnd(this.#node, wx, wy);
      this.#reset();
      if (moved < CLICK_SLOP) this.#click(e);
    } else if (state === "selecting" && start !== undefined) {
      const tool = config().tool;
      // The release position closes the path.
      const path: Array<[number, number]> = [...this.#path, [e.x, e.y]];
      overlay.hide();
      this.#reset();
      if (moved < CLICK_SLOP) {
        this.#click(e);
      } else if (tool === "rectangle") {
        engine.selectRect(start.x, start.y, e.x, e.y, opOf(e));
        hooks.selectionChanged();
      } else if (path.length >= 3) {
        engine.selectPolygon(Float32Array.from(path.flat()), opOf(e));
        hooks.selectionChanged();
      }
    } else {
      this.#reset();
      if (moved < CLICK_SLOP) this.#click(e);
    }
  }

  cancel(): void {
    if (this.#state === "dragging") {
      this.#d.engine.endDrag();
      const last = this.#last;
      const [wx, wy] = last ? this.#d.engine.toWorld(last.x, last.y) : [0, 0];
      this.#d.hooks.dragEnd(this.#node, wx, wy);
    }
    this.#d.overlay.hide();
    this.#reset();
  }

  #reset(): void {
    this.#state = "idle";
    this.#last = this.#start = undefined;
    this.#node = -1;
    this.#pending = -1;
    this.#path = [];
    this.#clientPath = [];
  }

  #click(e: PointerInput): void {
    const { engine, hooks, config } = this.#d;
    const hit = engine.pick(e.x, e.y);
    if (hit >= 0) hooks.click(hit);
    const mode = config().clickSelection;
    if (mode === "none") return;
    const op = opOf(e);
    const found = engine.selectAt(e.x, e.y, mode === "neighbours", op);
    if (found !== "none") {
      hooks.selectionChanged();
    } else if (op === "replace" && engine.hasSelection()) {
      engine.clearSelection();
      hooks.selectionChanged();
    }
  }
}
