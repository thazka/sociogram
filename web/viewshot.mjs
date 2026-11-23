import puppeteer from "puppeteer-core";
import { chromePath } from "./chrome.mjs";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({
  executablePath: chromePath(),
  headless: true,
  args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"],
});
try {
  for (const [n, wait] of [[Number(process.argv[2] ?? 2000), Number(process.argv[3] ?? 9000)]]) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
    page.on("pageerror", (e) => console.log("[pageerror]", e.message));
    await page.goto(`http://127.0.0.1:8765/web/viewer.html?n=${n}`);
    await sleep(wait);
    const stats = await page.evaluate("document.getElementById('stats').textContent + ' | ' + document.getElementById('msg').textContent");
    console.log(n, "->", stats);
    await page.screenshot({ path: `viewer-${n}.png` });
    await page.close();
  }
} finally {
  await browser.close();
}
