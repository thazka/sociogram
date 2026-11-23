# sociogram

GPU-accelerated graph visualization for the browser, written in Rust and compiled to WebAssembly. Force-directed layout and rendering both run on the GPU through WebGPU (wgpu), so large networks stay interactive.

> **Status: early (0.1).** It works and is tested, but the API will change and it is not yet published to npm or crates.io. See [Status and limits](#status-and-limits).

![sociogram showing 100,000 nodes grouped into communities](docs/images/100k.png)

## What it does

- Force-directed layout on the GPU (hierarchical-grid repulsion, springs, gravity). Tested at 100,000 nodes and 300,000 links.
- Renders nodes (anti-aliased circles) and links with per-node color and size.
- Pan, zoom to the cursor, hover and click on nodes, auto-fit camera.
- Sharp on high-DPI screens (the drawing buffer follows `devicePixelRatio`).
- `screenshot()` renders the current view offscreen and returns a PNG.
- Large graphs stay fast: long links fade out by on-screen length, and the detail adapts to the frame rate.
- Framework neutral. A plain `<canvas>` API today; wrappers for React, Vue and Angular are planned.

## Quick start

Not on npm yet, so build from source (see below). Then:

```ts
import { SociogramView } from "sociogram";

const canvas = document.querySelector("canvas")!; // give it a CSS size
const view = await SociogramView.create(canvas);

view.setData({
  nodeCount: 4,
  edges: new Uint32Array([0, 1, 1, 2, 2, 3, 3, 0]), // source, target, source, target, ...
  colors: new Float32Array([
    0.96, 0.45, 0.45, 1, //  RGBA per node
    0.45, 0.80, 0.55, 1,
    0.45, 0.60, 0.97, 1,
    0.97, 0.80, 0.40, 1,
  ]),
});

view.on("nodeClick", (index) => console.log("clicked node", index));
view.on("nodeHover", (index) => console.log("hover", index)); // null when leaving a node

const png: Blob = await view.screenshot();
view.destroy(); // releases the GPU; safe to call twice
```

`create()` rejects with `WebGpuUnsupportedError` when the browser cannot run WebGPU. Invalid data makes `setData()` throw and keeps the previous graph.

## Build from source

Requirements: Rust (stable), Node.js, and a browser with WebGPU (a current Chrome or Edge).

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # must match the crate version

cd packages/sociogram
npm install
npm run build          # builds the wasm engine and the TypeScript package

cd ../../web
npm install
npm run serve          # then open http://127.0.0.1:8765/web/viewer.html
```

The viewer lets you pick the number of nodes (up to 100,000), switch link detail, and take screenshots.

## Tests

```bash
node scripts/verify.mjs      # fmt, clippy, tests (GPU tests required), package build, browser checks
cargo test --workspace       # just the Rust tests; GPU tests skip themselves without a real GPU
```

CI runs formatting, clippy and the tests that need no GPU. The GPU and browser tests need a real GPU, so `scripts/verify.mjs` runs them locally (see [CONTRIBUTING.md](CONTRIBUTING.md)).

On Windows the engine defaults to DX12 (the Vulkan backend crashes on some Intel drivers). Set `WGPU_BACKEND=vulkan|dx12|gl|metal` to override. Set `CHROME` to point the browser tests at a specific executable.

## Repository layout

| Path | What |
|------|------|
| `crates/sociogram-core` | Graph storage (CSR), camera, picking index, reference layout. No GPU code, no dependencies. |
| `crates/sociogram-gpu` | wgpu renderer and the GPU layout (compute shaders). |
| `crates/sociogram-web` | Scene logic and the WebAssembly bindings. |
| `packages/sociogram` | The npm package: TypeScript API around the wasm engine. |
| `web` | Demo viewer, browser tests and benchmarks (not published). |

## Status and limits

Measured on one machine (integrated Intel UHD GPU, Windows 11, Chrome); expect different numbers elsewhere.

- 100,000 nodes and 305,000 links: about 46 frames per second once the layout has settled, and the layout converges into clear clusters on a community-structured test graph. Rendering long links is the main cost, which is why large graphs fade them out by default. Set `setLinkFade("off")` to draw everything.
- Layout speed: about 18 ms per step at 100,000 nodes natively and about 17 ms in the browser.
- A one-time stall of several seconds was seen at startup in the benchmark harness and is not yet explained.
- Only Chrome and Edge on one GPU have been tested. Firefox, Safari, high-end GPUs and fractional display scaling (125%, 150%) are untested.
- Not implemented yet: dragging nodes, selection (box and lasso), labels, clusters, link arrows and curves, node shapes and images, timeline filters, 3D, data loaders (CSV, Parquet, Arrow), and the framework wrappers.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first, in particular the rule about not copying code from other projects.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project shall be dual licensed as above, without any additional terms or conditions.

sociogram is an independent implementation and is not affiliated with any other graph visualization product.
