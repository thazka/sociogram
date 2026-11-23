// The compiled code imports "./wasm/sociogram_web.js"; make dist self-contained
// by copying the wasm-bindgen output next to it.
import { cpSync, mkdirSync } from "node:fs";
mkdirSync("dist/wasm", { recursive: true });
cpSync("src/wasm", "dist/wasm", { recursive: true });
