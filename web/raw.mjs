// node raw.mjs "<query string>" ...   e.g. node raw.mjs "n=100000" "n=100000&rep=0"
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import puppeteer from "puppeteer-core";
import { chromePath } from "./chrome.mjs";

const root = path.resolve(import.meta.dirname, "..");
const types = { ".html": "text/html", ".js": "text/javascript", ".wgsl": "text/plain", ".wasm": "application/wasm" };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const file = path.join(root, url.pathname === "/" ? "web/raw.html" : url.pathname);
  if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const queries = process.argv.slice(2);
const browser = await puppeteer.launch({
  executablePath: chromePath(),
  headless: !process.env.HEADED,
  args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist", ...(process.env.FLAGS ? process.env.FLAGS.split(" ") : [])],
});
try {
  for (const q of queries) {
    const page = await browser.newPage();
    page.on("pageerror", (e) => console.log("[pageerror]", e.message));
    await page.goto(`http://127.0.0.1:${port}/?${q}`);
    await page.waitForFunction("window.__out", { timeout: 240000 });
    const o = await page.evaluate("window.__out");
    const gpu = o.gpuMs ? ` gpu acc ${o.gpuMs.accumulate.toFixed(1)} ms, step ${o.gpuMs.step.toFixed(1)} ms` : "";
    console.log(`${q.padEnd(44)} ${o.error ? "ERROR " + o.error : `${o.msPerStep} ms/step (first ${o.firstStepMs} ms)${gpu}`}${o.errors?.length ? " errors:" + o.errors.join("|") : ""}${o.compileMessages?.length ? " compile:" + o.compileMessages.join("|") : ""}`);
    await page.close();
  }
} finally {
  await browser.close();
  server.close();
}
