// node selection-probe.mjs: selection timing at 100,000 nodes in headless Chrome.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import puppeteer from "puppeteer-core";
import { chromePath } from "./chrome.mjs";

const root = path.resolve(import.meta.dirname, "..");
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".map": "application/json" };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const file = path.join(root, url.pathname === "/" ? "web/selection-probe.html" : url.pathname);
  if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const browser = await puppeteer.launch({
  executablePath: chromePath(),
  headless: !process.env.HEADED,
  args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 800, height: 600, deviceScaleFactor: 1 });
  page.on("pageerror", (err) => console.log("[pageerror]", err.message));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction("window.__ready || window.__error", { timeout: 180000 });
  const err = await page.evaluate("window.__error ?? null");
  if (err) { console.log("ERROR:", err); process.exitCode = 1; }
  else console.log(JSON.stringify(await page.evaluate("window.__log"), null, 2));
} finally {
  await browser.close();
  server.close();
}
