// Build the Rust engine for the browser and generate the JavaScript glue into
// src/wasm/ (generated files are not committed).
//
// Requirements, checked below with a clear message:
//   rustup target add wasm32-unknown-unknown
//   cargo install wasm-bindgen-cli --version <pinned> --locked
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../..");
const outDir = resolve(here, "../src/wasm");
const crate = "sociogram-web";
const wasmFile = join(repoRoot, "target/wasm32-unknown-unknown/release/sociogram_web.wasm");

function run(cmd, args, options = {}) {
  return execFileSync(cmd, args, { stdio: "inherit", cwd: repoRoot, ...options });
}

function capture(cmd, args) {
  try {
    return execFileSync(cmd, args, { cwd: repoRoot, encoding: "utf8" }).trim();
  } catch {
    return undefined;
  }
}

// The wasm-bindgen CLI must match the crate version exactly or the glue fails to load.
const manifest = readFileSync(join(repoRoot, "crates", crate, "Cargo.toml"), "utf8");
const pinned = /wasm-bindgen\s*=\s*"=([\d.]+)"/.exec(manifest)?.[1];
if (pinned === undefined) {
  console.error(`Could not find the pinned wasm-bindgen version in crates/${crate}/Cargo.toml`);
  process.exit(1);
}
const installed = capture("wasm-bindgen", ["--version"])?.split(" ")[1];
if (installed !== pinned) {
  console.error(
    installed === undefined
      ? `wasm-bindgen CLI not found. Install it with:\n  cargo install wasm-bindgen-cli --version ${pinned} --locked`
      : `wasm-bindgen CLI is ${installed} but the crate pins ${pinned}. Install the matching one:\n  cargo install wasm-bindgen-cli --version ${pinned} --locked --force`,
  );
  process.exit(1);
}
const targets = capture("rustup", ["target", "list", "--installed"]) ?? "";
if (!targets.split(/\r?\n/).includes("wasm32-unknown-unknown")) {
  console.error("The wasm32 target is missing. Install it with:\n  rustup target add wasm32-unknown-unknown");
  process.exit(1);
}

run("cargo", ["build", "-p", crate, "--target", "wasm32-unknown-unknown", "--release"]);
if (!existsSync(wasmFile)) {
  console.error(`Expected build output not found: ${wasmFile}`);
  process.exit(1);
}
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
run("wasm-bindgen", ["--target", "web", "--typescript", "--out-dir", outDir, wasmFile]);
console.log(`wasm glue written to ${outDir}`);
