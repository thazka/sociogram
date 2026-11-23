// Browser benchmark: node bench.mjs [n] [edgesPerNode]
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import puppeteer from "puppeteer-core";
import { chromePath } from "./chrome.mjs";

const root = path.resolve(import.meta.dirname, "..");
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".map": "application/json" };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const file = path.join(root, url.pathname === "/" ? "web/bench.html" : url.pathname);
  if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const n = process.argv[2] ?? "100000";
const e = process.argv[3] ?? "5";

const browser = await puppeteer.launch({
  executablePath: chromePath(),
  headless: true,
  userDataDir: process.env.USERDATA || undefined,
  args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist", "--disable-gpu-vsync", "--disable-frame-rate-limit"],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 800, height: 600, deviceScaleFactor: 1 });
  page.on("pageerror", (err) => console.log("[pageerror]", err.message));
  await page.goto(`http://127.0.0.1:${port}/?n=${n}&e=${e}&ms=${process.argv[4] ?? 6000}`);
  await page.waitForFunction("window.__bench || window.__error", { timeout: 180000 });
  const err = await page.evaluate("window.__error ?? null");
  if (err) { console.log("ERROR:", err); process.exitCode = 1; }
  else {
    const b = await page.evaluate("({ ...window.__bench, setDataMs: window.__setDataMs })");
    const lt = await page.evaluate("window.__longTasks.filter(t => t[1] >= 200)");
    console.log("long tasks >=200 ms [start, duration]:", JSON.stringify(lt));
    console.log(JSON.stringify({ ...b, fps: Math.round(b.fps * 10) / 10, worstFrameMs: Math.round(b.worstFrameMs), setDataMs: Math.round(b.setDataMs), medianMs: Math.round(b.medianMs * 10) / 10, p95Ms: Math.round(b.p95Ms * 10) / 10 }));
  }
} finally {
  await browser.close();
  server.close();
}
