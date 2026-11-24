import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import puppeteer from "puppeteer-core";
import { PNG } from "pngjs";
import { chromePath } from "./chrome.mjs";

const root = path.resolve(import.meta.dirname, "..");
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".map": "application/json" };
const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  const file = path.join(root, url.pathname === "/" ? "web/demo.html" : url.pathname);
  if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

const results = [];
const check = (name, ok, detail = "") => {
  results.push(!!ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  (" + detail + ")" : ""}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await puppeteer.launch({
  executablePath: chromePath(),
  headless: true,
  args: ["--enable-unsafe-webgpu", "--ignore-gpu-blocklist"],
});
try {
  // ---- main scenario at device pixel ratio 2
  const page = await browser.newPage();
  await page.setViewport({ width: 480, height: 360, deviceScaleFactor: 2 });
  page.on("pageerror", (e) => console.log("[pageerror]", e.message));
  await page.goto(base + "/");
  await page.waitForFunction("window.__ready || window.__error", { timeout: 60000 });
  check("view created", await page.evaluate("!!window.__ready"), await page.evaluate("window.__error ?? ''"));

  const size = await page.evaluate("[document.getElementById('c').width, document.getElementById('c').height]");
  check("drawing buffer is device resolution (HiDPI)", size[0] === 960 && size[1] === 720, size.join("x"));

  await sleep(6000); // let the layout run and snapshots arrive
  const lit = async () => {
    const png = PNG.sync.read(await page.screenshot());
    let n = 0;
    for (let i = 0; i < png.data.length; i += 4) if (png.data[i] + png.data[i + 1] + png.data[i + 2] > 120) n++;
    return { n, w: png.width, h: png.height };
  };
  const shot = await lit();
  check("screenshot is 960x720 and shows the graph", shot.w === 960 && shot.h === 720 && shot.n > 500, `${shot.w}x${shot.h}, lit ${shot.n}`);
  fs.writeFileSync(path.join(import.meta.dirname, "shot-dpr2.png"), await page.screenshot());

  // hover: sweep the pointer until a node reports hover
  let hoverPos;
  for (let y = 40; y < 330 && !hoverPos; y += 6) {
    for (let x = 40; x < 440; x += 6) {
      await page.mouse.move(x, y);
      await sleep(4);
      const ev = await page.evaluate("window.__events.filter(e => e[0]==='hover' && e[1] !== null).length");
      if (ev > 0) { hoverPos = { x, y }; break; }
    }
  }
  check("hover event fires with a node index", !!hoverPos, hoverPos ? `${hoverPos.x},${hoverPos.y}` : "");
  check("cursor becomes a pointer over a node", hoverPos && (await page.evaluate("document.getElementById('c').style.cursor")) === "pointer");

  if (hoverPos) {
    await page.mouse.click(hoverPos.x, hoverPos.y);
    await sleep(50);
    const clicks = await page.evaluate("window.__events.filter(e => e[0]==='click')");
    check("click event fires with a valid node index", clicks.length === 1 && clicks[0][1] >= 0 && clicks[0][1] < 400, JSON.stringify(clicks));
  }

  // dragging pans without producing a click
  await page.evaluate("window.__events.length = 0");
  await page.mouse.move(240, 180);
  await page.mouse.down();
  await page.mouse.move(300, 220, { steps: 6 });
  await page.mouse.up();
  await sleep(200);
  check("dragging does not count as a click", (await page.evaluate("window.__events.filter(e => e[0]==='click').length")) === 0);

  // wheel zoom changes what is on screen
  const a = PNG.sync.read(await page.screenshot());
  await page.mouse.move(240, 180);
  await page.mouse.wheel({ deltaY: -400 });
  await sleep(300);
  const b = PNG.sync.read(await page.screenshot());
  let diff = 0;
  for (let i = 0; i < a.data.length; i += 4) if (a.data[i] !== b.data[i] || a.data[i + 1] !== b.data[i + 1]) diff++;
  check("wheel zoom changes the picture", diff > 2000, `${diff} pixels differ`);

  // resize follows the CSS size at the device pixel ratio
  await page.evaluate("document.getElementById('c').style.width = '300px'; document.getElementById('c').style.height = '200px';");
  await sleep(400);
  const resized = await page.evaluate("[document.getElementById('c').width, document.getElementById('c').height]");
  check("resize keeps device resolution", resized[0] === 600 && resized[1] === 400, resized.join("x"));

  // screenshot() API
  const info = await page.evaluate(`(async () => {
    const blob = await window.__view.screenshot();
    const head = new Uint8Array(await blob.slice(0, 8).arrayBuffer());
    const bmp = await createImageBitmap(blob);
    return { type: blob.type, size: blob.size, head: Array.from(head), w: bmp.width, h: bmp.height };
  })()`);
  const pngSig = [137, 80, 78, 71, 13, 10, 26, 10];
  check("screenshot() returns a PNG at buffer size", JSON.stringify(info.head) === JSON.stringify(pngSig) && info.w === 600 && info.h === 400, JSON.stringify({ w: info.w, h: info.h, size: info.size }));

  // whenIdle(): resolves once the GPU has finished, repeatedly
  const idle = await page.evaluate(`(async () => {
    const t = performance.now();
    await window.__view.whenIdle();
    await window.__view.whenIdle();
    return performance.now() - t;
  })()`);
  check("whenIdle() resolves (twice in a row)", idle >= 0 && idle < 10000, `${Math.round(idle)} ms`);

  // two isolated nodes that are really inside the canvas (earlier checks panned, zoomed and resized it)
  const pair = await page.evaluate(`(() => {
    const v = window.__view;
    const r = document.querySelector("canvas").getBoundingClientRect();
    const pts = [];
    for (let i = 0; i < 400; i++) {
      const p = v.nodeScreenPosition(i);
      if (p === undefined) break;
      if (p.x > r.left + 30 && p.x < r.right - 30 && p.y > r.top + 30 && p.y < r.bottom - 30) pts.push({ i, x: p.x, y: p.y });
    }
    const alone = (a) => pts.every((b) => b.i === a.i || Math.hypot(a.x - b.x, a.y - b.y) > 25);
    const a = pts.find(alone);
    const b = a && pts.find((p) => alone(p) && Math.hypot(p.x - a.x, p.y - a.y) > 120);
    return a && b ? { a, b } : null;
  })()`);
  check("two isolated nodes are on the canvas for the pointer checks", pair !== null, JSON.stringify(pair));
  if (pair !== null) {
    // drag node a: it follows the pointer, the camera (and the far node b) do not move
    await page.mouse.move(pair.a.x, pair.a.y);
    await page.mouse.down();
    await page.mouse.move(pair.a.x + 40, pair.a.y + 25, { steps: 8 });
    // Measure while the pointer is still down: after release the node rejoins the layout
    // and its links pull it back, by design.
    await sleep(400);
    const held = await page.evaluate(`(() => ({ a: window.__view.nodeScreenPosition(${pair.a.i}), b: window.__view.nodeScreenPosition(${pair.b.i}) }))()`);
    await page.mouse.up();
    check(
      "dragging a node moves that node, not the camera",
      Math.hypot(held.a.x - (pair.a.x + 40), held.a.y - (pair.a.y + 25)) < 20 && Math.hypot(held.b.x - pair.b.x, held.b.y - pair.b.y) < 20,
      JSON.stringify({ dragged: [held.a.x - pair.a.x, held.a.y - pair.a.y], control: [held.b.x - pair.b.x, held.b.y - pair.b.y] }),
    );
    await sleep(400);

    // rectangle tool: drag over the whole canvas selects nodes and reports it
    await page.evaluate(`(() => { window.__sel = null; window.__view.on("selectionChange", (e) => { window.__sel = e; }); window.__view.setSelectionTool("rectangle"); })()`);
    const box = await page.evaluate(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
    await page.mouse.move(box.x + 4, box.y + 4);
    await page.mouse.down();
    await page.mouse.move(box.x + box.w - 4, box.y + box.h - 4, { steps: 6 });
    await page.mouse.up();
    await sleep(200);
    const selected = await page.evaluate(`(() => window.__sel ? { n: window.__sel.nodes.length, links: window.__sel.linkCount } : null)()`);
    check("rectangle tool selects nodes and emits selectionChange", selected !== null && selected.n > 0, JSON.stringify(selected));
    await page.screenshot({ path: path.join(import.meta.dirname, "shot-selection.png") });

    // a small selection: everything else is dimmed (look at shot-selection-single.png)
    await page.evaluate(`window.__view.setSelection([${pair.a.i}])`);
    await sleep(200);
    await page.screenshot({ path: path.join(import.meta.dirname, "shot-selection-single.png") });

    // back to no tool: clicking empty canvas clears the selection
    await page.evaluate(`window.__view.setSelectionTool("none")`);
    await page.mouse.click(box.x + 3, box.y + 3);
    await sleep(150);
    const cleared = await page.evaluate(`window.__view.getSelection().length`);
    check("clicking empty canvas clears the selection", cleared === 0, String(cleared));

    // pin node b, then let the layout run: it must not move
    await page.evaluate(`window.__view.setPinnedNodes([${pair.b.i}])`);
    await sleep(500); // the pin lands and the position snapshot catches up
    const p0 = await page.evaluate(`window.__view.nodeScreenPosition(${pair.b.i})`);
    await page.evaluate(`window.__view.reheat()`);
    await sleep(1500);
    const p1 = await page.evaluate(`window.__view.nodeScreenPosition(${pair.b.i})`);
    check("a pinned node stays put while the layout runs", Math.hypot(p1.x - p0.x, p1.y - p0.y) < 1.5, `${(p1.x - p0.x).toFixed(2)}, ${(p1.y - p0.y).toFixed(2)}`);
  }

  const badIndices = await page.evaluate(`(() => { const r = []; for (const v of [[1e9], [1.5], [-1]]) { try { window.__view.setPinnedNodes(v); r.push("no error"); } catch (e) { r.push(e instanceof RangeError ? "RangeError" : "other: " + e.message); } } return r; })()`);
  check("unknown or malformed node indices throw a RangeError", badIndices.every((r) => r === "RangeError"), JSON.stringify(badIndices));
  await page.evaluate(`window.__view.setPinnedNodes([])`);

  // invalid data is rejected and does not break the view
  const bad = await page.evaluate(`(() => { try { window.__view.setData({ nodeCount: 2, edges: [0, 9] }); return "no error"; } catch (e) { return e.message; } })()`);
  check("invalid data throws a clear error", /invalid graph/i.test(bad), bad);
  await sleep(200);
  check("render loop survives invalid data", await page.evaluate("window.__events.filter(e => e[0]==='error').length === 0"));

  // link fade API: valid modes work, invalid ranges are rejected with a clear error
  const fadeApi = await page.evaluate(`(() => {
    const v = window.__view;
    const out = { ok: true, errors: [] };
    try { v.setLinkFade("auto"); v.setLinkFade("off"); v.setLinkFade({ startPx: 50, endPx: 200 }); v.setLinkFade("auto"); } catch (e) { out.ok = false; out.errors.push(String(e)); }
    const bad = [{ startPx: 200, endPx: 50 }, { startPx: -1, endPx: 10 }, { startPx: NaN, endPx: 10 }, { startPx: 10, endPx: Infinity }];
    out.rejected = bad.map((m) => { try { v.setLinkFade(m); return false; } catch (e) { return e instanceof RangeError; } });
    return out;
  })()`);
  check("setLinkFade accepts auto / off / a range", fadeApi.ok, fadeApi.errors.join("; "));
  check("setLinkFade rejects invalid ranges with RangeError", fadeApi.rejected.every(Boolean), JSON.stringify(fadeApi.rejected));

  // destroy is idempotent
  const destroyed = await page.evaluate(`(() => { const v = window.__view; v.destroy(); v.destroy(); let threw = false; try { v.setData({ nodeCount: 0, edges: [] }); } catch { threw = true; } return { d: v.destroyed, threw }; })()`);
  check("destroy() twice is safe and later calls throw", destroyed.d && destroyed.threw);
  await page.close();

  // ---- no WebGPU
  const p2 = await browser.newPage();
  await p2.evaluateOnNewDocument("Object.defineProperty(navigator, 'gpu', { value: undefined, configurable: true }); delete Navigator.prototype.gpu;");
  await p2.goto(base + "/");
  await p2.waitForFunction("window.__ready || window.__error", { timeout: 30000 });
  const err = await p2.evaluate("window.__error ?? ''");
  check("clear error when WebGPU is missing", err.startsWith("WebGpuUnsupportedError"), err);
} finally {
  await browser.close();
  server.close();
}
const failed = results.filter((r) => !r).length;
console.log(failed === 0 ? `\nALL ${results.length} CHECKS PASSED` : `\n${failed} of ${results.length} CHECKS FAILED`);
process.exitCode = failed === 0 ? 0 : 1;
