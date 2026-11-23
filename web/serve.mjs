// Static file server for the demo pages: node serve.mjs [port]
// Serves the p4 folder on 127.0.0.1 only. Open http://127.0.0.1:8765/web/viewer.html
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const port = Number(process.argv[2] ?? 8765);
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".map": "application/json", ".png": "image/png" };
http
  .createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    const file = path.join(root, decodeURIComponent(url.pathname === "/" ? "/web/viewer.html" : url.pathname));
    if (!file.startsWith(root) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      res.writeHead(404).end("not found");
      return;
    }
    res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream", "cache-control": "no-store" });
    fs.createReadStream(file).pipe(res);
  })
  .listen(port, "127.0.0.1", () => console.log(`serving ${root} at http://127.0.0.1:${port}/`));
