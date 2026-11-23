// Runs before `npm pack` / `npm publish`: put the licenses next to the package so
// they are part of the tarball. The repository keeps one copy, at its root.
import { copyFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(packageDir, "../..");

for (const name of ["LICENSE-MIT", "LICENSE-APACHE"]) {
  const source = join(repoRoot, name);
  if (!existsSync(source)) {
    console.error(`prepack: ${source} is missing`);
    process.exit(1);
  }
  copyFileSync(source, join(packageDir, name));
}
if (!existsSync(join(packageDir, "dist/index.js"))) {
  console.error("prepack: dist/ is missing. Run `npm run build` first.");
  process.exit(1);
}
// stderr, so `npm pack --json` keeps a clean, parseable stdout.
console.error("prepack: licenses copied");
