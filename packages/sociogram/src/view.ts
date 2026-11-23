import { WasmLoadError, WebGpuUnsupportedError } from "./errors.js";
import init, { Sociogram } from "./wasm/sociogram_web.js";

/** Largest drawing-buffer side, matching the engine's limit. */
const MAX_SIDE = 8192;
/** Pointer movement (CSS pixels) below which a press counts as a click. */
const CLICK_SLOP = 4;
/** Frames between position snapshots used for picking. */
const SNAPSHOT_EVERY = 6;
const CAPTURE_TIMEOUT_MS = 10_000;
const IDLE_TIMEOUT_MS = 30_000;

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
}

export interface SociogramEvents {
  /** Fired when the node under the pointer changes (`null` = none). */
  nodeHover: (index: number | null) => void;
  /** Fired for a click (not a drag) on a node. */
  nodeClick: (index: number) => void;
  /** Fired once if the render loop stopped because of an error. */
  error: (error: Error) => void;
}

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
  readonly #listeners: Listeners = { nodeHover: new Set(), nodeClick: new Set(), error: new Set() };
  readonly #cleanup: Array<() => void> = [];
  #engine: Sociogram | undefined;
  #raf = 0;
  #frameCount = 0;
  #hovered = -1;
  #pointer: { x: number; y: number } | undefined;
  #hoverDirty = false;
  #drag: { x: number; y: number; moved: number } | undefined;

  private constructor(canvas: HTMLCanvasElement, engine: Sociogram, maxPixelRatio: number) {
    this.#canvas = canvas;
    this.#engine = engine;
    this.#maxPixelRatio = maxPixelRatio;
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
    const view = new SociogramView(canvas, engine, options.maxPixelRatio ?? Infinity);
    view.#attach();
    return view;
  }

  get destroyed(): boolean {
    return this.#engine === undefined;
  }

  /** Replace the graph. Throws on invalid data and keeps the previous graph. */
  setData(data: GraphData): void {
    const edges = data.edges instanceof Uint32Array ? data.edges : Uint32Array.from(data.edges);
    this.#live().set_data(data.nodeCount, edges, data.positions, data.sizes, data.colors);
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
    for (const undo of this.#cleanup.splice(0)) undo();
    for (const set of Object.values(this.#listeners)) (set as Set<unknown>).clear();
    engine.destroy();
    engine.free();
  }

  #live(): Sociogram {
    if (this.#engine === undefined) throw new Error("this SociogramView was destroyed");
    return this.#engine;
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

    this.#listen(canvas, "pointerdown", (e: PointerEvent) => {
      canvas.setPointerCapture(e.pointerId);
      this.#drag = { x: e.clientX, y: e.clientY, moved: 0 };
    });
    this.#listen(canvas, "pointermove", (e: PointerEvent) => {
      const engine = this.#engine;
      if (engine === undefined) return;
      const p = this.#toCanvas(e);
      this.#pointer = p;
      if (this.#drag !== undefined) {
        const dx = e.clientX - this.#drag.x;
        const dy = e.clientY - this.#drag.y;
        this.#drag.moved += Math.abs(dx) + Math.abs(dy);
        this.#drag.x = e.clientX;
        this.#drag.y = e.clientY;
        const scale = canvas.width / Math.max(canvas.getBoundingClientRect().width, 1);
        engine.pan_by(dx * scale, dy * scale);
      } else {
        this.#hoverDirty = true;
      }
    });
    const release = (e: PointerEvent): void => {
      const drag = this.#drag;
      this.#drag = undefined;
      if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
      if (drag !== undefined && drag.moved < CLICK_SLOP && this.#engine !== undefined) {
        const p = this.#toCanvas(e);
        const hit = this.#engine.pick(p.x, p.y);
        if (hit >= 0) this.#emit("nodeClick", hit);
      }
    };
    this.#listen(canvas, "pointerup", release);
    this.#listen(canvas, "pointercancel", () => {
      this.#drag = undefined;
    });
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
        if (this.#hoverDirty && this.#pointer !== undefined && this.#drag === undefined) {
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
