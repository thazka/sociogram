// Everything CI checks, plus what CI cannot: the GPU tests on a real GPU and the
// browser tests. Run it before pushing:
//
//   node scripts/verify.mjs               # everything
//   node scripts/verify.mjs --no-browser  # skip the headless-Chrome checks
//
// GPU tests run with SOCIOGRAM_REQUIRE_GPU=1, so a machine without a real GPU
// fails here instead of silently skipping them.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const skipBrowser = process.argv.includes("--no-browser");

const steps = [
  { name: "cargo fmt", cmd: "cargo", args: ["fmt", "--all", "--", "--check"] },
  { name: "clippy (native)", cmd: "cargo", args: ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"] },
  { name: "clippy (wasm32)", cmd: "cargo", args: ["clippy", "-p", "sociogram-web", "--target", "wasm32-unknown-unknown", "--", "-D", "warnings"] },
  {
    name: "cargo test (GPU tests required)",
    cmd: "cargo",
    args: ["test", "--workspace"],
    env: { SOCIOGRAM_REQUIRE_GPU: "1" },
  },
  {
    name: "npm package build",
    cwd: "packages/sociogram",
    cmd: "npm",
    args: ["run", "build"],
    before: () => ensureInstalled("packages/sociogram"),
  },
];
if (!skipBrowser) {
  steps.push({
    name: "browser tests (headless Chrome, WebGPU)",
    cwd: "web",
    cmd: "npm",
    args: ["test"],
    before: () => ensureInstalled("web"),
  });
}

function run(cmd, args, options) {
  // `shell` makes npm resolve to npm.cmd on Windows.
  return spawnSync(cmd, args, { stdio: "inherit", shell: process.platform === "win32", ...options });
}

function ensureInstalled(dir) {
  if (!existsSync(join(root, dir, "node_modules"))) {
    console.log(`installing dependencies in ${dir} ...`);
    const r = run("npm", ["ci"], { cwd: join(root, dir) });
    if (r.status !== 0) process.exit(r.status ?? 1);
  }
}

const results = [];
for (const step of steps) {
  console.log(`\n=== ${step.name} ===`);
  step.before?.();
  const started = Date.now();
  const r = run(step.cmd, step.args, {
    cwd: join(root, step.cwd ?? "."),
    env: { ...process.env, ...step.env },
  });
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  const ok = r.status === 0;
  results.push({ name: step.name, ok, seconds });
  if (!ok) break;
}

console.log("\n=== summary ===");
for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}  (${r.seconds}s)`);
const failed = results.some((r) => !r.ok);
if (skipBrowser) console.log("note: browser tests were skipped (--no-browser)");
console.log(failed ? "\nNOT safe to push." : "\nAll checks passed.");
process.exit(failed ? 1 : 0);
