# Contributing to sociogram

Thanks for helping. This is a small project, so the rules are short.

## The one hard rule: original work only

sociogram is a clean-room implementation. **Do not copy or translate code, shaders, assets, text or API designs from other graph visualization projects or products** (including Cosmograph and cosmos.gl), and do not paste in decompiled or minified code. Many of them have licenses that are incompatible with ours.

- Ideas, public algorithms and published papers are fine (force-directed layout, Barnes-Hut and grid approximations, instanced rendering, GPU atomics). Write the code yourself.
- If you read another project's source closely, say so in the pull request and keep that part of the work to someone else, so the implementation stays independent.
- Say where a non-obvious algorithm comes from (paper, book, documentation) in a comment.

Pull requests that contain copied code will be closed.

## Setup

See "Build from source" in the [README](README.md). You need Rust (stable), the `wasm32-unknown-unknown` target, a matching `wasm-bindgen-cli` (the version is pinned in `crates/sociogram-web/Cargo.toml`), and Node.js.

## Before you open a pull request

```bash
node scripts/verify.mjs               # fmt, clippy (native + wasm32), tests, package build, browser tests
node scripts/verify.mjs --no-browser  # the same without the headless-Chrome checks
```

**CI does not run the GPU tests.** Free CI runners have no GPU, so those tests skip themselves there (CI prints how many were skipped). That means a green CI badge says nothing about rendering or layout. `scripts/verify.mjs` sets `SOCIOGRAM_REQUIRE_GPU=1`, so on your machine a missing GPU is a failure, not a skip. Please run it for any change to `sociogram-gpu`, `sociogram-web` or the browser API, try more than one backend when you can (`WGPU_BACKEND=dx12`, `vulkan`, `gl`, `metal`), and say in the pull request what you ran it on.

To run it automatically before every push: `git config core.hooksPath .githooks`.

**Changes to the npm package** (`packages/sociogram`) also need its unit, server-rendering and type tests (`npm test` and `npm run typecheck` there), and, if you touch a wrapper or the build, the example apps. `node web/verify-example.mjs <example>` packs the package like `npm publish` does, installs it into a temporary copy of the example, builds it and checks it in headless Chrome; add `--dev` for the dev server or `--serve` to open it yourself. See [examples/README.md](examples/README.md).

A new framework wrapper should be a thin layer over `mountSociogram` (`packages/sociogram/src/mount.ts`), which owns the part that is easy to get wrong: a component unmounted before the view finished creating, and StrictMode mounting twice. It needs an example that passes the same checks as the others.

Tests that need a GPU use `sociogram_gpu::testing::test_context()`. It skips on machines without a real GPU, and the environment variables `SOCIOGRAM_REQUIRE_GPU=1` (fail instead of skip) and `SOCIOGRAM_SOFTWARE_GPU=1` (also run on software rasterizers such as lavapipe) change that. `SOCIOGRAM_NO_GPU=1` pretends there is no GPU, which runs the suite the way CI sees it: every GPU test must skip cleanly and none may fail.

When you write a GPU test, a skipped test must return early (`let Some(ctx) = context() else { return };`). Never return made-up data from a helper to keep a skipped test passing.

## Code guidelines

- Write the test first and watch it fail for the right reason. Bug fixes need a test that fails without the fix.
- Validate input at the boundary (what JavaScript passes in) and return clear errors. Do not panic on bad input.
- Keep files under 500 lines. Split by responsibility.
- `sociogram-core` stays free of GPU code and dependencies.
- Instance structs that go to the GPU must have no padding between fields that the vertex attributes pack tightly; there are tests for the sizes.
- No `unsafe` without a comment explaining why it is sound.
- Measure before optimizing and put the numbers in the pull request (machine, GPU, browser, graph size). Performance claims without numbers will be asked for them.
- User-visible changes belong in the README's status section.

## Pull requests

- Keep them focused; one change per pull request.
- Describe what changed and why, how you tested it, and on which hardware and browser.
- Be open about what you did not verify.

## License of contributions

Unless you state otherwise, any contribution you submit is licensed under the same terms as the project: `MIT OR Apache-2.0`.

## Conduct

Be respectful. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
