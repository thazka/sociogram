import { describe, expect, it } from "vitest";
import { WasmLoadError, WebGpuUnsupportedError } from "../src/errors.js";

describe("WasmLoadError", () => {
  it("keeps the original error as the cause and includes its message", () => {
    const original = new Error("failed to fetch Wasm: 404 Not Found");
    const error = new WasmLoadError(original);
    expect(error).toBeInstanceOf(Error);
    expect(error.name).toBe("WasmLoadError");
    expect(error.cause).toBe(original);
    expect(error.message).toContain("404 Not Found");
  });

  it("tells the reader what to do about it", () => {
    const message = new WasmLoadError(new Error("x")).message;
    expect(message).toContain("sociogram_web_bg.wasm");
    expect(message).toContain("wasmUrl");
    expect(message).toContain("public assets");
  });

  it("accepts values that are not errors", () => {
    const error = new WasmLoadError("plain string");
    expect(error.message).toContain("plain string");
    expect(error.cause).toBe("plain string");
  });
});

describe("WebGpuUnsupportedError", () => {
  it("has a stable name callers can check", () => {
    const error = new WebGpuUnsupportedError("no adapter");
    expect(error.name).toBe("WebGpuUnsupportedError");
    expect(error).toBeInstanceOf(Error);
  });
});
