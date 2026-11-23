/** Thrown by `SociogramView.create` when the browser cannot run the engine (no WebGPU, or no suitable GPU). */
export class WebGpuUnsupportedError extends Error {
  override name = "WebGpuUnsupportedError";
}

/**
 * Thrown by `SociogramView.create` when the engine's WebAssembly file cannot be
 * loaded. The original error is in `cause`.
 */
export class WasmLoadError extends Error {
  override name = "WasmLoadError";

  constructor(cause: unknown) {
    const reason = cause instanceof Error ? cause.message : String(cause);
    super(
      `Could not load the sociogram engine (sociogram_web_bg.wasm): ${reason}. ` +
        "Most bundlers copy this file for you. If yours does not (Angular's builder, for example), " +
        "copy node_modules/sociogram/dist/wasm/sociogram_web_bg.wasm to your public assets, " +
        "or pass its URL in the `wasmUrl` option.",
      { cause },
    );
  }
}
