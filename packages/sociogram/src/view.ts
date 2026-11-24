import { WasmLoadError, WebGpuUnsupportedError } from "./errors.js";
import init, { Sociogram } from "./wasm/sociogram_web.js";
import { toIndices } from "./indices.js";
import { DomOverlay } from "./overlay.js";
import { adaptEngine, OP_CODE } from "./pointer-engine.js";
import { PointerMachine } from "./pointer.js";
import type { ClickSelection, SelectOp, SelectionTool } from "./pointer.js";

/** Largest drawing-buffer side, matching the engine's limit. */
const MAX_SIDE = 8192;
/** Frames between position snapshots used for picking. */
const SNAPSHOT_EVERY = 6;
const CAPTURE_TIMEOUT_MS = 10_000;
const IDLE_TIMEOUT_MS = 30_000;

import type {
  GraphData,
  LinkFadeMode,
  NodeDragEvent,
  SelectionChange,
  SociogramEvents,
  SociogramOptions,
} from "./types.js";

export type { GraphData, LinkFadeMode, NodeDragEvent, SelectionChange, SociogramEvents, SociogramOptions };

type Listeners = { [K in keyof SociogramEvents]: Set<SociogramEvents[K]> };

let wasmReady: Promise<unknown> | undefined;

function loadWasm(url: string | URL | undefined): Promise<unknown> {
  // One shared initialisation; a failed load can be retried.
  wasmReady ??= init(url === undefined ? undefined : { module_or_path: url }).catch(
    (e: unknown) => {
      wasmReady = undefined;
      throw new WasmLoadError(e);
    },
  );
  return wasmReady;
}

export { WasmLoadError, WebGpuUnsupportedError };

export class SociogramView {
  readonly #canvas: HTMLCanvasElement;
  readonly #maxPixelRatio: number;
  readonly #listeners: Listeners = {
    nodeHover: new Set(),
    nodeClick: new Set(),
    nodeDragStart: new Set(),
    nodeDrag: new Set(),
    nodeDragEnd: new Set(),
    selectionChange: new Set(),
    error: new Set(),
  };
  readonly #cleanup: Array<() => void> = [];
  #engine: Sociogram | undefined;
  #raf = 0;
  #frameCount = 0;
  #hovered = -1;
  #pointer: { x: number; y: number } | undefined;
  #hoverDirty = false;
  #dragNodes: boolean;
  #tool: SelectionTool = "none";
  #clickSelection: ClickSelection;
  #machine: PointerMachine | undefined;
  #overlay: DomOverlay | undefined;

  private constructor(canvas: HTMLCanvasElement, engine: Sociogram, maxPixelRatio: number, options: SociogramOptions) {
    this.#canvas = canvas;
    this.#engine = engine;
    this.#maxPixelRatio = maxPixelRatio;
    this.#dragNodes = options.dragNodes ?? true;
    this.#clickSelection = options.clickSelection ?? "node";
  }

  /** Create a view on `canvas`. Rejects with `WebGpuUnsupportedError` without WebGPU. */
  static async create(canvas: HTMLCanvasElement, options: SociogramOptions = {}): Promise<SociogramView> {
    if (!("gpu" in navigator)) {
      throw new WebGpuUnsupportedError("WebGPU is not available in this browser");
    }
    await loadWasm(options.wasmUrl);
    // Size the drawing buffer before the surface is created, so the first
    // frame is already at device resolution.
    const ratio = Math.min(options.maxPixelRatio ?? Infinity, window.devicePixelRatio || 1);
    const rect = canvas.getBoundingClientRect();
    canvas.width = clampSide(rect.width * ratio);
    canvas.height = clampSide(rect.height * ratio);

    let engine: Sociogram;
    try {
      engine = await Sociogram.create(canvas);
    } catch (e) {
      throw new WebGpuUnsupportedError(e instanceof Error ? e.message : String(e));
    }
    const heat = options.dragHeat ?? 0.3;
    if (!Number.isFinite(heat) || heat < 0 || heat > 1) {
      engine.free();
      throw new RangeError("dragHeat must be between 0 and 1");
    }
    engine.set_drag_heat(heat);
    engine.set_dim_unselected(options.dimUnselected ?? true);
    const view = new SociogramView(canvas, engine, options.maxPixelRatio ?? Infinity, options);
    view.#attach();
    return view;
  }

  get destroyed(): boolean {
    return this.#engine === undefined;
  }

  /** Replace the graph. Throws on invalid data and keeps the previous graph. */
  setData(data: GraphData): void {
    const engine = this.#live();
    const edges = data.edges instanceof Uint32Array ? data.edges : Uint32Array.from(data.edges);
    const hadSelection = engine.selection_active();
    engine.set_data(data.nodeCount, edges, data.positions, data.sizes, data.colors);
    if (hadSelection) this.#emitSelection(); // new data starts without a selection
  }

  on<K extends keyof SociogramEvents>(event: K, listener: SociogramEvents[K]): () => void {
    const set = this.#listeners[event] as Set<SociogramEvents[K]>;
    set.add(listener);
    return () => void set.delete(listener);
  }

  fitView(): void {
    this.#live().fit_view();
  }

  /** Make the layout move again. */
  reheat(): void {
    this.#live().reheat();
  }

  /** Switch node dragging on or off (off: a drag always pans). */
  setDragNodes(on: boolean): void {
    this.#live();
    this.#dragNodes = on;
  }

  setClickSelection(mode: ClickSelection): void {
    if (mode !== "node" && mode !== "neighbours" && mode !== "none") {
      throw new RangeError(`unknown clickSelection "${String(mode)}"`);
    }
    this.#live();
    this.#clickSelection = mode;
  }

  /** Choose what a drag on the canvas does: nothing special, a rectangle or a lasso. */
  setSelectionTool(tool: SelectionTool): void {
    if (tool !== "none" && tool !== "rectangle" && tool !== "lasso") {
      throw new RangeError(`unknown selection tool "${String(tool)}"`);
    }
    this.#live();
    this.#tool = tool;
    if (tool === "none") this.#machine?.cancel();
  }

  /** Pin exactly these nodes; an empty list unpins all. Throws `RangeError` for an unknown index. */
  setPinnedNodes(indices: ArrayLike<number>): void {
    const engine = this.#live();
    engine.set_pinned(toIndices(indices, engine.node_count()));
  }

  getPinnedNodes(): Uint32Array {
    return Uint32Array.from(this.#live().pinned());
  }

  /** Select these nodes (and the links among them). Throws `RangeError` for an unknown index. */
  setSelection(indices: ArrayLike<number>, mode: SelectOp = "replace"): void {
    const engine = this.#live();
    engine.select_nodes(toIndices(indices, engine.node_count()), OP_CODE[mode]);
    this.#emitSelection();
  }

  getSelection(): Uint32Array {
    return Uint32Array.from(this.#live().selected_nodes());
  }

  clearSelection(): void {
    this.#live().clear_selection();
    this.#emitSelection();
  }

  /** Dim everything outside the selection (default) or draw it normally. */
  setDimUnselected(on: boolean): void {
    this.#live().set_dim_unselected(on);
  }

  /** Where a node is on screen, in CSS pixels relative to the viewport. */
  nodeScreenPosition(index: number): { x: number; y: number } | undefined {
    const p = this.#live().node_screen_position(index);
    if (p.length < 2) return undefined;
    const r = this.#canvas.getBoundingClientRect();
    const scale = this.#canvas.width / Math.max(r.width, 1);
    return { x: r.left + (p[0] as number) / scale, y: r.top + (p[1] as number) / scale };
  }

  /**
   * Fade out long links; see {@link LinkFadeMode}. Invalid ranges (not finite,
   * negative, `endPx <= startPx`) throw a `RangeError`.
   */
  setLinkFade(mode: LinkFadeMode): void {
    const engine = this.#live();
    if (mode === "auto") engine.set_link_fade_auto();
    else if (mode === "off") engine.set_link_fade_off();
    else {
      const { startPx, endPx } = mode;
      if (!Number.isFinite(startPx) || !Number.isFinite(endPx) || startPx < 0 || endPx <= startPx) {
        throw new RangeError("setLinkFade: expected finite 0 <= startPx < endPx");
      }
      // CSS pixels to drawing-buffer pixels.
      const scale = this.#canvas.width / Math.max(this.#canvas.getBoundingClientRect().width, 1);
      engine.set_link_fade(startPx * scale, endPx * scale);
    }
  }

  /**
   * Resolve when the GPU has finished everything submitted so far (pipelines
   * compiled, buffers uploaded, first layout steps and frame). Rejects if the
   * view is destroyed meanwhile or the GPU does not answer in time.
   */
  async whenIdle(timeoutMs = IDLE_TIMEOUT_MS): Promise<void> {
    this.#live().request_idle();
    const start = performance.now();
    for (;;) {
      const engine = this.#engine;
      if (engine === undefined) throw new Error("destroyed while waiting for the GPU");
      if (engine.poll_idle()) return;
      if (performance.now() - start > timeoutMs) throw new Error("the GPU did not become idle in time");
      await new Promise((r) => setTimeout(r, 4));
    }
  }

  /** Render the current view offscreen and return it as a PNG. */
  async screenshot(): Promise<Blob> {
    const engine = this.#live();
    engine.request_capture();
    const start = performance.now();
    for (;;) {
      const engine2 = this.#engine;
      if (engine2 === undefined) throw new Error("destroyed while capturing");
      const capture = engine2.poll_capture();
      if (capture !== undefined) {
        const scratch = document.createElement("canvas");
        scratch.width = capture.width;
        scratch.height = capture.height;
        const ctx = scratch.getContext("2d");
        if (ctx === null) throw new Error("2D canvas is unavailable");
        const pixels = new Uint8ClampedArray(capture.rgba);
        capture.free();
        ctx.putImageData(new ImageData(pixels, scratch.width, scratch.height), 0, 0);
        return new Promise((resolve, reject) =>
          scratch.toBlob((b) => (b ? resolve(b) : reject(new Error("PNG encoding failed"))), "image/png"),
        );
      }
      if (performance.now() - start > CAPTURE_TIMEOUT_MS) throw new Error("screenshot timed out");
      await new Promise((r) => setTimeout(r, 4));
    }
  }

  /** Stop rendering and release the GPU. Safe to call more than once. */
  destroy(): void {
    const engine = this.#engine;
    if (engine === undefined) return;
    this.#engine = undefined;
    cancelAnimationFrame(this.#raf);
    this.#machine?.cancel();
    this.#overlay?.hide();
    for (const undo of this.#cleanup.splice(0)) undo();
    for (const set of Object.values(this.#listeners)) (set as Set<unknown>).clear();
    engine.destroy();
    engine.free();
  }

  #live(): Sociogram {
    if (this.#engine === undefined) throw new Error("this SociogramView was destroyed");
    return this.#engine;
  }

  #emitSelection(): void {
    const engine = this.#engine;
    if (engine === undefined) return;
    this.#emit("selectionChange", {
      nodes: Uint32Array.from(engine.selected_nodes()),
      linkCount: engine.selected_link_count(),
    });
  }

  #emit<K extends keyof SociogramEvents>(event: K, ...args: Parameters<SociogramEvents[K]>): void {
    for (const fn of this.#listeners[event] as Set<(...a: Parameters<SociogramEvents[K]>) => void>) {
      try {
        fn(...args);
      } catch (e) {
        console.error(`sociogram: ${event} listener threw`, e);
      }
    }
  }

  #listen<T extends EventTarget>(target: T, type: string, fn: (e: never) => void, opts?: AddEventListenerOptions): void {
    target.addEventListener(type, fn as EventListener, opts);
    this.#cleanup.push(() => target.removeEventListener(type, fn as EventListener, opts));
  }

  /** Canvas pixel coordinates of a pointer event (physical pixels). */
  #toCanvas(e: { clientX: number; clientY: number }): { x: number; y: number } {
    const r = this.#canvas.getBoundingClientRect();
    return {
      x: ((e.clientX - r.left) * this.#canvas.width) / Math.max(r.width, 1),
      y: ((e.clientY - r.top) * this.#canvas.height) / Math.max(r.height, 1),
    };
  }

  #attach(): void {
    const canvas = this.#canvas;
    this.#observeSize();

    const engine = this.#live();
    const toInput = (e: PointerEvent) => ({
      ...this.#toCanvas(e),
      clientX: e.clientX,
      clientY: e.clientY,
      shiftKey: e.shiftKey,
      altKey: e.altKey,
    });
    this.#overlay = new DomOverlay(document);
    const machine = new PointerMachine({
      engine: adaptEngine(engine),
      overlay: this.#overlay,
      config: () => ({ dragNodes: this.#dragNodes, tool: this.#tool, clickSelection: this.#clickSelection }),
      scale: () => canvas.width / Math.max(canvas.getBoundingClientRect().width, 1),
      hooks: {
        dragStart: (index, x, y) => this.#emit("nodeDragStart", { index, x, y }),
        drag: (index, x, y) => this.#emit("nodeDrag", { index, x, y }),
        dragEnd: (index, x, y) => this.#emit("nodeDragEnd", { index, x, y }),
        click: (index) => this.#emit("nodeClick", index),
        selectionChanged: () => this.#emitSelection(),
      },
    });
    this.#machine = machine;

    this.#listen(canvas, "pointerdown", (e: PointerEvent) => {
      if (e.button !== 0 || !e.isPrimary) return; // left button or first touch only
      canvas.setPointerCapture(e.pointerId);
      machine.down(toInput(e));
    });
    this.#listen(canvas, "pointermove", (e: PointerEvent) => {
      this.#pointer = this.#toCanvas(e);
      if (machine.busy) machine.move(toInput(e));
      else this.#hoverDirty = true;
    });
    this.#listen(canvas, "pointerup", (e: PointerEvent) => {
      if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
      machine.up(toInput(e));
    });
    this.#listen(canvas, "pointercancel", () => machine.cancel());
    this.#listen(canvas, "lostpointercapture", () => machine.cancel());
    this.#listen(window, "blur", () => machine.cancel());
    this.#listen(canvas, "pointerleave", () => {
      this.#pointer = undefined;
      this.#setHover(-1);
    });
    this.#listen(
      canvas,
      "wheel",
      (e: WheelEvent) => {
        e.preventDefault();
        const engine = this.#engine;
        if (engine === undefined) return;
        const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
        const p = this.#toCanvas(e);
        engine.zoom_at(Math.exp(-e.deltaY * unit * 0.0015), p.x, p.y);
        this.#hoverDirty = true;
      },
      { passive: false },
    );

    let lastFrameAt: number | undefined;
    const loop = (now: number): void => {
      const engine = this.#engine;
      if (engine === undefined) return;
      try {
        // Frame pacing feeds the adaptive link detail: slow frames on a large
        // graph make long links fade sooner, fast frames bring them back.
        if (lastFrameAt !== undefined) engine.report_frame_time(now - lastFrameAt);
        lastFrameAt = now;
        engine.frame();
        if (this.#frameCount % SNAPSHOT_EVERY === 0) engine.request_snapshot();
        engine.poll_snapshot();
        this.#frameCount++;
        if (this.#hoverDirty && this.#pointer !== undefined && !machine.busy) {
          this.#hoverDirty = false;
          this.#setHover(engine.pick(this.#pointer.x, this.#pointer.y));
        }
      } catch (e) {
        // A broken render loop must not keep spinning; tell the owner once.
        this.#emit("error", e instanceof Error ? e : new Error(String(e)));
        return;
      }
      this.#raf = requestAnimationFrame(loop);
    };
    this.#raf = requestAnimationFrame(loop);
  }

  #setHover(index: number): void {
    if (index === this.#hovered) return;
    this.#hovered = index;
    this.#canvas.style.cursor = index >= 0 ? "pointer" : "";
    this.#emit("nodeHover", index >= 0 ? index : null);
  }

  /** Keep the drawing buffer at device resolution (HiDPI aware). */
  #observeSize(): void {
    const canvas = this.#canvas;
    const apply = (w: number, h: number): void => {
      const engine = this.#engine;
      if (engine === undefined) return;
      const width = clampSide(w);
      const height = clampSide(h);
      if (width === canvas.width && height === canvas.height) return;
      canvas.width = width;
      canvas.height = height;
      engine.resize(width, height);
    };
    const fromCssSize = (cssW: number, cssH: number): void => {
      const ratio = Math.min(this.#maxPixelRatio, window.devicePixelRatio || 1);
      apply(cssW * ratio, cssH * ratio);
    };
    const observer = new ResizeObserver((entries) => {
      const entry = entries[entries.length - 1];
      const css = entry?.contentBoxSize[0];
      if (entry === undefined || css === undefined) return;
      const ratio = Math.min(this.#maxPixelRatio, window.devicePixelRatio || 1);
      let w = css.inlineSize * ratio;
      let h = css.blockSize * ratio;
      // The browser's own device-pixel size is snapped to whole pixels, which
      // keeps edges crisp at fractional ratios (125%, 150%). Only trust it when
      // it agrees with CSS size x ratio: some environments (device emulation)
      // report CSS pixels there, which would leave the canvas blurry.
      const device = entry.devicePixelContentBoxSize?.[0];
      if (
        device !== undefined &&
        Math.abs(device.inlineSize - w) <= 2 &&
        Math.abs(device.blockSize - h) <= 2
      ) {
        w = device.inlineSize;
        h = device.blockSize;
      }
      apply(w, h);
    });
    try {
      observer.observe(canvas, { box: "device-pixel-content-box" });
    } catch {
      observer.observe(canvas);
    }
    this.#cleanup.push(() => observer.disconnect());

    // Moving the window to another monitor, or browser zoom, changes the ratio
    // without necessarily changing the CSS size.
    const watchRatio = (): void => {
      const query = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      const onChange = (): void => {
        const r = canvas.getBoundingClientRect();
        fromCssSize(r.width, r.height);
        watchRatio();
      };
      query.addEventListener("change", onChange, { once: true });
      this.#cleanup.push(() => query.removeEventListener("change", onChange));
    };
    watchRatio();
  }
}

function clampSide(n: number): number {
  return Math.min(MAX_SIDE, Math.max(1, Math.round(n)));
}
