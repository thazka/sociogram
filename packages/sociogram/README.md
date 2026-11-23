# sociogram

GPU-accelerated graph visualization for the browser. A Rust engine compiled to WebAssembly lays out and draws large networks on the GPU through WebGPU.

> Early software (0.x): the API will change. Full documentation, examples and the project's limits are in the [repository](https://github.com/thazka/sociogram#readme).

```bash
npm install sociogram
```

One package, with an entry point per framework. React, Vue and Angular are optional peer dependencies.

```ts
import { SociogramView } from "sociogram";                    // plain TypeScript
import { Sociogram } from "sociogram/react";                   // React 18/19, Next.js
import { Sociogram } from "sociogram/vue";                     // Vue 3, Nuxt
import { defineSociogramElement } from "sociogram/element";    // <sociogram-view>: Angular, Svelte, plain HTML
```

```ts
const view = await SociogramView.create(document.querySelector("canvas")!);
view.setData({ nodeCount: 3, edges: new Uint32Array([0, 1, 1, 2]) });
view.on("nodeClick", (index) => console.log("clicked", index));
```

Requires a browser with WebGPU (a current Chrome or Edge). `create()` rejects with `WebGpuUnsupportedError` otherwise.

Licensed under either of Apache-2.0 or MIT, at your option.
