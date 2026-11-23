// node ready-probe.mjs [runs] [n]   — each run uses a fresh browser (fresh profile).
// Set USERDATA to reuse one profile directory (warm shader cache) across runs.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import puppeteer from "puppeteer-core";
import { chromePath } from "./chrome.mjs";

const root = path.resolve(import.meta.dirname, "..");
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".map": "application/json" };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const file = path.join(root, url.pathname === "/" ? "web/ready-probe.html" : url.pathname);
  if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const runs = Number(process.argv[2] ?? 3);
const n = process.argv[3] ?? "100000";

for (const idle of ["0", "1"]) {
  for (let i = 0; i < runs; i++) {
    const browser = await puppeteer.launch({
      executablePath: chromePath(),
      headless: !process.env.HEADED,
      userDataDir: process.env.USERDATA || undefined,
      args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"],
    });
    try {
      const page = await browser.newPage();
      await page.setViewport({ width: 800, height: 600, deviceScaleFactor: 1 });
      page.on("pageerror", (err) => console.log("[pageerror]", err.message));
      await page.goto(`http://127.0.0.1:${port}/?n=${n}&idle=${idle}`);
      await page.waitForFunction("window.__ready || window.__error", { timeout: 180000 });
      const err = await page.evaluate("window.__error ?? null");
      if (err) console.log(`idle=${idle} run ${i}: ERROR ${err}`);
      else console.log(`idle=${idle} run ${i}:`, JSON.stringify(await page.evaluate("window.__log")));
    } finally {
      await browser.close();
    }
  }
}
server.close();
