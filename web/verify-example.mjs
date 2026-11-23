// Build one example app against the PACKED package (the tarball `npm publish`
// would upload, not a symlink), start it, and check it in a real browser.
//
//   node web/verify-example.mjs react-vite
//   node web/verify-example.mjs react-vite --dev      # use the dev server instead of a production build
//
// Needs: the package built (`npm run build` in packages/sociogram), Chrome or Edge
// with WebGPU (CHROME=... to choose), and network access for `npm install`.
//
// Every example follows the same contract so one script can test them all:
//   #toggle  button that unmounts / mounts the graph
//   #status  element with data-status="ready" once the view is ready
//   #clicks  number of node clicks, #hover  hovered node index or "none"
//   .graph   the box that contains the graph
// and an `example.json` with { "build": [cmd...], "start": [cmd...] } where
// "{port}" is replaced by a free port.
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";
import { PNG } from "pngjs";
import { chromePath } from "./chrome.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const name = process.argv[2];
const dev = process.argv.includes("--dev");
if (!name) {
  console.error("usage: node web/verify-example.mjs <example-name> [--dev]");
  process.exit(2);
}
const exampleDir = join(repo, "examples", name);
if (!existsSync(exampleDir)) {
  console.error(`no such example: ${exampleDir}`);
  process.exit(2);
}
const config = JSON.parse(readFileSync(join(exampleDir, "example.json"), "utf8"));
const isWin = process.platform === "win32";
const tmp = realpathSync.native(tmpdir());

const results = [];
const check = (label, ok, detail = "") => {
  results.push(!!ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${label}${detail ? "  (" + detail + ")" : ""}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// One command line for the shell, with arguments quoted (paths contain spaces on Windows).
const shellLine = (cmd, args) =>
  [cmd, ...args].map((a) => (/[\s"&|<>^()]/.test(a) ? `"${a.replaceAll('"', '\\"')}"` : a)).join(" ");

function run(cmd, args, cwd, label) {
  const started = Date.now();
  const r = spawnSync(shellLine(cmd, args), { cwd, shell: true, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  const seconds = ((Date.now() - started) / 1000).toFixed(0);
  if (r.status !== 0) {
    console.log(`FAIL  ${label} (${seconds}s)`);
    console.log(((r.stdout ?? "") + (r.stderr ?? "")).split("\n").slice(-40).join("\n"));
    process.exit(1);
  }
  console.log(`ok    ${label} (${seconds}s)`);
  return r.stdout ?? "";
}

async function freePort() {
  return new Promise((resolvePort) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolvePort(port));
    });
  });
}

// ---- 1. pack the package exactly as it would be published
const packDir = mkdtempSync(join(tmp, "sociogram-pack-"));
const packOut = run("npm", ["pack", "--pack-destination", packDir, "--json"], join(repo, "packages/sociogram"), "npm pack");
// Lifecycle scripts may print before the JSON; take the JSON block at the end.
const packInfo = JSON.parse(packOut.slice(packOut.search(/\[\s*\{\s*"id"/)))[0];
const tarball = join(packDir, packInfo.filename);
const packedFiles = packInfo.files.map((f) => f.path);
check("tarball contains the wasm engine", packedFiles.some((p) => p.endsWith("sociogram_web_bg.wasm")));
check("tarball contains every entry point", ["react", "vue", "element", "index"].every((e) => packedFiles.includes(`dist/${e}.js`)));
check("tarball has no sources, tests or secrets", !packedFiles.some((p) => /^(src|test|scripts|node_modules)\//.test(p) || p.endsWith(".env")));
check(
  "tarball includes both licenses and a README",
  ["LICENSE-MIT", "LICENSE-APACHE", "README.md", "package.json"].every((f) => packedFiles.includes(f)),
);

// ---- 2. copy the example, install deps and the tarball
const work = join(mkdtempSync(join(tmp, "sociogram-example-")), name);
cpSync(exampleDir, work, {
  recursive: true,
  filter: (src) => !/[\\/](node_modules|dist|\.next|\.nuxt|\.output|\.angular)([\\/]|$)/.test(src),
});
run("npm", ["install", "--no-audit", "--no-fund", tarball], work, "npm install (from the tarball)");

// ---- 3. build and start
const port = await freePort();
const sub = (args) => args.map((a) => a.replaceAll("{port}", String(port)));
const startCmd = sub(dev ? config.dev : config.start);
if (!dev && config.build) run(config.build[0], sub(config.build.slice(1)), work, `build (${config.build.join(" ")})`);

// `--build-only`: stop after a successful build. Needs no GPU, so CI can run it.
if (process.argv.includes("--build-only")) {
  rmSync(packDir, { recursive: true, force: true });
  const failedChecks = results.filter((r) => !r).length;
  console.log(failedChecks === 0 ? `\n${name}: built from the packed package, ${results.length} checks passed` : `\n${name}: ${failedChecks} of ${results.length} CHECKS FAILED`);
  process.exit(failedChecks === 0 ? 0 : 1);
}
const server = spawn(shellLine(startCmd[0], startCmd.slice(1)), { cwd: work, shell: true, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, PORT: String(port) } });
let serverLog = "";
server.stdout.on("data", (d) => (serverLog += d));
server.stderr.on("data", (d) => (serverLog += d));
const stopServer = () => {
  if (isWin) spawnSync("taskkill", ["/pid", String(server.pid), "/T", "/F"], { stdio: "ignore" });
  else server.kill("SIGTERM");
};

const base = `http://127.0.0.1:${port}/`;
let up = false;
// Wait up to 4 minutes: a cold dev server compiles on its first start, and some
// (Nuxt) answer 503 "starting" before they are ready, so wait after any non-OK
// answer too, not only after a refused connection.
for (let i = 0; i < 480 && !up; i++) {
  try {
    up = (await fetch(base)).ok;
  } catch {
    /* not listening yet */
  }
  if (!up) await sleep(500);
}
check(`server answers (${dev ? "dev" : "production"} mode)`, up);
if (!up) {
  console.log(serverLog.split("\n").slice(-30).join("\n"));
  stopServer();
  process.exit(1);
}

// `--serve`: stop here and leave the server running so a person can open it.
if (process.argv.includes("--serve")) {
  console.log(`\nSERVING ${base}\nPress Ctrl+C (or stop this process) to shut it down.`);
  process.on("SIGINT", () => {
    stopServer();
    process.exit(0);
  });
  await new Promise(() => {});
}

// ---- 4. check it in a browser
const browser = await puppeteer.launch({
  executablePath: chromePath(),
  headless: true,
  args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1000, height: 800, deviceScaleFactor: 1 });
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (["error", "warning"].includes(m.type()) && !/favicon|DevTools|Download the (React|Vue)/i.test(m.text())) {
      const where = m.location()?.url ? ` [${m.location().url}]` : "";
      problems.push(`console.${m.type()}: ${m.text()}${where}`);
    }
  });
  page.on("requestfailed", (r) => {
    if (!/favicon/.test(r.url())) problems.push(`request failed: ${r.url()}`);
  });
  page.on("response", (r) => {
    if (r.status() >= 400 && !/favicon/.test(r.url())) problems.push(`HTTP ${r.status()}: ${r.url()}`);
  });

  const response = await page.goto(base, { waitUntil: "domcontentloaded" });
  const html = await response.text();
  if (config.ssr) {
    // The page really was rendered on the server (the controls are in the HTML), and nothing in it needs a browser.
    check("the server rendered the page (controls are in the HTML)", /id="toggle"/.test(html) && /id="status"/.test(html));
    check("server-rendered HTML does not contain a canvas", !/<canvas/i.test(html));
  }

  const ready = async () => {
    try {
      await page.waitForSelector('#status[data-status="ready"]', { timeout: 90000 });
      return true;
    } catch {
      return false;
    }
  };
  // Counts canvases everywhere, including inside shadow roots.
  const canvasCount = () =>
    page.evaluate(() => {
      const count = (root) => {
        let n = root.querySelectorAll("canvas").length;
        for (const el of root.querySelectorAll("*")) if (el.shadowRoot) n += count(el.shadowRoot);
        return n;
      };
      return count(document);
    });

  check("becomes ready", await ready(), await page.evaluate(() => document.querySelector("#status")?.textContent ?? "no #status"));
  check("exactly one canvas after the first mount (no leftover from a StrictMode remount)", (await canvasCount()) === 1, `found ${await canvasCount()}`);

  await sleep(2500); // let the layout run
  const box = await page.evaluate(() => {
    const r = document.querySelector(".graph").getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  });
  const lit = async () => {
    const png = PNG.sync.read(await page.screenshot({ clip: box }));
    let n = 0;
    for (let i = 0; i < png.data.length; i += 4) if (png.data[i] + png.data[i + 1] + png.data[i + 2] > 150) n++;
    return n;
  };
  const litPixels = await lit();
  check("the graph is drawn", litPixels > 400, `${litPixels} lit pixels`);
  mkdirSync(join(repo, "web", "example-shots"), { recursive: true });
  await page.screenshot({ path: join(repo, "web", "example-shots", `${name}${dev ? "-dev" : ""}.png`) });

  // hover and click
  let found;
  for (let y = box.y + 30; y < box.y + box.height - 30 && !found; y += 8) {
    for (let x = box.x + 30; x < box.x + box.width - 30; x += 8) {
      await page.mouse.move(x, y);
      await sleep(3);
      if ((await page.$eval("#hover", (e) => e.textContent)) !== "none") {
        found = { x, y };
        break;
      }
    }
  }
  check("hovering a node reports its index", !!found);
  if (found) {
    await page.mouse.click(found.x, found.y);
    await sleep(100);
    check("clicking a node is reported", (await page.$eval("#clicks", (e) => e.textContent)) === "1");
  }

  // unmount and mount again, several times: the GPU must be released and re-acquired cleanly
  let remountOk = true;
  for (let i = 1; i <= 3; i++) {
    await page.click("#toggle");
    await sleep(300);
    if ((await canvasCount()) !== 0) remountOk = false;
    await page.click("#toggle");
    if (!(await ready()) || (await canvasCount()) !== 1) remountOk = false;
  }
  check("unmount and mount three times leaves exactly one working canvas", remountOk);
  await sleep(1500);
  check("the graph is drawn again after the remounts", (await lit()) > 400);

  // Consumers' build tools should not warn about the package (for example a source map that points to files that are not shipped).
  const logWarnings = serverLog.split("\n").filter((l) => /points to missing source files|Failed to resolve|Module not found/i.test(l));
  check("no package warnings in the server log", logWarnings.length === 0, logWarnings.slice(0, 2).join(" | ").replace(/\x1b\[[0-9;]*m/g, ""));

  const unique = [...new Set(problems)];
  check("no console errors, warnings or failed requests", unique.length === 0, unique.slice(0, 3).join(" | "));
  if (unique.length) console.log(unique.map((p) => "  - " + p).join("\n"));
} finally {
  await browser.close();
  stopServer();
  await sleep(300);
  rmSync(packDir, { recursive: true, force: true });
}

const failed = results.filter((r) => !r).length;
console.log(failed === 0 ? `\n${name}: ALL ${results.length} CHECKS PASSED` : `\n${name}: ${failed} of ${results.length} CHECKS FAILED`);
process.exit(failed === 0 ? 0 : 1);
