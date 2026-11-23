# Examples

One small app per framework, all showing the same thing: a community graph with hover, click, fit, reheat and a button that unmounts and mounts the graph again.

| Example | Framework | How it uses sociogram |
|---------|-----------|-----------------------|
| `react-vite` | React 19 + Vite, StrictMode | `import { Sociogram } from "sociogram/react"` |
| `next` | Next.js 16 (App Router, server rendering) | the same component inside a client component |
| `vue-vite` | Vue 3 + Vite | `import { Sociogram } from "sociogram/vue"` |
| `nuxt` | Nuxt 4 (server rendering) | the same component, no `<ClientOnly>` needed |
| `angular` | Angular 22 (standalone, signals) | the `<sociogram-view>` Web Component with `CUSTOM_ELEMENTS_SCHEMA` |
| `element-vite` | plain TypeScript + Vite | `defineSociogramElement()` and `<sociogram-view>` |

## Running one

The examples depend on `sociogram`, which is not published to npm yet, so `npm install` inside an example folder will not find it. Use the runner instead. It packs the package exactly as `npm publish` would, installs that tarball into a temporary copy of the example, builds it, and serves it:

```bash
cd packages/sociogram && npm install && npm run build && cd ../..   # once
(cd web && npm install)                                              # once

node web/verify-example.mjs react-vite --serve    # build and keep serving; open the printed address
node web/verify-example.mjs react-vite            # build and run the browser checks
node web/verify-example.mjs react-vite --dev      # the same checks against the dev server
```

Your editor will show errors such as `Cannot find type definition file for 'vite/client'` in an example folder. That is expected: nothing is installed there. The temporary copy is where dependencies live.

## What the checks cover

For every example, in headless Chrome with WebGPU:

- the tarball contains the engine (`.wasm`), every entry point, and no sources or secrets;
- server-rendered HTML (Next, Nuxt) already contains the page but no canvas;
- the view becomes ready, there is exactly one canvas, and the graph is drawn;
- hovering a node reports its index and clicking it is reported;
- unmounting and mounting three times leaves one working canvas (no GPU leak, no StrictMode leftovers);
- no console errors, no failed requests, no package warnings in the server log.

Each example follows one contract so a single script can test them all: a `#toggle` button, a `#status` element with `data-status="ready"` when the view is ready, `#clicks` and `#hover` readouts, a `.graph` container, and an `example.json` with the build and start commands.
