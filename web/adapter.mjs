// Which WebGPU adapter does each Chrome launch mode actually get?
import http from "node:http";
import puppeteer from "puppeteer-core";
import { chromePath } from "./chrome.mjs";

const server = http.createServer((_, res) => res.writeHead(200, { "content-type": "text/html" }).end("<!doctype html><title>x</title>"));
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/`;

const chrome = chromePath();
const modes = {
  "headless (default flags)": { headless: true, args: ["--enable-unsafe-webgpu"] },
  "headless + ignore blocklist": { headless: true, args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"] },
  "headless + d3d12 adapter": { headless: true, args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist", "--use-webgpu-adapter=d3d12", "--use-angle=d3d11"] },
  "headed": { headless: false, args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"] },
};
for (const [name, opts] of Object.entries(modes)) {
  const browser = await puppeteer.launch({ executablePath: chrome, ...opts });
  try {
    const page = await browser.newPage();
    await page.goto(url);
    const info = await page.evaluate(`(async () => {
      const a = await navigator.gpu.requestAdapter();
      if (!a) return "no adapter";
      const i = a.info ?? (await a.requestAdapterInfo?.());
      return JSON.stringify({ vendor: i.vendor, architecture: i.architecture, description: i.description, device: i.device, fallback: a.isFallbackAdapter });
    })()`);
    console.log(name.padEnd(30), info);
  } catch (e) {
    console.log(name.padEnd(30), "ERROR", e.message);
  } finally {
    await browser.close();
  }
}
server.close();
